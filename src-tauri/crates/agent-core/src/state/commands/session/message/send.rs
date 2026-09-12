//! The single turn-submission path (`agent_send_message` implementation).
//!
//! Every source of a turn — composer submit, force-send, queue flush,
//! plan-approval re-entry, mobile remote, background subagent and Agent Org
//! wakes — funnels through [`send_message_impl`]. It resolves identity, lazily
//! initializes the runtime, persists the identity/user-input snapshot, decides
//! between mid-turn steering and a fresh enqueue, and hands the scheduler a
//! closure that owns the turn's lifecycle from running-status promotion to
//! finalization.

use std::sync::Arc;

use crate::coordination::agent_member_interventions::{
    AgentMemberInterventionStore, EnqueueUserDirectedWorkParams, EnqueueUserDirectedWorkResult,
    DEFAULT_USER_DIRECTED_QUEUE_CAP,
};
use crate::coordination::agent_org_user_directed_work::{self, UserDirectedDeliveryStatus};
use crate::foundation::session_bridge::TurnIntentBridgeSource;
use crate::persistence::AgentResponse;
use crate::session::persistence as session_persistence;
use crate::state::commands::session::identity::{resolve_session_identity, IdentityOverrides};
use crate::state::commands::session::org_tasks;
use crate::state::AgentAppState;

use super::exec_mode::{resolve_agent_mode, restore_mode_before_plan_entry};
use super::org_wake::{
    promote_agent_org_user_directed_session_to_running, promote_agent_org_wake_session_to_running,
    resolve_agent_org_wake_mode,
};

pub(crate) const USER_DIRECTED_WAITING_ERROR_PREFIX: &str = "user_directed_waiting_for_yield:";
pub(crate) const USER_DIRECTED_CANCELLED_ERROR_PREFIX: &str = "user_directed_cancelled:";

/// Promote the exact scheduled Turn's Session immediately before processing.
/// A TaskExecution Task remains Pending here because its TaskAssigned input
/// has not been materialized yet; the turn processor starts the Task only
/// after that durable input is attached and immediately before Provider work.
pub(super) fn promote_turn_to_running_in_tx(
    conn: &rusqlite::Connection,
    session_id: &str,
    turn_intent_id: &str,
    wake_run_id: Option<&str>,
    intent_run_id: Option<&str>,
    is_user_directed_work: bool,
) -> Result<bool, String> {
    let persisted_context = if wake_run_id.is_some() || intent_run_id.is_some() {
        Some(
            crate::coordination::agent_org_turn_contexts::revalidate_context_with_connection(
                conn,
                session_id,
                turn_intent_id,
            )?,
        )
    } else {
        None
    };
    if let Some(context) = persisted_context.as_ref() {
        if context.turn_kind
            == crate::coordination::agent_org_turn_contexts::AgentOrgTurnKind::TaskExecution
            && !crate::coordination::agent_org_turn_contexts::member_dispatch_is_fifo_head_with_connection(
                conn,
                &context.org_run_id,
                context.dispatch_member_id.as_deref().ok_or_else(|| {
                    "TaskExecution context has no dispatch Member".to_string()
                })?,
                context.member_dispatch_sequence.ok_or_else(|| {
                    "TaskExecution context has no Member FIFO sequence".to_string()
                })?,
                false,
            )?
        {
            return Ok(false);
        }
    }
    if is_user_directed_work
        && !agent_org_user_directed_work::mark_turn_started_with_connection(
            conn,
            session_id,
            turn_intent_id,
        )?
    {
        return Ok(false);
    }

    let updated = if is_user_directed_work {
        conn.execute(
            "UPDATE agent_sessions SET status=?1,updated_at=?2 WHERE session_id=?3",
            rusqlite::params![
                crate::session::SessionStatus::Running.as_str(),
                chrono::Utc::now().to_rfc3339(),
                session_id,
            ],
        )
        .map_err(|error| error.to_string())?
    } else if let Some(run_id) = wake_run_id {
        promote_agent_org_wake_session_to_running(conn, run_id, session_id)?
    } else if let Some(run_id) = intent_run_id {
        promote_agent_org_user_directed_session_to_running(conn, run_id, session_id)?
    } else {
        conn.execute(
            "UPDATE agent_sessions SET status=?1, updated_at=?2 WHERE session_id=?3",
            rusqlite::params![
                crate::session::SessionStatus::Running.as_str(),
                chrono::Utc::now().to_rfc3339(),
                session_id,
            ],
        )
        .map_err(|error| error.to_string())?
    };
    if updated != 1 {
        if wake_run_id.is_some() || intent_run_id.is_some() {
            return Ok(false);
        }
        return Err(format!("session row missing at turn start: {session_id}"));
    }

    Ok(true)
}

fn should_dispatch_admitted_direct(
    duplicate: bool,
    turn_status: &str,
    allow_recovery: bool,
) -> bool {
    !duplicate || (allow_recovery && turn_status == "queued")
}

pub(super) fn ensure_agent_org_turn_is_runnable(
    run_id: &str,
    status: crate::coordination::agent_org_runs::AgentOrgRunStatus,
    allow_idle_root: bool,
    allow_direct_member: bool,
) -> Result<(), String> {
    use crate::coordination::agent_org_runs::AgentOrgRunStatus;

    match status {
        AgentOrgRunStatus::Running => Ok(()),
        AgentOrgRunStatus::Starting => Err(format!(
            "team_not_ready: Agent Org run {run_id} is still materializing"
        )),
        AgentOrgRunStatus::Paused if allow_direct_member => Ok(()),
        AgentOrgRunStatus::Paused => Err(format!(
            "team_paused: Agent Org run {run_id} cannot start a turn in this lifecycle slice"
        )),
        AgentOrgRunStatus::Idle if allow_idle_root || allow_direct_member => Ok(()),
        AgentOrgRunStatus::Idle => Err(format!(
            "team_idle: Agent Org run {run_id} accepts new turns only in its canonical Root session"
        )),
        AgentOrgRunStatus::Failed => Err(format!(
            "team_unavailable: Agent Org run {run_id} failed during materialization"
        )),
        AgentOrgRunStatus::Archived => Err(format!(
            "team_archived: Agent Org run {run_id} is read-only"
        )),
    }
}

async fn preflight_agent_org_turn_before_runtime(
    session_id: &str,
    explicit_run_id: Option<&str>,
    run_id_hint: Option<&str>,
    has_persisted_agent_org_identity: bool,
    is_direct_member: bool,
) -> Result<Option<String>, String> {
    if let (Some(explicit), Some(hint)) = (explicit_run_id, run_id_hint) {
        if explicit != hint {
            return Err(format!(
                "Agent Org turn intent run mismatch for session {session_id}: explicit run {explicit}, runtime run {hint}"
            ));
        }
    }

    let run_id = match explicit_run_id.or(run_id_hint) {
        Some(run_id) => Some(run_id.to_string()),
        None if has_persisted_agent_org_identity => {
            let session_id = session_id.to_string();
            tokio::task::spawn_blocking(move || {
                crate::coordination::agent_org_runs::AgentOrgRunStore::run_id_for_session_with_parent_walk(
                    &session_id,
                )
            })
            .await
            .map_err(|error| format!("Agent Org run lookup worker failed: {error}"))??
        }
        None => None,
    };

    let Some(run_id) = run_id else {
        return Ok(None);
    };

    crate::coordination::agent_org_runs::require_agent_org_enabled()?;
    let status_run_id = run_id.clone();
    let run = tokio::task::spawn_blocking(move || {
        crate::coordination::agent_org_runs::AgentOrgRunStore::load(&status_run_id)
    })
    .await
    .map_err(|error| format!("Agent Org status worker failed: {error}"))??
    .ok_or_else(|| format!("team_unavailable: Agent Org run {run_id} does not exist"))?;
    let allow_idle_root = run.root_session_id.as_deref() == Some(session_id);
    ensure_agent_org_turn_is_runnable(&run_id, run.status, allow_idle_root, is_direct_member)?;
    Ok(Some(run_id))
}

pub(super) fn should_divert_to_mid_turn_steering(
    source: TurnIntentBridgeSource,
    is_resume: bool,
    content: &str,
    images: Option<&[String]>,
    is_turn_processing: bool,
    has_agent_org_context: bool,
) -> bool {
    !has_agent_org_context
        && matches!(source, TurnIntentBridgeSource::UserSubmit)
        && !is_resume
        && !content.trim().is_empty()
        && images.map(|items| items.is_empty()).unwrap_or(true)
        && is_turn_processing
}

fn resume_requires_existing_agent_org_context(
    source: TurnIntentBridgeSource,
    wake_member_id: Option<&str>,
) -> bool {
    wake_member_id.is_none() && matches!(source, TurnIntentBridgeSource::Resume)
}

fn should_record_standalone_goal(
    source: TurnIntentBridgeSource,
    is_resume: bool,
    has_agent_org_context: bool,
) -> bool {
    matches!(
        source,
        TurnIntentBridgeSource::UserSubmit | TurnIntentBridgeSource::ForceSend
    ) && !is_resume
        && !has_agent_org_context
}

pub(super) fn terminal_intent_status_override(
    state: crate::session::DialogTurnState,
) -> Option<crate::foundation::session_bridge::TurnIntentBridgeStatus> {
    match state {
        crate::session::DialogTurnState::Cancelled => {
            Some(crate::foundation::session_bridge::TurnIntentBridgeStatus::Cancelled)
        }
        crate::session::DialogTurnState::Running
        | crate::session::DialogTurnState::Completed
        | crate::session::DialogTurnState::Failed => None,
    }
}

/// Implementation of agent_send_message.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn send_message_impl(
    state: &AgentAppState,
    session_id: String,
    content: String,
    display_text: Option<String>,
    overrides: IdentityOverrides,
    mode: Option<String>,
    images: Option<Vec<String>>,
    ide_context: Option<crate::session::IdeContext>,
    is_resume: bool,
    agent_org_direct_source_event_id: Option<String>,
    agent_org_group_root_source_event_id: Option<String>,
    allow_admitted_direct_recovery: bool,
    client_message_id: Option<String>,
    turn_intent_id: Option<String>,
    org_wake_run_id: Option<String>,
    org_wake_member_id: Option<String>,
    intent_org_run_id: Option<String>,
    source: TurnIntentBridgeSource,
) -> Result<AgentResponse, String> {
    if state.is_shutting_down() {
        return Err("app_shutdown_in_progress: refusing to enqueue a new Agent turn".into());
    }
    // Canonical user-intent id: callers that already mint one at the
    // submit boundary pass it through; legacy / internal callers that
    // don't (mobile remote, wake hook, plan-approval re-entry) get a
    // server-side fallback so the bridge slot is always non-empty.
    let effective_turn_intent_id =
        turn_intent_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let preadmitted_user_directed_work = if agent_org_direct_source_event_id.is_none()
        && agent_org_group_root_source_event_id.is_none()
        && intent_org_run_id.is_some()
    {
        let lookup_session_id = session_id.clone();
        let lookup_turn_intent_id = effective_turn_intent_id.clone();
        tokio::task::spawn_blocking(move || {
            agent_org_user_directed_work::status_by_turn(&lookup_session_id, &lookup_turn_intent_id)
        })
        .await
        .map_err(|error| format!("UserDirectedWork receipt lookup failed: {error}"))??
    } else {
        None
    };

    let default_mode = crate::session::AgentExecMode::Build.as_str();
    tracing::info!(
        "[agent_send_message] session={}, model={:?}, account={:?}, mode={:?}, images={}, turn_intent_id={}",
        session_id,
        overrides.model.as_deref().unwrap_or("<default>"),
        overrides.account_id.as_deref().unwrap_or("<default>"),
        mode.as_deref().unwrap_or(default_mode),
        images
            .as_ref()
            .map(|v| format!("{} image(s)", v.len()))
            .unwrap_or_else(|| "none".to_string()),
        effective_turn_intent_id,
    );

    // ── 1. Resolve session identity (unified — single code path) ─────────
    let identity = resolve_session_identity(state, &session_id, overrides).await?;

    let explicit_org_run_id = match (org_wake_run_id.as_deref(), intent_org_run_id.as_deref()) {
        (Some(wake_run_id), Some(intent_run_id)) if wake_run_id != intent_run_id => {
            return Err(format!(
                "Agent Org wake/intent run mismatch for session {session_id}: wake run {wake_run_id}, intent run {intent_run_id}"
            ));
        }
        (Some(run_id), _) | (None, Some(run_id)) => Some(run_id),
        (None, None) => None,
    };
    let is_direct_member = agent_org_direct_source_event_id.is_some();
    let is_group_root = agent_org_group_root_source_event_id.is_some();
    let is_user_directed_work = is_direct_member || preadmitted_user_directed_work.is_some();
    let is_exact_group_turn = is_user_directed_work || is_group_root;
    let has_preadmitted_user_directed_work = preadmitted_user_directed_work.is_some();
    if is_direct_member
        && (is_resume
            || content.trim().is_empty()
            || !matches!(source, TurnIntentBridgeSource::UserSubmit))
    {
        return Err(
            "user_directed_source_invalid: DirectMember requires a non-empty user_submit Turn"
                .to_string(),
        );
    }
    if is_group_root
        && (is_resume
            || content.trim().is_empty()
            || !matches!(source, TurnIntentBridgeSource::UserSubmit))
    {
        return Err(
            "group_root_source_invalid: GroupRoot requires a non-empty user_submit Turn"
                .to_string(),
        );
    }
    let direct_source_display_content = display_text
        .as_deref()
        .unwrap_or(content.as_str())
        .to_string();
    let preflight_org_run_id = preflight_agent_org_turn_before_runtime(
        &session_id,
        explicit_org_run_id,
        identity.agent_org_run_id_hint.as_deref(),
        identity.has_persisted_agent_org_identity,
        is_user_directed_work,
    )
    .await?;

    // Goal loop: a real user submission becomes (or replaces) the
    // session's standing goal and resets the continuation counter.
    // `Queue`-sourced messages (goal continuations, queued flushes) and
    // resumes never reset it — otherwise the loop would feed itself.
    if should_record_standalone_goal(source, is_resume, preflight_org_run_id.is_some()) {
        crate::session::goal_loop::on_user_message(&session_id, &content, display_text.as_deref());
    }

    let effective_model = identity.model;
    let effective_account_id = identity.account_id;
    let effective_workspace_root = identity.workspace_root;
    let effective_native_harness_type = identity.native_harness_type;

    // ── 2. Ensure session is initialized (lazy runtime creation) ─────────
    let launch_spec = crate::init::launch_spec::AgentLaunchSpec::from_session_sources(
        state,
        &session_id,
        effective_workspace_root.clone(),
        effective_account_id.clone(),
        Some(effective_model.clone()),
        effective_native_harness_type,
    )
    .await?;

    // A busy direct Member submission must first suspend the runtime that is
    // already executing TaskExecution. Re-entering generic initialization
    // here can decide that model/account/workspace drift requires a rebuild,
    // replace the lease before the durable handoff CAS, and strand the
    // receipt in `yield_requested`. Reuse only the exact live runtime for
    // this pre-admission window; idle direct and every ordinary SDE message
    // retain the normal initialization/revalidation path.
    let active_direct_runtime = if is_direct_member {
        match state.get_session(&session_id).await {
            Some(session) if session.runtime_turn_identity().await.is_some() => {
                session.get_runtime().await
            }
            _ => None,
        }
    } else {
        None
    };
    let runtime = match active_direct_runtime {
        Some(runtime) => {
            tracing::info!(
                event = "agent_org_user_directed_reuse_active_runtime",
                session_id = %session_id,
                "reusing the exact busy Member runtime until durable handoff"
            );
            runtime
        }
        None => crate::init::init_session(state, launch_spec).await?,
    };

    // Turn intent ownership is independent from wake behavior. Explicit
    // callers (initial Org launch, direct member message, wake) pass the run
    // id before the runtime necessarily exists; ordinary messages recover it
    // from the canonical runtime context. Never allow a retry to cross runs.
    let runtime_org_run_id = runtime
        .agent_org_context
        .as_ref()
        .map(|context| context.run_id.clone());
    let effective_intent_org_run_id = match (
        intent_org_run_id.as_deref(),
        runtime_org_run_id.as_deref(),
    ) {
        (Some(explicit), Some(runtime_id)) if explicit != runtime_id => {
            return Err(format!(
                "Agent Org turn intent run mismatch for session {session_id}: explicit run {explicit}, runtime run {runtime_id}"
            ));
        }
        (Some(_), _) => intent_org_run_id,
        (None, Some(_)) => runtime_org_run_id,
        (None, None) => preflight_org_run_id,
    };

    // Every Agent Org Turn receives persisted typed authority before a base
    // intent or scheduler item can exist. Background Member wakes derive one
    // TaskExecution binding from canonical unread formal work; direct Member
    // submissions use the source/receipt transaction below instead.
    if let Some(run_id) = effective_intent_org_run_id.as_deref() {
        let admission_run_id = run_id.to_string();
        let admission_session_id = session_id.clone();
        let admission_turn_intent_id = effective_turn_intent_id.clone();
        let admission_client_message_id = client_message_id.clone();
        let admission_wake_member_id = org_wake_member_id.clone();
        if !is_direct_member {
            tokio::task::spawn_blocking(move || match admission_wake_member_id {
            Some(member_id) => crate::coordination::agent_org_turn_contexts::accept_wake(
                &admission_run_id,
                &admission_session_id,
                &admission_turn_intent_id,
                admission_client_message_id,
                &member_id,
            ),
            None if has_preadmitted_user_directed_work => {
                crate::coordination::agent_org_turn_contexts::require_existing_context(
                    &admission_run_id,
                    &admission_session_id,
                    &admission_turn_intent_id,
                )
            }
            None
                if resume_requires_existing_agent_org_context(source, None) =>
            {
                crate::coordination::agent_org_turn_contexts::require_existing_context(
                    &admission_run_id,
                    &admission_session_id,
                    &admission_turn_intent_id,
                )
            }
            None if agent_org_group_root_source_event_id.is_some() => {
                let admission = crate::coordination::agent_org_turn_contexts::AgentOrgTurnAdmission::group_root(
                    admission_run_id,
                    admission_session_id,
                    admission_turn_intent_id,
                    admission_client_message_id,
                    agent_org_group_root_source_event_id
                        .expect("GroupRoot match proved the source exists"),
                );
                crate::coordination::agent_org_turn_contexts::accept(&admission)
            }
            None => {
                let admission = crate::coordination::agent_org_turn_contexts::AgentOrgTurnAdmission::coordinator(
                    admission_run_id,
                    admission_session_id,
                    admission_turn_intent_id,
                    admission_client_message_id,
                    source,
                );
                crate::coordination::agent_org_turn_contexts::accept(&admission)
            }
        })
            .await
            .map_err(|error| format!("Agent Org Turn admission worker failed: {error}"))??;
        }
    } else if is_direct_member || is_group_root {
        return Err(
            "agent_org_source_invalid: source was supplied for a non-Agent-Org Session".to_string(),
        );
    }

    // Wingman resume: reopen the bottom bar. On fresh start the frontend
    // sends `wingman_start` which opens the bar, but after app restart
    // the frontend doesn't re-send that command. Best-effort — a missing
    // bar doesn't block the session.
    if crate::definitions::prefix_lookup::is_wingman_session_id(&session_id) {
        if let Some(ref app_h) = state.app_handle {
            crate::session::wingman::open_wingman_bar(app_h, &session_id, "Active", None);
        }
    }

    // ── 3. Snapshot session resources (single lookup) ─────────────────────
    //
    // After `ensure_session_initialized` the session is guaranteed to exist
    // in memory, so we look it up once and extract everything we need.
    // `session_handle` stays alive for the enqueue step at the end;
    // `agent_session_arc` (clone) is moved into the async closure.
    let session_handle = state
        .get_session(&session_id)
        .await
        .ok_or_else(|| format!("Session not found after init: {}", session_id))?;

    session_handle.refresh_last_active().await;

    let cancel_flag = Arc::clone(&session_handle.cancel_flag);
    let session_for_closure = Arc::clone(&session_handle);
    let load_workspace_resources = runtime.resolved.load_workspace_resources;

    let mut direct_runtime_admission = None;
    let direct_user_directed_work: Option<EnqueueUserDirectedWorkResult> = if let Some(
        source_event_id,
    ) =
        agent_org_direct_source_event_id.as_ref()
    {
        let run_id = effective_intent_org_run_id.clone().ok_or_else(|| {
            "user_directed_target_invalid: canonical Agent Org run is missing".to_string()
        })?;
        let lookup_session_id = session_id.clone();
        let member_id = tokio::task::spawn_blocking(move || {
                crate::session::persistence::get_session(&lookup_session_id)
                    .map_err(|error| error.to_string())?
                    .and_then(|record| record.org_member_id)
                    .ok_or_else(|| {
                        format!(
                            "user_directed_target_invalid: Session {lookup_session_id} has no canonical Member"
                        )
                    })
            })
            .await
            .map_err(|error| format!("Agent Org Member lookup worker failed: {error}"))??;
        let runtime_admission = session_handle
            .reserve_runtime_admission(&runtime, &effective_turn_intent_id)
            .await?;
        let enqueue = EnqueueUserDirectedWorkParams {
            org_run_id: run_id,
            session_id: session_id.clone(),
            member_id,
            turn_intent_id: effective_turn_intent_id.clone(),
            client_message_id: client_message_id.clone(),
            source_event_id: source_event_id.clone(),
            dispatch_content: content.clone(),
            source_display_content: direct_source_display_content.clone(),
            source_images: images.clone().filter(|images| !images.is_empty()),
            runtime_reservation_id: runtime_admission.reservation_id.clone(),
            runtime_lease_id: runtime_admission.runtime_lease_id.clone(),
            queue_cap: DEFAULT_USER_DIRECTED_QUEUE_CAP,
        };
        let admitted = match tokio::task::spawn_blocking(move || {
            AgentMemberInterventionStore::enqueue_user_directed_work(enqueue)
        })
        .await
        {
            Ok(admitted) => admitted,
            Err(error) => {
                session_handle
                    .release_runtime_admission(&runtime_admission)
                    .await;
                return Err(format!("UserDirectedWork admission worker failed: {error}"));
            }
        };
        match admitted {
            Ok(admitted) => {
                direct_runtime_admission = Some(runtime_admission);
                Some(admitted)
            }
            Err(error) => {
                session_handle
                    .release_runtime_admission(&runtime_admission)
                    .await;
                return Err(error);
            }
        }
    } else {
        None
    };

    if let Some(admission) = direct_user_directed_work.as_ref() {
        tracing::info!(
            event = "agent_org_user_directed_admitted",
            org_run_id = %admission.context.org_run_id,
            session_id = %admission.context.session_id,
            turn_intent_id = %admission.context.turn_intent_id,
            member_dispatch_sequence = ?admission.context.member_dispatch_sequence,
            intervention_receipt_id = %admission.intervention.intervention_receipt_id,
            duplicate = admission.duplicate,
            turn_status = %admission.turn_status,
            runtime_admission_status = %admission.runtime_admission_status,
            "accepted durable direct Member Turn"
        );
        if admission.should_request_yield {
            org_tasks::schedule_user_directed_yield_timeout_observer(
                admission.intervention.intervention_receipt_id.clone(),
            );
            if let Some(original_turn_intent_id) =
                admission.intervention.original_turn_intent_id.as_deref()
            {
                match session_handle.runtime_turn_identity().await {
                    Some(identity)
                        if identity.turn_intent_id.as_deref() == Some(original_turn_intent_id) =>
                    {
                        let receipt_id = admission.intervention.intervention_receipt_id.clone();
                        let original_turn_intent_id = original_turn_intent_id.to_string();
                        let runtime_lease_id = identity.runtime_lease_id.clone();
                        let dialog_turn_generation = identity.dialog_turn_generation.clone();
                        let bound = tokio::task::spawn_blocking(move || {
                            AgentMemberInterventionStore::bind_runtime_and_request_yield(
                                &receipt_id,
                                &original_turn_intent_id,
                                &runtime_lease_id,
                                &dialog_turn_generation,
                            )
                        })
                        .await
                        .map_err(|error| {
                            format!("UserDirectedWork handoff worker failed: {error}")
                        })??;
                        if bound {
                            session_handle
                                .cancel_active_turn(
                                    crate::state::control_flow::CancelReason::UserIntervention,
                                )
                                .await;
                        }
                    }
                    None => {
                        // The durable formal intent can become Running just
                        // before begin_turn installs its exact in-memory
                        // identity. Preserve a pre-turn cancellation marker;
                        // the formal finalizer binds and releases the exact
                        // lease once the identity exists.
                        session_handle
                            .cancel_active_turn(
                                crate::state::control_flow::CancelReason::UserIntervention,
                            )
                            .await;
                    }
                    Some(_) => {}
                }
            }
        }
    }

    if let Some(status) = preadmitted_user_directed_work {
        if status != UserDirectedDeliveryStatus::Pending {
            return Ok(AgentResponse {
                content: serde_json::json!({
                    "queued": status == UserDirectedDeliveryStatus::Started,
                    "duplicate": true,
                    "turnStatus": status.as_str(),
                })
                .to_string(),
                session_id,
                model: effective_model,
            });
        }
    }

    if let Some(admission) = direct_user_directed_work.as_ref() {
        if !should_dispatch_admitted_direct(
            admission.duplicate,
            &admission.turn_status,
            allow_admitted_direct_recovery,
        ) {
            if let Some(reservation) = direct_runtime_admission.as_ref() {
                session_handle.release_runtime_admission(reservation).await;
            }
            return Ok(AgentResponse {
                content: serde_json::json!({
                    "queued": matches!(admission.turn_status.as_str(), "queued" | "running"),
                    "duplicate": true,
                    "turnStatus": admission.turn_status,
                    "runtimeAdmissionState": admission.runtime_admission_status,
                    "interventionReceiptId": admission.intervention.intervention_receipt_id,
                })
                .to_string(),
                session_id,
                model: effective_model,
            });
        }
    }

    let app_handle = state.app_handle.clone();
    let app_state_for_closure = state.clone();

    // ── 3b. Mid-turn steering divert ─────────────────────────────────────
    //
    // A plain-text user message that arrives while a turn is RUNNING is
    // injected into that turn (drained by the turn loop before the next
    // LLM iteration) instead of waiting behind it as its own turn — the
    // model can change course immediately. Agent Org user turns are excluded:
    // Root Coordinator follow-ups must retain their own durable FIFO turn
    // boundary, while direct Member messages use the intervention path below.
    // Also excluded: force-sends (they interrupt via their own boundary
    // semantics), resumes, queue-sourced continuations, image messages, and
    // empty content. The Stop boundary clears the buffer, matching queued-
    // message discard semantics.
    // `is_turn_processing`, not `is_processing`: only a running turn drains
    // the steering queue. A maintenance job (manual compaction) occupies the
    // worker without a turn loop, so a message diverted here during one would
    // wait forever.
    if should_divert_to_mid_turn_steering(
        source,
        is_resume,
        &content,
        images.as_deref(),
        session_handle.scheduler.is_turn_processing(),
        effective_intent_org_run_id.is_some(),
    ) && direct_user_directed_work.is_none()
    {
        // Steering mutates an already-running member turn, so intervention is
        // part of accepting the control action. If the durable takeover row
        // cannot be written, do not inject a message that Wake may race.
        if effective_intent_org_run_id.is_none() {
            crate::foundation::session_bridge::upsert_turn_intent(
                &session_id,
                &effective_turn_intent_id,
                client_message_id.as_deref(),
                None,
                source,
                crate::foundation::session_bridge::TurnIntentBridgeStatus::Queued,
            );
        }
        session_handle
            .steering_queue
            .lock()
            .await
            .push(crate::turn_executor::SteeringInjection {
                content: content.clone(),
                turn_intent_id: effective_turn_intent_id.clone(),
            });

        // Race closure: the turn may have ended between the is_processing
        // check and the push. If it's idle now, reclaim the injection (when
        // still unconsumed) and fall through to a normal enqueue.
        let reclaimed = if !session_handle.scheduler.is_turn_processing() {
            let mut steering = session_handle.steering_queue.lock().await;
            let before = steering.len();
            steering.retain(|inj| inj.turn_intent_id != effective_turn_intent_id);
            steering.len() != before
        } else {
            false
        };

        if !reclaimed {
            tracing::info!(
                "[agent_send_message] Steering message into active turn for session {} (intent={})",
                session_id,
                effective_turn_intent_id
            );
            return Ok(AgentResponse {
                content: serde_json::json!({
                    "queued": true,
                    "steered": true,
                    "messageId": effective_turn_intent_id,
                    "queuePosition": 0,
                    "duplicate": false,
                })
                .to_string(),
                session_id,
                model: effective_model,
            });
        }
    }

    // ── 4. Persist identity and user_input (single DB write) ─────────────
    //
    // Also closes the override-account persistence gap: callers that switch
    // the account purely on the message wire (plan-approval Build kick-off,
    // composer-sent account) used to only rebuild the runtime — the DB row
    // kept the old account, so an app restart silently reverted the switch.
    // Syncing the resolved account here keeps memory and DB in one truth.
    {
        let sid = session_id.clone();
        let input_preview: String = crate::utils::safe_truncate_chars_to_string(&content, 100);
        let model_clone = effective_model.clone();
        let account_clone = effective_account_id.clone();
        let prev_account = tokio::task::spawn_blocking(move || {
            let mut prev_account: Option<Option<String>> = None;
            if let Ok(Some(mut db_session)) = session_persistence::get_session(&sid) {
                if db_session.user_input.is_none() {
                    db_session.user_input = Some(input_preview);
                    db_session.model = Some(model_clone);
                }
                if account_clone.is_some() && db_session.account_id != account_clone {
                    prev_account = Some(db_session.account_id.take());
                    db_session.account_id = account_clone;
                }
                if let Err(err) = session_persistence::upsert_session(&db_session) {
                    tracing::warn!("[session] Failed to upsert session {sid}: {err}");
                }
            } else {
                tracing::warn!("[session] DB row missing for {sid}, cannot persist status");
            }
            prev_account
        })
        .await
        .map_err(|err| err.to_string())?;
        // `Some(prev)` only when the account actually flipped above.
        if let (Some(prev), Some(to_account)) = (prev_account, effective_account_id.as_deref()) {
            crate::lifecycle::emit_session_account_switched(
                state.app_handle.as_ref(),
                &session_id,
                prev.as_deref(),
                to_account,
                Some(&effective_model),
            );
        }
    }

    // ── 4b. Project root WorkItem bootstrap (orgtrack/v1 §7.2) ──────────
    //
    // The first accepted non-empty submission of a Project session with
    // no active WorkItem creates and links its root. Resumes replay an
    // already-accepted submission, so they never bootstrap.
    if !is_resume {
        super::project_bootstrap::ensure_project_root_work_item(&session_id, &content).await?;
        if let Some(run) = super::project_bootstrap::enqueue_project_turn_if_needed(
            &session_id,
            &content,
            display_text.as_deref(),
            &effective_turn_intent_id,
            client_message_id.as_deref(),
            source,
        )
        .await?
        {
            tracing::info!(
                session_id = %session_id,
                run_id = %run.id,
                "queued Project turn through durable WorkItem dispatcher"
            );
            return Ok(AgentResponse {
                content: serde_json::json!({
                    "queued": true,
                    "durableRunId": run.id,
                    "messageId": client_message_id
                        .as_deref()
                        .unwrap_or(&effective_turn_intent_id),
                    "queuePosition": 0,
                    "duplicate": false,
                })
                .to_string(),
                session_id,
                model: effective_model,
            });
        }
    }

    // ── 5. Build the processing closure ──────────────────────────────────
    let sid_for_closure = session_id.clone();
    let content_for_closure = content.clone();
    let display_text_for_closure = display_text;
    let workspace_root_for_closure = effective_workspace_root.clone();
    let turn_intent_id_for_closure = effective_turn_intent_id.clone();
    let direct_user_directed_work_for_closure = direct_user_directed_work.clone();
    let direct_runtime_admission_for_closure = direct_runtime_admission.clone();
    let is_user_directed_work_for_closure = is_user_directed_work;
    let intent_org_run_id_for_closure = effective_intent_org_run_id.clone();
    // Resolve durable mode-control rows from exactly the bounded inbox batch
    // this background wake will drain. A control row in a later batch must
    // not change the mode of earlier work; rows become one-shot only when the
    // successful turn commits their read watermark.
    let inbox_control_mode = if let Some(run_id) = org_wake_run_id.as_deref() {
        let mode_session_id = session_id.clone();
        let mode_run_id = run_id.to_string();
        let mode_turn_intent_id = effective_turn_intent_id.clone();
        tokio::task::spawn_blocking(move || {
            resolve_agent_org_wake_mode(&mode_session_id, &mode_run_id, &mode_turn_intent_id)
        })
        .await
        .map_err(|error| format!("Agent Org pre-turn mode resolver failed: {error}"))??
    } else {
        None
    };
    // A direct human message owns this turn's mode. Do not consume the legacy
    // in-memory background override during intervention; durable unread rows
    // remain the source of truth for the next background wake.
    let coordinator_mode_override = org_wake_run_id
        .as_ref()
        .and_then(|_| session_handle.requested_exec_mode_cache.take(&session_id));
    let agent_mode = match inbox_control_mode.or(coordinator_mode_override) {
        Some(forced) => forced,
        None => resolve_agent_mode(mode.as_deref())?,
    };

    // Track the Plan-mode pre-mode snapshot.
    {
        let session = &session_handle;
        let current_mode = agent_mode;
        if matches!(current_mode, crate::session::AgentExecMode::Plan) {
            if session.pre_plan_mode_cache.get(&session_id).is_none() {
                let previous = restore_mode_before_plan_entry(
                    session.last_non_plan_mode_cache.get(&session_id),
                );
                session.pre_plan_mode_cache.set(&session_id, previous);
            }
        } else {
            session
                .last_non_plan_mode_cache
                .set(&session_id, current_mode);
        }
    }

    let execute: crate::session::scheduler::ExecuteFn = Box::new(move || {
        let sid = sid_for_closure;
        let content = content_for_closure;
        let display_text = display_text_for_closure;
        let workspace_root = workspace_root_for_closure;
        let session = session_for_closure;
        let turn_intent_id = turn_intent_id_for_closure;
        let direct_user_directed_work = direct_user_directed_work_for_closure;
        let direct_runtime_admission = direct_runtime_admission_for_closure;
        let is_user_directed_work = is_user_directed_work_for_closure;
        let is_exact_group_turn = is_exact_group_turn;
        let org_wake_run_id = org_wake_run_id;
        let intent_org_run_id = intent_org_run_id_for_closure;
        let app_state = app_state_for_closure;

        Box::pin(async move {
            if is_exact_group_turn && session.scheduler.turn_is_invalidated(&turn_intent_id) {
                if let Some(reservation) = direct_runtime_admission.as_ref() {
                    session.release_runtime_admission(reservation).await;
                }
                if is_user_directed_work {
                    let _ = agent_org_user_directed_work::mark_turn_terminal(
                        &sid,
                        &turn_intent_id,
                        UserDirectedDeliveryStatus::Cancelled,
                        Some("cancelled_before_runtime_commit"),
                    );
                }
                return Err(format!(
                    "{USER_DIRECTED_CANCELLED_ERROR_PREFIX} exact Turn was stopped before start"
                ));
            }
            // Clear a stale pre-turn cancel signal before the durable
            // Agent Org gate. This must happen before that gate: deletion may
            // establish its cancelled fence immediately after the DB claim
            // and then set the cancel flag while `active_turn` is not yet
            // registered. Clearing later would erase that deletion signal.
            //
            // Messages that reach this closure have already passed the
            // scheduler generation check, so queued work invalidated by Stop
            // or hierarchy deletion is discarded before this callback runs.
            cancel_flag.store(false, std::sync::atomic::Ordering::SeqCst);

            // Runtime reservation is the first half of direct admission. It
            // must still name the exact installed Provider before the durable
            // lifecycle may cross from prepared to committed/running.
            if let Some(reservation) = direct_runtime_admission.as_ref() {
                if !session
                    .runtime_admission_is_current(reservation, &runtime)
                    .await
                {
                    session.release_runtime_admission(reservation).await;
                    let _ = agent_org_user_directed_work::mark_turn_terminal(
                        &sid,
                        &turn_intent_id,
                        UserDirectedDeliveryStatus::Failed,
                        Some("runtime_reservation_lost_before_commit"),
                    );
                    return Err(
                        "agent_org_runtime_admission_stale: prepared runtime reservation was lost"
                            .to_string(),
                    );
                }
            }

            // Queued and coalesced messages are not running sessions. Promote
            // the DB state only when the scheduler actually begins execution.
            // Both Agent Org wakes and direct turns require a Running Team.
            // This execute-time check closes the race after submit preflight:
            // a queued turn becomes a no-op if lifecycle advances first.
            let status_sid = sid.clone();
            let status_wake_run_id = org_wake_run_id.clone();
            let status_intent_run_id = intent_org_run_id.clone();
            let status_turn_intent_id = turn_intent_id.clone();
            let is_user_directed_work_turn = is_user_directed_work;
            match tokio::task::spawn_blocking(move || {
                database::db::with_sessions_writer(|| -> Result<bool, String> {
                    let mut conn = database::db::get_connection().map_err(|err| err.to_string())?;
                    let tx = conn
                        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
                        .map_err(|err| err.to_string())?;
                    let promotion = promote_turn_to_running_in_tx(
                        &tx,
                        &status_sid,
                        &status_turn_intent_id,
                        status_wake_run_id.as_deref(),
                        status_intent_run_id.as_deref(),
                        is_user_directed_work_turn,
                    )?;
                    tx.commit().map_err(|err| err.to_string())?;
                    Ok(promotion)
                })
            })
            .await
            {
                Ok(Ok(true)) => {}
                Ok(Ok(false)) if is_user_directed_work_turn => {
                    if let Some(reservation) = direct_runtime_admission.as_ref() {
                        session.release_runtime_admission(reservation).await;
                    }
                    let _ = agent_org_user_directed_work::mark_turn_terminal(
                        &sid,
                        &turn_intent_id,
                        UserDirectedDeliveryStatus::Failed,
                        Some("runtime_commit_rejected"),
                    );
                    return Err(format!(
                        "{USER_DIRECTED_WAITING_ERROR_PREFIX} intervention handoff is not released"
                    ));
                }
                Ok(Ok(false)) => return Ok(String::new()),
                Ok(Err(err)) => {
                    if let Some(reservation) = direct_runtime_admission.as_ref() {
                        session.release_runtime_admission(reservation).await;
                    }
                    let _ = agent_org_user_directed_work::mark_turn_terminal(
                        &sid,
                        &turn_intent_id,
                        UserDirectedDeliveryStatus::Failed,
                        Some("runtime_commit_failed"),
                    );
                    return Err(format!("failed to persist running status: {err}"));
                }
                Err(err) => {
                    if let Some(reservation) = direct_runtime_admission.as_ref() {
                        session.release_runtime_admission(reservation).await;
                    }
                    let _ = agent_org_user_directed_work::mark_turn_terminal(
                        &sid,
                        &turn_intent_id,
                        UserDirectedDeliveryStatus::Failed,
                        Some("runtime_commit_worker_failed"),
                    );
                    return Err(format!("running-status task failed: {err}"));
                }
            }
            if is_exact_group_turn && session.scheduler.turn_is_invalidated(&turn_intent_id) {
                if is_user_directed_work {
                    let _ = agent_org_user_directed_work::mark_turn_terminal(
                        &sid,
                        &turn_intent_id,
                        UserDirectedDeliveryStatus::Cancelled,
                        Some("user_stop"),
                    );
                    if direct_runtime_admission.is_some() {
                        let _ = AgentMemberInterventionStore::reject_committed_runtime_admission(
                            &sid,
                            &turn_intent_id,
                            "cancelled_before_provider_start",
                        );
                    }
                } else {
                    crate::foundation::session_bridge::update_turn_intent_status(
                        &sid,
                        &turn_intent_id,
                        crate::foundation::session_bridge::TurnIntentBridgeStatus::Cancelled,
                    );
                }
                if let Some(reservation) = direct_runtime_admission.as_ref() {
                    session.release_runtime_admission(reservation).await;
                }
                return Err(format!(
                    "{USER_DIRECTED_CANCELLED_ERROR_PREFIX} exact Group Turn was stopped before Provider execution"
                ));
            }
            if let Some(accepted) = direct_user_directed_work.as_ref() {
                crate::coordination::agent_org_run_events::notify_agent_org_run_changed(
                    &accepted.context.org_run_id,
                );
                if let Err(error) = session
                    .attach_warm_runtime_if_empty(Arc::clone(&runtime))
                    .await
                {
                    if let Some(reservation) = direct_runtime_admission.as_ref() {
                        session.release_runtime_admission(reservation).await;
                    }
                    let _ = AgentMemberInterventionStore::reject_committed_runtime_admission(
                        &sid,
                        &turn_intent_id,
                        "runtime_attach_failed",
                    );
                    return Err(error);
                }
            }

            let turn_id = session
                .begin_turn_with_intent(content.clone(), Some(turn_intent_id.clone()))
                .await;
            if let Some(reservation) = direct_runtime_admission.as_ref() {
                session.release_runtime_admission(reservation).await;
            }
            if is_exact_group_turn && session.scheduler.turn_is_invalidated(&turn_intent_id) {
                session
                    .cancel_active_turn(crate::state::control_flow::CancelReason::UserDirectedStop)
                    .await;
            }
            // Keep the exact identity installed at the Turn boundary. A
            // cancelled Agent Org Turn can publish terminal state before the
            // handoff finalizer runs; consulting only the live slot there can
            // therefore lose the lease/generation needed for the CAS release.
            let turn_identity = session.runtime_turn_identity().await;

            let input = crate::session::TurnInput {
                content: content.clone(),
                display_text,
                agent_mode: Some(agent_mode),
                images,
                ide_context,
                is_resume,
                channel: None,
                chat_id: None,
                turn_id: Some(turn_id.clone()),
                turn_intent_id: turn_intent_id.clone(),
            };

            let response =
                crate::session::process_message(Arc::clone(&session), input, app_handle.clone())
                    .await;

            let final_turn_state = if cancel_flag.load(std::sync::atomic::Ordering::SeqCst) {
                crate::session::DialogTurnState::Cancelled
            } else if response.is_ok() {
                crate::session::DialogTurnState::Completed
            } else {
                crate::session::DialogTurnState::Failed
            };

            let stats = response
                .as_ref()
                .ok()
                .map(|r| crate::session::TurnStats {
                    prompt_tokens: r.prompt_tokens,
                    completion_tokens: r.completion_tokens,
                    total_tokens: r.total_tokens,
                    context_tokens: 0,
                    tool_calls_count: r.tool_calls_count,
                    duration: None,
                })
                .unwrap_or_default();
            org_tasks::settle_pause_handoff_after_turn(
                &app_state,
                &session,
                intent_org_run_id.as_deref(),
                &turn_intent_id,
            )
            .await;
            org_tasks::settle_user_directed_handoff_after_turn(
                &session,
                &turn_intent_id,
                turn_identity.as_ref(),
            )
            .await;
            session.end_turn(final_turn_state, stats).await;

            // The turn processor can return Ok with an empty response after a
            // user stop. Persist the authoritative cancelled terminal before
            // handing control back to the scheduler; its generic Ok =>
            // completed write is then rejected by the intent state machine.
            if let Some(status) = terminal_intent_status_override(final_turn_state) {
                crate::foundation::session_bridge::update_turn_intent_status(
                    &sid,
                    &turn_intent_id,
                    status,
                );
            }

            // A durable WorkItemRun owns exactly this turn, not the whole
            // Session. Persist its terminal state before lifecycle fan-out so
            // app exit cannot lose finality and a later turn on the same
            // Session cannot be mistaken for this Run.
            if turn_intent_id.starts_with("wir_") {
                let run_id = turn_intent_id.clone();
                let run_session_id = sid.clone();
                let outcome = match final_turn_state {
                    crate::session::DialogTurnState::Cancelled => {
                        project_management::work_run_service::WorkItemRunTerminalOutcome::Cancelled
                    }
                    crate::session::DialogTurnState::Failed => {
                        project_management::work_run_service::WorkItemRunTerminalOutcome::Failed
                    }
                    crate::session::DialogTurnState::Running
                    | crate::session::DialogTurnState::Completed => {
                        project_management::work_run_service::WorkItemRunTerminalOutcome::Succeeded
                    }
                };
                let usage = response
                    .as_ref()
                    .ok()
                    .map(
                        |result| project_management::projects::types::WorkItemRunUsage {
                            input_tokens: u64::try_from(result.prompt_tokens).unwrap_or(0),
                            output_tokens: u64::try_from(result.completion_tokens).unwrap_or(0),
                            total_tokens: u64::try_from(result.total_tokens).unwrap_or(0),
                            ..Default::default()
                        },
                    )
                    .unwrap_or_default();
                let terminal_error = response.as_ref().err().cloned();
                match tokio::task::spawn_blocking(move || {
                    project_management::work_run_service::record_run_terminal(
                        &run_id,
                        Some(&run_session_id),
                        outcome,
                        usage,
                        terminal_error.as_deref(),
                    )
                })
                .await
                {
                    Ok(Ok(_)) => {}
                    Ok(Err(err)) => tracing::error!(
                        session_id = %sid,
                        turn_intent_id = %turn_intent_id,
                        error = %err,
                        "failed to persist Work Item Run terminal"
                    ),
                    Err(err) => tracing::error!(
                        session_id = %sid,
                        turn_intent_id = %turn_intent_id,
                        error = %err,
                        "Work Item Run terminal task failed"
                    ),
                }
            }

            let terminal_turn =
                response
                    .as_ref()
                    .ok()
                    .map(|r| crate::lifecycle::TerminalTurnSignal {
                        turn_id: r.turn_id.clone(),
                        turn_intent_id: Some(turn_intent_id.clone()),
                        status: match final_turn_state {
                            crate::session::DialogTurnState::Cancelled => {
                                crate::lifecycle::TurnTerminalStatus::Cancelled
                            }
                            crate::session::DialogTurnState::Failed => {
                                crate::lifecycle::TurnTerminalStatus::Failed
                            }
                            crate::session::DialogTurnState::Running
                            | crate::session::DialogTurnState::Completed => {
                                crate::lifecycle::TurnTerminalStatus::Completed
                            }
                        },
                        completed_at: chrono::Utc::now()
                            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                    });

            let content_result = response.map(|r| r.content);

            // Persist the UserDirectedWork terminal before the ordinary
            // session finalizer emits `session-status-changed`. The frontend
            // uses that existing terminal event as its local, websocket-free
            // Run View invalidation, so every read triggered by the event must
            // already observe the direct FIFO slot as terminal. Keep the
            // result until after session finalization so a receipt write error
            // cannot strand the Session itself in Running.
            let direct_terminal_result = if is_user_directed_work {
                let terminal_session_id = sid.clone();
                let terminal_turn_intent_id = turn_intent_id.clone();
                let terminal_status = match final_turn_state {
                    crate::session::DialogTurnState::Cancelled => "cancelled",
                    crate::session::DialogTurnState::Failed => "failed",
                    crate::session::DialogTurnState::Running
                    | crate::session::DialogTurnState::Completed => "completed",
                };
                let terminal_error = content_result.as_ref().err().map(String::as_str);
                tokio::task::block_in_place(|| {
                    agent_org_user_directed_work::mark_turn_terminal(
                        &terminal_session_id,
                        &terminal_turn_intent_id,
                        match terminal_status {
                            "cancelled" => UserDirectedDeliveryStatus::Cancelled,
                            "failed" => UserDirectedDeliveryStatus::Failed,
                            _ => UserDirectedDeliveryStatus::Completed,
                        },
                        terminal_error,
                    )
                })
            } else {
                Ok(false)
            };

            crate::lifecycle::finalize_session(
                &sid,
                &content_result,
                app_handle.as_ref(),
                Some(workspace_root.as_path()),
                load_workspace_resources,
                terminal_turn,
            )
            .await;
            let user_directed_changed = direct_terminal_result?;
            if intent_org_run_id.is_some() || org_wake_run_id.is_some() {
                let completed_session_id = sid.clone();
                let completed_turn_intent_id = turn_intent_id.clone();
                match tokio::task::spawn_blocking(move || {
                    agent_org_user_directed_work::next_pending_after_terminal(
                        &completed_session_id,
                        &completed_turn_intent_id,
                    )
                })
                .await
                {
                    Ok(Ok(Some(next))) => {
                        let wake = crate::tools::impls::orchestration::org_send_message::UserDirectedWake {
                            org_run_id: next.org_run_id,
                            recipient_member_id: next.recipient_member_id,
                            recipient_session_id: next.recipient_session_id,
                            turn_intent_id: next.turn_intent_id,
                            content: next.content,
                            display_text: next.display_text,
                            images: next.images,
                        };
                        if let Some(app_handle) = app_state.app_handle.clone() {
                            use crate::core::tools::impls::orchestration::inbox_wake::AppHandleInboxWakeHook;
                            use crate::tools::impls::orchestration::org_send_message::InboxWakeHook;
                            AppHandleInboxWakeHook::new(app_handle).wake_user_directed_member(wake);
                        } else {
                            tracing::warn!(
                                session_id = %sid,
                                turn_intent_id = %turn_intent_id,
                                "next durable UDW kick has no AppHandle and is deferred to startup recovery"
                            );
                        }
                    }
                    Ok(Ok(None)) => {}
                    Ok(Err(error)) => tracing::warn!(
                        session_id = %sid,
                        turn_intent_id = %turn_intent_id,
                        error = %error,
                        "failed to resolve next durable UDW FIFO item"
                    ),
                    Err(error) => tracing::warn!(
                        session_id = %sid,
                        turn_intent_id = %turn_intent_id,
                        error = %error,
                        "next durable UDW lookup task failed"
                    ),
                }
            }
            if user_directed_changed {
                let completed_session_id = sid.clone();
                let completed_turn_intent_id = turn_intent_id.clone();
                let owner = tokio::task::spawn_blocking(move || {
                    agent_org_user_directed_work::dispatch_owner_for_turn(
                        &completed_session_id,
                        &completed_turn_intent_id,
                    )
                })
                .await;
                match owner {
                    Ok(Ok(Some((run_id, member_id)))) => {
                        if let Some(app_handle) = app_state.app_handle.clone() {
                            use crate::core::tools::impls::orchestration::inbox_wake::AppHandleInboxWakeHook;
                            use crate::tools::impls::orchestration::org_send_message::InboxWakeHook;
                            AppHandleInboxWakeHook::new(app_handle)
                                .wake_member(&member_id, &run_id);
                        }
                    }
                    Ok(Ok(None)) => {}
                    Ok(Err(error)) => tracing::warn!(
                        session_id = %sid,
                        turn_intent_id = %turn_intent_id,
                        error = %error,
                        "failed to resolve completed UDW dispatch owner"
                    ),
                    Err(error) => tracing::warn!(
                        session_id = %sid,
                        turn_intent_id = %turn_intent_id,
                        error = %error,
                        "completed UDW owner lookup task failed"
                    ),
                }
            }

            cancel_flag.store(false, std::sync::atomic::Ordering::SeqCst);

            content_result
        })
    });

    // ── 6. Enqueue and return immediately ────────────────────────────────
    let scheduler_client_message_id = if is_user_directed_work {
        Some(effective_turn_intent_id.clone())
    } else {
        client_message_id
    };
    let intervention_receipt_id = direct_user_directed_work
        .as_ref()
        .map(|accepted| accepted.intervention.intervention_receipt_id.clone());
    let msg = crate::session::ScheduledMessage {
        kind: crate::session::ScheduledKind::Turn,
        message_id: uuid::Uuid::new_v4().to_string(),
        generation: 0,
        client_message_id: scheduler_client_message_id,
        turn_intent_id: effective_turn_intent_id.clone(),
        org_run_id: effective_intent_org_run_id.clone(),
        content,
        execute,
    };

    // Lifecycle: record the intent as `queued` before handing the scheduler
    // ownership of the message. The scheduler worker promotes it to
    // `running` / terminal as the turn executes; `invalidate_pending`
    // marks it `stale` if rewound before it ran. See `session_turn_intents`
    // for the state machine.
    if effective_intent_org_run_id.is_none() {
        crate::foundation::session_bridge::upsert_turn_intent(
            &session_id,
            &effective_turn_intent_id,
            msg.client_message_id.as_deref(),
            None,
            source,
            crate::foundation::session_bridge::TurnIntentBridgeStatus::Queued,
        );
    }

    let enqueue_result = match session_handle.scheduler.enqueue(msg).await {
        Ok(result) => result,
        Err(error) => {
            if let Some(reservation) = direct_runtime_admission.as_ref() {
                session_handle.release_runtime_admission(reservation).await;
            }
            if is_user_directed_work {
                let failed_session_id = session_id.clone();
                let failed_turn_intent_id = effective_turn_intent_id.clone();
                let enqueue_failure = error.clone();
                tokio::task::spawn_blocking(move || {
                    if has_preadmitted_user_directed_work {
                        Ok(false)
                    } else if allow_admitted_direct_recovery {
                        AgentMemberInterventionStore::requeue_direct_after_recovery_enqueue_failure(
                            &failed_session_id,
                            &failed_turn_intent_id,
                        )
                    } else {
                        let failure = format!("scheduler_enqueue_failed: {enqueue_failure}");
                        agent_org_user_directed_work::mark_turn_terminal(
                            &failed_session_id,
                            &failed_turn_intent_id,
                            UserDirectedDeliveryStatus::Failed,
                            Some(&failure),
                        )
                    }
                })
                .await
                .map_err(|join_error| {
                    format!(
                        "Failed to enqueue message: {error}; failed to persist terminal evidence: {join_error}"
                    )
                })??;
            }
            return Err(format!("Failed to enqueue message: {error}"));
        }
    };

    tracing::info!(
        "[agent_send_message] Enqueued message {} at position {} for session {}",
        enqueue_result.message_id,
        enqueue_result.queue_position,
        session_id
    );

    Ok(AgentResponse {
        content: serde_json::json!({
            "queued": true,
            "messageId": enqueue_result.message_id,
            "queuePosition": enqueue_result.queue_position,
            "duplicate": enqueue_result.duplicate,
            "interventionReceiptId": intervention_receipt_id,
            "runtimeAdmissionState": direct_user_directed_work
                .as_ref()
                .map(|accepted| accepted.runtime_admission_status.as_str()),
        })
        .to_string(),
        session_id,
        model: effective_model,
    })
}

#[cfg(test)]
mod admission_tests {
    use super::*;

    #[test]
    fn ordinary_resume_wake_creates_context_but_pause_continuation_reuses_it() {
        assert!(!resume_requires_existing_agent_org_context(
            TurnIntentBridgeSource::Resume,
            Some("worker")
        ));
        assert!(resume_requires_existing_agent_org_context(
            TurnIntentBridgeSource::Resume,
            None
        ));
        assert!(!resume_requires_existing_agent_org_context(
            TurnIntentBridgeSource::Queue,
            None
        ));
    }

    #[test]
    fn agent_org_submissions_never_create_standalone_goal_state() {
        assert!(!should_record_standalone_goal(
            TurnIntentBridgeSource::UserSubmit,
            false,
            true
        ));
        assert!(should_record_standalone_goal(
            TurnIntentBridgeSource::UserSubmit,
            false,
            false
        ));
        assert!(!should_record_standalone_goal(
            TurnIntentBridgeSource::Queue,
            false,
            false
        ));
        assert!(!should_record_standalone_goal(
            TurnIntentBridgeSource::ForceSend,
            true,
            false
        ));
    }

    #[test]
    fn exact_direct_retry_never_replays_but_startup_can_recover_one_queued_turn() {
        assert!(should_dispatch_admitted_direct(false, "queued", false));
        assert!(!should_dispatch_admitted_direct(true, "queued", false));
        assert!(should_dispatch_admitted_direct(true, "queued", true));
        assert!(!should_dispatch_admitted_direct(true, "running", true));
        assert!(!should_dispatch_admitted_direct(true, "completed", true));
        assert!(!should_dispatch_admitted_direct(true, "failed", true));
    }
}
