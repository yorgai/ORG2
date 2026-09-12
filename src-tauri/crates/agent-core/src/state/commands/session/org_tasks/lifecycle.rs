//! Durable Agent Org Pause/Resume commands and post-commit runtime handoff.

use std::sync::Arc;
use std::time::Duration;

use crate::coordination::agent_inbox::AgentInboxStore;
use crate::coordination::agent_org_archive::{
    ArchiveRunOutcome, ArchiveTeardownTarget, ARCHIVE_TEARDOWN_MAX_ATTEMPTS,
};
use crate::coordination::agent_org_pause::{
    ContinuationDispatch, PauseRunOutcome, ResumeRunOutcome,
};
use crate::coordination::agent_org_runs::{AgentOrgRunContext, COORDINATOR_MEMBER_ID};
use crate::foundation::session_bridge::TurnIntentBridgeSource;
use crate::state::commands::session::identity::IdentityOverrides;
use crate::state::control_flow::CancelReason;
use crate::state::session_runtime::RuntimeTurnIdentity;
use crate::state::{AgentAppState, AgentSession};

use super::context::session_org_read_context;

const DRAIN_DEADLINE: Duration = Duration::from_secs(10);
const USER_DIRECTED_YIELD_SLO: Duration = Duration::from_secs(5);
const DRAIN_OBSERVATION_INTERVAL: Duration = Duration::from_millis(100);
const CONTINUATION_DISPATCH_LIMIT: usize = 256;
const ARCHIVE_ROUND_TIMEOUT: Duration = Duration::from_secs(10);
const ARCHIVE_RETRY_BACKOFFS: [Duration; 2] = [Duration::from_secs(5), Duration::from_secs(15)];
const ARCHIVE_RECONCILE_LIMIT: usize = 128;

fn archive_retry_backoff(attempt_count: i64, remaining: Duration) -> Duration {
    let index = usize::try_from(attempt_count.saturating_sub(1))
        .unwrap_or(0)
        .min(ARCHIVE_RETRY_BACKOFFS.len() - 1);
    ARCHIVE_RETRY_BACKOFFS[index].min(remaining)
}

fn archive_retry_delay(attempt_count: i64, remaining: Duration) -> Option<Duration> {
    (attempt_count < ARCHIVE_TEARDOWN_MAX_ATTEMPTS && !remaining.is_zero())
        .then(|| archive_retry_backoff(attempt_count, remaining))
}

#[tauri::command]
pub async fn agent_org_archive_run(
    state: tauri::State<'_, AgentAppState>,
    session_id: String,
    request_id: String,
) -> Result<ArchiveRunOutcome, String> {
    crate::coordination::agent_org_runs::require_agent_org_enabled()?;
    let read_context = session_org_read_context(&state, &session_id)
        .await?
        .ok_or_else(|| format!("Session {session_id} is not part of an Agent Org run"))?;
    let context = read_context
        .context
        .ok_or_else(|| format!("Session {session_id} has no Agent Org context"))?;
    let run_id = context.run_id.clone();
    let archive_run_id = run_id.clone();
    let commit = tokio::task::spawn_blocking(move || {
        crate::coordination::agent_org_archive::archive_run_commit(&archive_run_id, &request_id)
    })
    .await
    .map_err(|error| format!("Agent Org Archive transaction worker failed: {error}"))??;
    let outcome = commit.outcome;
    crate::coordination::agent_org_run_events::notify_agent_org_run_changed(&run_id);

    if commit.owns_teardown {
        schedule_archive_teardown(state.inner().clone(), outcome.receipt_id.clone());
    }
    Ok(outcome)
}

#[tauri::command]
pub async fn agent_org_pause_run(
    state: tauri::State<'_, AgentAppState>,
    session_id: String,
    request_id: String,
) -> Result<PauseRunOutcome, String> {
    crate::coordination::agent_org_runs::require_agent_org_enabled()?;
    let read_context = session_org_read_context(&state, &session_id)
        .await?
        .ok_or_else(|| format!("Session {session_id} is not part of an Agent Org run"))?;
    let context = read_context
        .context
        .ok_or_else(|| format!("Session {session_id} has no Agent Org context"))?;
    let run_id = context.run_id.clone();
    let pause_run_id = run_id.clone();
    let commit = tokio::task::spawn_blocking(move || {
        crate::coordination::agent_org_pause::pause_run_commit(&pause_run_id, &request_id)
    })
    .await
    .map_err(|error| format!("Agent Org Pause transaction worker failed: {error}"))??;
    let outcome = commit.outcome;

    crate::coordination::agent_org_run_events::notify_agent_org_run_changed(&run_id);
    if let Some(teardown_owner_id) = commit.teardown_owner_id {
        let teardown_state = state.inner().clone();
        let teardown_episode_id = outcome.episode_id.clone();
        let teardown_run_id = run_id;
        tauri::async_runtime::spawn(async move {
            if let Err(error) = teardown_pause_episode(
                teardown_state,
                teardown_run_id.clone(),
                teardown_episode_id,
                teardown_owner_id,
            )
            .await
            {
                tracing::warn!(
                    run_id = %teardown_run_id,
                    error = %error,
                    "Agent Org Pause fence committed, but runtime drain owner failed"
                );
            }
        });
    }
    Ok(outcome)
}

#[tauri::command]
pub async fn agent_org_resume_run(
    state: tauri::State<'_, AgentAppState>,
    session_id: String,
    request_id: String,
) -> Result<ResumeRunOutcome, String> {
    crate::coordination::agent_org_runs::require_agent_org_enabled()?;
    let read_context = session_org_read_context(&state, &session_id)
        .await?
        .ok_or_else(|| format!("Session {session_id} is not part of an Agent Org run"))?;
    let context = read_context
        .context
        .ok_or_else(|| format!("Session {session_id} has no Agent Org context"))?;
    let run_id = context.run_id.clone();
    let resume_run_id = run_id.clone();
    let outcome = tokio::task::spawn_blocking(move || {
        crate::coordination::agent_org_pause::resume_run(&resume_run_id, &request_id)
    })
    .await
    .map_err(|error| format!("Agent Org Resume transaction worker failed: {error}"))??;

    crate::coordination::agent_org_run_events::notify_agent_org_run_changed(&run_id);
    schedule_ready_continuations(state.inner().clone());
    if let Some(app_handle) = state.app_handle.clone() {
        schedule_non_continuation_progress_wakes(app_handle, context, outcome.episode_id.clone());
    }
    Ok(outcome)
}

fn schedule_archive_teardown(state: AgentAppState, receipt_id: String) {
    tauri::async_runtime::spawn(async move {
        if let Err(error) = teardown_archive_receipt(&state, &receipt_id).await {
            tracing::warn!(
                archive_receipt_id = %receipt_id,
                error = %error,
                "Agent Org Archive fence committed, but bounded runtime teardown failed"
            );
        }
    });
}

/// One-shot startup reconciliation. It intentionally installs no watchdog or
/// recurring timer; every pending receipt receives only its remaining bounded
/// attempts and then becomes quiesced or retained-runtime evidence.
pub fn reconcile_pending_archive_teardowns(state: AgentAppState) {
    tauri::async_runtime::spawn(async move {
        let receipts = match tokio::task::spawn_blocking(|| {
            crate::coordination::agent_org_archive::pending_receipt_ids(ARCHIVE_RECONCILE_LIMIT)
        })
        .await
        {
            Ok(Ok(receipts)) => receipts,
            Ok(Err(error)) => {
                tracing::warn!(error = %error, "failed to read pending Archive receipts");
                return;
            }
            Err(error) => {
                tracing::warn!(error = %error, "pending Archive receipt reader failed");
                return;
            }
        };
        for receipt_id in receipts {
            if let Err(error) = teardown_archive_receipt(&state, &receipt_id).await {
                tracing::warn!(
                    archive_receipt_id = %receipt_id,
                    error = %error,
                    "startup Archive teardown reconciliation failed"
                );
            }
        }
    });
}

async fn teardown_archive_receipt(state: &AgentAppState, receipt_id: &str) -> Result<(), String> {
    loop {
        let read_receipt_id = receipt_id.to_string();
        let targets = tokio::task::spawn_blocking(move || {
            crate::coordination::agent_org_archive::teardown_targets(&read_receipt_id)
        })
        .await
        .map_err(|error| format!("Archive teardown target reader failed: {error}"))??;
        if targets.is_empty() {
            return Ok(());
        }
        let run_id = targets[0].run_id.clone();
        let summary_run_id = run_id.clone();
        let pre_round_summary = tokio::task::spawn_blocking(move || {
            crate::coordination::agent_org_archive::summary_for_run(&summary_run_id)
        })
        .await
        .map_err(|error| format!("Archive teardown summary reader failed: {error}"))??
        .ok_or_else(|| "Archive teardown summary disappeared".to_string())?;
        let deadline = chrono::DateTime::parse_from_rfc3339(&pre_round_summary.deadline_at)
            .map_err(|error| format!("invalid Archive teardown deadline: {error}"))?
            .with_timezone(&chrono::Utc);
        let remaining = deadline.signed_duration_since(chrono::Utc::now());
        if remaining <= chrono::Duration::zero() {
            let expired_receipt_id = receipt_id.to_string();
            tokio::task::spawn_blocking(move || {
                crate::coordination::agent_org_archive::mark_deadline_expired(&expired_receipt_id)
            })
            .await
            .map_err(|error| format!("Archive teardown deadline writer failed: {error}"))??;
            crate::coordination::agent_org_run_events::notify_agent_org_run_changed(&run_id);
            return Ok(());
        }
        let round_timeout =
            ARCHIVE_ROUND_TIMEOUT.min(remaining.to_std().map_err(|error| {
                format!("Archive teardown deadline conversion failed: {error}")
            })?);
        let max_attempt = targets
            .iter()
            .map(|target| target.attempt_count)
            .max()
            .unwrap_or(0);
        if max_attempt >= ARCHIVE_TEARDOWN_MAX_ATTEMPTS {
            let expired_receipt_id = receipt_id.to_string();
            tokio::task::spawn_blocking(move || {
                crate::coordination::agent_org_archive::mark_deadline_expired(&expired_receipt_id)
            })
            .await
            .map_err(|error| format!("Archive teardown deadline writer failed: {error}"))??;
            crate::coordination::agent_org_run_events::notify_agent_org_run_changed(&run_id);
            return Ok(());
        }

        let mut round = tokio::task::JoinSet::new();
        for target in targets {
            let child_state = state.clone();
            round.spawn(async move {
                teardown_archive_target(&child_state, target, round_timeout).await
            });
        }
        while let Some(result) = round.join_next().await {
            match result {
                Ok(Ok(())) => {}
                Ok(Err(error)) => tracing::warn!(error = %error, "Archive target teardown failed"),
                Err(error) => tracing::warn!(error = %error, "Archive target teardown task failed"),
            }
        }
        crate::coordination::agent_org_run_events::notify_agent_org_run_changed(&run_id);

        let summary_receipt_id = receipt_id.to_string();
        let summary = tokio::task::spawn_blocking(move || {
            let conn = database::db::get_connection().map_err(|error| error.to_string())?;
            let run_id: String = conn
                .query_row(
                    "SELECT org_run_id FROM agent_org_runtime_archive_episodes
                     WHERE archive_receipt_id=?1",
                    [&summary_receipt_id],
                    |row| row.get(0),
                )
                .map_err(|error| error.to_string())?;
            crate::coordination::agent_org_archive::summary_for_run(&run_id)
        })
        .await
        .map_err(|error| format!("Archive teardown summary worker failed: {error}"))??;
        let Some(summary) = summary else {
            return Ok(());
        };
        if summary.status != crate::coordination::agent_org_archive::ArchiveTeardownStatus::Pending
        {
            return Ok(());
        }
        if summary.attempt_count >= ARCHIVE_TEARDOWN_MAX_ATTEMPTS {
            let expired_receipt_id = receipt_id.to_string();
            tokio::task::spawn_blocking(move || {
                crate::coordination::agent_org_archive::mark_deadline_expired(&expired_receipt_id)
            })
            .await
            .map_err(|error| format!("Archive teardown finalizer failed: {error}"))??;
            crate::coordination::agent_org_run_events::notify_agent_org_run_changed(&run_id);
            return Ok(());
        }
        let deadline = chrono::DateTime::parse_from_rfc3339(&summary.deadline_at)
            .map_err(|error| format!("invalid Archive teardown deadline: {error}"))?
            .with_timezone(&chrono::Utc);
        let remaining = deadline.signed_duration_since(chrono::Utc::now());
        if remaining <= chrono::Duration::zero() {
            continue;
        }
        let Some(backoff) = archive_retry_delay(
            summary.attempt_count,
            remaining.to_std().unwrap_or_default(),
        ) else {
            continue;
        };
        tokio::time::sleep(backoff).await;
    }
}

#[cfg(test)]
mod archive_retry_policy_tests {
    use super::{archive_retry_delay, ARCHIVE_RETRY_BACKOFFS, ARCHIVE_ROUND_TIMEOUT};
    use std::time::Duration;

    #[test]
    fn fake_clock_three_round_policy_stays_inside_absolute_sixty_second_budget() {
        let mut fake_elapsed = Duration::ZERO;
        let mut scheduled_timers = 0;
        for attempt in 1..=3 {
            fake_elapsed += ARCHIVE_ROUND_TIMEOUT;
            if let Some(delay) = archive_retry_delay(attempt, Duration::from_secs(60)) {
                scheduled_timers += 1;
                fake_elapsed += delay;
            }
        }
        assert_eq!(
            ARCHIVE_RETRY_BACKOFFS,
            [Duration::from_secs(5), Duration::from_secs(15)]
        );
        assert_eq!(fake_elapsed, Duration::from_secs(50));
        assert_eq!(scheduled_timers, 2);
        assert_eq!(archive_retry_delay(3, Duration::from_secs(10)), None);
        assert!(fake_elapsed <= Duration::from_secs(60));
    }

    #[test]
    fn retry_backoff_is_clamped_by_the_absolute_deadline() {
        assert_eq!(
            archive_retry_delay(1, Duration::from_secs(2)),
            Some(Duration::from_secs(2))
        );
        assert_eq!(
            archive_retry_delay(2, Duration::from_secs(7)),
            Some(Duration::from_secs(7))
        );
        assert_eq!(archive_retry_delay(2, Duration::ZERO), None);
    }
}

async fn teardown_archive_target(
    state: &AgentAppState,
    target: ArchiveTeardownTarget,
    round_timeout: Duration,
) -> Result<(), String> {
    // Seal first. `MemoryJobCoordinator` orders this against submission under
    // one mutex, so an old post-turn callback either installed its job before
    // the seal and is cancelled here, or observes the seal and is rejected.
    let cancelled_memory_jobs =
        crate::memory::background::seal_memory_jobs_for_session(&target.session_id);
    if cancelled_memory_jobs > 0 {
        tracing::info!(
            session_id = %target.session_id,
            cancelled_memory_jobs,
            "Archive cancelled session-owned background jobs"
        );
    }

    let session = state.get_session(&target.session_id).await;
    if let Some(session) = session.as_ref() {
        session.cancel_active_turn(CancelReason::OrgArchive).await;
    }
    crate::tools::impls::coding::exec::registry::request_cancel_for_session(&target.session_id);
    let captured = match session.as_ref() {
        Some(session) => session.runtime_lease_identity().await,
        None => None,
    };
    let lease_id = captured
        .as_ref()
        .map(|identity| identity.runtime_lease_id.clone());
    let turn_generation = captured
        .as_ref()
        .and_then(|identity| identity.dialog_turn_generation.clone());

    let released = tokio::time::timeout(round_timeout, async {
        let runtime_release = async {
            let (Some(session), Some(captured)) = (session, captured) else {
                return Ok::<bool, String>(true);
            };
            loop {
                let current = session.runtime_lease_identity().await;
                match current {
                    None => return Ok(true),
                    Some(current) if current.runtime_lease_id != captured.runtime_lease_id => {
                        return Err("archive_runtime_lease_replaced".to_string());
                    }
                    Some(current) if current.dialog_turn_generation.is_none() => {
                        return Ok(session
                            .release_runtime_lease_if_current(&captured.runtime_lease_id)
                            .await);
                    }
                    Some(_) => tokio::time::sleep(DRAIN_OBSERVATION_INTERVAL).await,
                }
            }
        };
        let memory_idle =
            crate::memory::background::wait_for_memory_jobs_for_session_idle(&target.session_id);
        let background_jobs_final =
            crate::tools::impls::coding::exec::registry::wait_for_session_finality(
                &target.session_id,
            );
        let (runtime_result, (), background_result) =
            tokio::join!(runtime_release, memory_idle, background_jobs_final);
        let runtime_released = runtime_result?;
        background_result?;

        // Runtime release closes the last in-flight registration path. Repeat
        // the idempotent Session barrier after that point so a job registered
        // during the first parallel wait cannot slip between the barrier and
        // the durable quiesced receipt.
        crate::tools::impls::coding::exec::registry::request_cancel_for_session(&target.session_id);
        crate::tools::impls::coding::exec::registry::wait_for_session_finality(&target.session_id)
            .await?;
        Ok::<bool, String>(runtime_released)
    })
    .await;

    match released {
        Ok(Ok(true)) => {
            persist_archive_teardown_attempt(target, lease_id, turn_generation, true, None).await
        }
        Ok(Ok(false)) => {
            persist_archive_teardown_attempt(
                target,
                lease_id,
                turn_generation,
                false,
                Some("archive_runtime_release_stale".to_string()),
            )
            .await
        }
        Ok(Err(error)) => {
            persist_archive_teardown_attempt(target, lease_id, turn_generation, false, Some(error))
                .await
        }
        Err(_) => {
            let blockers =
                crate::tools::impls::coding::exec::registry::execution_blockers_for_sessions(
                    std::slice::from_ref(&target.session_id),
                    8,
                );
            let blocker_evidence = blockers
                .iter()
                .map(|job| format!("{}:{}:{}", job.kind, job.handle, job.execution_state))
                .collect::<Vec<_>>()
                .join(",");
            let error = if blocker_evidence.is_empty() {
                "archive_runtime_memory_or_jobs_timeout".to_string()
            } else {
                format!("archive_runtime_memory_or_jobs_timeout:{blocker_evidence}")
            };
            persist_archive_teardown_attempt(target, lease_id, turn_generation, false, Some(error))
                .await
        }
    }
}

async fn persist_archive_teardown_attempt(
    target: ArchiveTeardownTarget,
    runtime_lease_id: Option<String>,
    dialog_turn_generation: Option<String>,
    released: bool,
    error: Option<String>,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        crate::coordination::agent_org_archive::record_teardown_attempt(
            &target,
            runtime_lease_id.as_deref(),
            dialog_turn_generation.as_deref(),
            released,
            error.as_deref(),
        )
        .map(|_| ())
    })
    .await
    .map_err(|error| format!("Archive teardown receipt worker failed: {error}"))?
}

async fn teardown_pause_episode(
    state: AgentAppState,
    run_id: String,
    episode_id: String,
    teardown_owner_id: String,
) -> Result<(), String> {
    let deadline_at = tokio::time::Instant::now() + DRAIN_DEADLINE;
    let read_episode_id = episode_id.clone();
    let handoffs = tokio::task::spawn_blocking(move || {
        crate::coordination::agent_org_pause::list_running_handoffs(
            &read_episode_id,
            &teardown_owner_id,
        )
    })
    .await
    .map_err(|error| format!("Pause handoff reader failed: {error}"))??;

    let mut signals = tokio::task::JoinSet::new();
    for handoff in handoffs {
        let child_state = state.clone();
        signals.spawn(async move { request_exact_handoff_yield(&child_state, handoff).await });
    }
    let mut signal_deadline_elapsed = false;
    loop {
        let result = match tokio::time::timeout_at(deadline_at, signals.join_next()).await {
            Ok(Some(result)) => result,
            Ok(None) => break,
            Err(_) => {
                signal_deadline_elapsed = true;
                signals.abort_all();
                break;
            }
        };
        match result {
            Ok(Ok(())) => {}
            Ok(Err(error)) => tracing::warn!(
                run_id = %run_id,
                error = %error,
                "failed to signal one captured Agent Org runtime"
            ),
            Err(error) => tracing::warn!(
                run_id = %run_id,
                error = %error,
                "captured Agent Org runtime signal task failed"
            ),
        }
    }

    let observed_episode_id = episode_id.clone();
    let observer_run_id = run_id.clone();
    let drained = tokio::time::timeout_at(deadline_at, async move {
        loop {
            let summary_episode_id = observed_episode_id.clone();
            let summary_run_id = observer_run_id.clone();
            let summary = tokio::task::spawn_blocking(move || {
                crate::coordination::agent_org_pause::pause_summary_for_run(&summary_run_id)
            })
            .await
            .map_err(|error| format!("Pause drain observer failed: {error}"))??;
            match summary {
                Some(summary)
                    if summary.episode_id == summary_episode_id && summary.draining_count == 0 =>
                {
                    return Ok::<(), String>(());
                }
                Some(summary) if summary.episode_id != summary_episode_id => return Ok(()),
                None => return Ok(()),
                _ => tokio::time::sleep(DRAIN_OBSERVATION_INTERVAL).await,
            }
        }
    })
    .await;

    if signal_deadline_elapsed || drained.is_err() {
        let timeout_episode_id = episode_id.clone();
        tokio::task::spawn_blocking(move || {
            crate::coordination::agent_org_pause::mark_unresolved_timed_out(&timeout_episode_id)
        })
        .await
        .map_err(|error| format!("Pause timeout writer failed: {error}"))??;
    } else if let Ok(Err(error)) = drained {
        return Err(error);
    }
    crate::coordination::agent_org_run_events::notify_agent_org_run_changed(&run_id);
    Ok(())
}

async fn request_exact_handoff_yield(
    state: &AgentAppState,
    handoff: crate::coordination::agent_org_pause::RunningPauseHandoff,
) -> Result<(), String> {
    let Some(session) = state.get_session(&handoff.session_id).await else {
        return persist_runtime_absent(handoff).await;
    };
    let Some(identity) = session.runtime_turn_identity().await else {
        // The durable Turn may have been promoted to Running just before
        // Pause, while `begin_turn_with_intent` has not installed its
        // in-memory identity yet. Preserve a pre-turn cancellation marker so
        // that narrow provider-start window still yields. This is safe only
        // for the identity-absent case; a mismatching active identity may be
        // future UserDirectedWork and is deliberately not cancelled here.
        session.cancel_active_turn(CancelReason::OrgPause).await;
        return persist_runtime_absent(handoff).await;
    };
    if identity.turn_intent_id.as_deref() != Some(handoff.turn_intent_id.as_str()) {
        return persist_runtime_absent(handoff).await;
    }

    let bind = handoff.clone();
    let runtime_lease_id = identity.runtime_lease_id.clone();
    let dialog_turn_generation = identity.dialog_turn_generation.clone();
    let bound = tokio::task::spawn_blocking(move || {
        crate::coordination::agent_org_pause::bind_runtime_and_request_yield(
            &bind.episode_id,
            &bind.session_id,
            &bind.turn_intent_id,
            &runtime_lease_id,
            &dialog_turn_generation,
        )
    })
    .await
    .map_err(|error| format!("Pause runtime binding worker failed: {error}"))??;
    if bound {
        session.cancel_active_turn(CancelReason::OrgPause).await;
    }
    Ok(())
}

async fn persist_runtime_absent(
    handoff: crate::coordination::agent_org_pause::RunningPauseHandoff,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        crate::coordination::agent_org_pause::mark_runtime_absent(
            &handoff.episode_id,
            &handoff.session_id,
            &handoff.turn_intent_id,
        )
        .map(|_| ())
    })
    .await
    .map_err(|error| format!("Pause runtime-absent worker failed: {error}"))?
}

/// Completion callback for every real Turn. Ordinary SDE turns do one indexed
/// read and return; only a runtime identity already bound to a Pause receipt
/// can clear the runtime slot.
pub(crate) async fn settle_pause_handoff_after_turn(
    state: &AgentAppState,
    session: &Arc<AgentSession>,
    run_id: Option<&str>,
    turn_intent_id: &str,
) {
    let Some(identity) = session.runtime_turn_identity().await else {
        return;
    };
    if identity.turn_intent_id.as_deref() != Some(turn_intent_id) {
        return;
    }
    let lookup_session_id = session.id.clone();
    let lookup_intent_id = turn_intent_id.to_string();
    let lookup_lease = identity.runtime_lease_id.clone();
    let lookup_generation = identity.dialog_turn_generation.clone();
    let episode = match tokio::task::spawn_blocking(move || {
        crate::coordination::agent_org_pause::bound_episode_for_runtime(
            &lookup_session_id,
            &lookup_intent_id,
            &lookup_lease,
            &lookup_generation,
        )
    })
    .await
    {
        Ok(Ok(episode)) => episode,
        Ok(Err(error)) => {
            tracing::warn!(session_id = %session.id, error = %error, "failed to read Pause handoff receipt");
            return;
        }
        Err(error) => {
            tracing::warn!(session_id = %session.id, error = %error, "Pause handoff receipt worker failed");
            return;
        }
    };
    if episode.is_none() {
        return;
    }
    let process_owner = crate::tools::call_context::TurnProcessOwner {
        session_id: session.id.clone(),
        turn_intent_id: turn_intent_id.to_string(),
        runtime_lease_id: identity.runtime_lease_id.clone(),
        dialog_turn_generation: identity.dialog_turn_generation.clone(),
    };
    if let Err(error) =
        crate::tools::impls::coding::exec::registry::cancel_and_await_jobs_for_owner(
            &process_owner,
            DRAIN_DEADLINE,
        )
        .await
    {
        tracing::warn!(
            session_id = %session.id,
            runtime_lease_id = %identity.runtime_lease_id,
            error = %error,
            "Pause handoff remains draining because exact-owner background work is not terminal"
        );
        return;
    }
    let released_current_slot = session
        .release_runtime_if_current(&identity.runtime_lease_id, &identity.dialog_turn_generation)
        .await;
    if !released_current_slot {
        tracing::debug!(
            session_id = %session.id,
            runtime_lease_id = %identity.runtime_lease_id,
            "Pause completion observed a replaced runtime lease; preserving the current slot"
        );
        return;
    }
    let release_session_id = session.id.clone();
    let release_intent_id = turn_intent_id.to_string();
    let release_lease = identity.runtime_lease_id;
    let release_generation = identity.dialog_turn_generation;
    match tokio::task::spawn_blocking(move || {
        crate::coordination::agent_org_pause::mark_released(
            &release_session_id,
            &release_intent_id,
            &release_lease,
            &release_generation,
        )
    })
    .await
    {
        Ok(Ok(_)) => {}
        Ok(Err(error)) => {
            tracing::warn!(session_id = %session.id, error = %error, "failed to persist Pause release receipt");
            return;
        }
        Err(error) => {
            tracing::warn!(session_id = %session.id, error = %error, "Pause release receipt worker failed");
            return;
        }
    }
    if let Some(run_id) = run_id {
        crate::coordination::agent_org_run_events::notify_agent_org_run_changed(run_id);
    }
    schedule_ready_continuations(state.clone());
}

/// Release the exact formal runtime lease captured by a direct-user
/// intervention. This runs at the formal Turn boundary before the queued UDW
/// starts, so the same scheduler worker may initialize one replacement lease;
/// no parallel runtime or dispatcher is introduced.
pub(crate) fn schedule_user_directed_yield_timeout_observer(receipt_id: String) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(USER_DIRECTED_YIELD_SLO).await;
        let slo_receipt_id = receipt_id.clone();
        let still_waiting = tokio::task::spawn_blocking(move || {
            crate::coordination::agent_member_interventions::AgentMemberInterventionStore::get_by_receipt(
                &slo_receipt_id,
            )
            .map(|receipt| {
                receipt.is_some_and(|receipt| {
                    receipt.status
                        == crate::coordination::agent_member_interventions::MemberInterventionStatus::YieldRequested
                })
            })
        })
        .await;
        match still_waiting {
            Ok(Ok(true)) => tracing::warn!(
                intervention_receipt_id = %receipt_id,
                event = "agent_org_user_directed_yield_slo_exceeded",
                "UserDirectedWork handoff exceeded the five-second yield SLO"
            ),
            Ok(Ok(false)) => return,
            Ok(Err(error)) => {
                tracing::warn!(
                    intervention_receipt_id = %receipt_id,
                    error = %error,
                    "failed to inspect UserDirectedWork yield SLO"
                );
                return;
            }
            Err(error) => {
                tracing::warn!(
                    intervention_receipt_id = %receipt_id,
                    error = %error,
                    "UserDirectedWork yield SLO observer failed"
                );
                return;
            }
        }

        tokio::time::sleep(USER_DIRECTED_YIELD_SLO).await;
        let timeout_receipt_id = receipt_id.clone();
        match tokio::task::spawn_blocking(move || {
            crate::coordination::agent_member_interventions::AgentMemberInterventionStore::mark_yield_timeout(
                &timeout_receipt_id,
            )
        })
        .await
        {
            Ok(Ok(true)) => tracing::warn!(
                intervention_receipt_id = %receipt_id,
                event = "agent_org_user_directed_yield_timeout",
                "UserDirectedWork remains durably waiting after the ten-second hard yield bound"
            ),
            Ok(Ok(false)) => {}
            Ok(Err(error)) => tracing::warn!(
                intervention_receipt_id = %receipt_id,
                error = %error,
                "failed to persist UserDirectedWork yield timeout"
            ),
            Err(error) => tracing::warn!(
                intervention_receipt_id = %receipt_id,
                error = %error,
                "UserDirectedWork yield timeout worker failed"
            ),
        }
    });
}

pub(crate) async fn settle_user_directed_handoff_after_turn(
    session: &Arc<AgentSession>,
    turn_intent_id: &str,
    captured_identity: Option<&RuntimeTurnIdentity>,
) {
    let identity = match captured_identity {
        Some(identity) => identity.clone(),
        None => match session.runtime_turn_identity().await {
            Some(identity) => identity,
            None => return,
        },
    };
    if identity.turn_intent_id.as_deref() != Some(turn_intent_id) {
        return;
    }

    let lookup_session_id = session.id.clone();
    let lookup_turn_intent_id = turn_intent_id.to_string();
    let lookup_lease = identity.runtime_lease_id.clone();
    let lookup_generation = identity.dialog_turn_generation.clone();
    let receipt_id = match tokio::task::spawn_blocking(move || -> Result<Option<String>, String> {
        let bound = crate::coordination::agent_member_interventions::AgentMemberInterventionStore::bound_receipt_for_runtime(
                &lookup_session_id,
                &lookup_turn_intent_id,
                &lookup_lease,
                &lookup_generation,
            )?;
        if bound.is_some() {
            return Ok(bound);
        }
        let pending = crate::coordination::agent_member_interventions::AgentMemberInterventionStore::receipt_for_original_turn(
                &lookup_session_id,
                &lookup_turn_intent_id,
            )?;
        if let Some(receipt_id) = pending.as_deref() {
            let bound = crate::coordination::agent_member_interventions::AgentMemberInterventionStore::bind_runtime_and_request_yield(
                    receipt_id,
                    &lookup_turn_intent_id,
                    &lookup_lease,
                    &lookup_generation,
                )?;
            if !bound {
                return Ok(None);
            }
        }
        Ok(pending)
    })
    .await
    {
        Ok(Ok(value)) => value,
        Ok(Err(error)) => {
            tracing::warn!(session_id = %session.id, error = %error, "failed to read UserDirectedWork handoff receipt");
            return;
        }
        Err(error) => {
            tracing::warn!(session_id = %session.id, error = %error, "UserDirectedWork handoff receipt worker failed");
            return;
        }
    };
    let Some(receipt_id) = receipt_id else {
        return;
    };

    let process_owner = crate::tools::call_context::TurnProcessOwner {
        session_id: session.id.clone(),
        turn_intent_id: turn_intent_id.to_string(),
        runtime_lease_id: identity.runtime_lease_id.clone(),
        dialog_turn_generation: identity.dialog_turn_generation.clone(),
    };
    if let Err(slo_error) =
        crate::tools::impls::coding::exec::registry::cancel_and_await_jobs_for_owner(
            &process_owner,
            USER_DIRECTED_YIELD_SLO,
        )
        .await
    {
        tracing::warn!(
            session_id = %session.id,
            intervention_receipt_id = %receipt_id,
            error = %slo_error,
            event = "agent_org_user_directed_yield_slo_exceeded",
            "UserDirectedWork handoff exceeded the five-second yield SLO"
        );
        if let Err(hard_error) =
            crate::tools::impls::coding::exec::registry::cancel_and_await_jobs_for_owner(
                &process_owner,
                USER_DIRECTED_YIELD_SLO,
            )
            .await
        {
            let timeout_receipt = receipt_id.clone();
            let _ = tokio::task::spawn_blocking(move || {
                crate::coordination::agent_member_interventions::AgentMemberInterventionStore::mark_yield_timeout(
                    &timeout_receipt,
                )
            })
            .await;
            tracing::warn!(
                session_id = %session.id,
                intervention_receipt_id = %receipt_id,
                error = %hard_error,
                event = "agent_org_user_directed_yield_timeout",
                "UserDirectedWork remains durably waiting after the ten-second hard yield bound"
            );
            return;
        }
    }

    let released_current = session
        .release_runtime_if_current(&identity.runtime_lease_id, &identity.dialog_turn_generation)
        .await;
    let released = if released_current {
        true
    } else {
        // A terminal Agent Org path may already have cleared the live Turn
        // identity. The captured lease still makes this fail closed: an idle
        // slot is released only if the exact lease is still current, while a
        // replacement/newer Turn leaves active identity installed and wins.
        session
            .release_yielded_runtime_if_idle(&identity.runtime_lease_id)
            .await
    };
    if !released {
        tracing::warn!(
            session_id = %session.id,
            intervention_receipt_id = %receipt_id,
            runtime_lease_id = %identity.runtime_lease_id,
            dialog_turn_generation = %identity.dialog_turn_generation,
            "UserDirectedWork handoff did not own the current runtime lease"
        );
        return;
    }
    let release_lease = identity.runtime_lease_id;
    let release_generation = identity.dialog_turn_generation;
    match tokio::task::spawn_blocking(move || {
        crate::coordination::agent_member_interventions::AgentMemberInterventionStore::mark_yield_released(
            &receipt_id,
            &release_lease,
            &release_generation,
        )
    })
    .await
    {
        Ok(Ok(_)) => {}
        Ok(Err(error)) => tracing::warn!(session_id = %session.id, error = %error, "failed to mark UserDirectedWork handoff released"),
        Err(error) => tracing::warn!(session_id = %session.id, error = %error, "UserDirectedWork release worker failed"),
    }
}

pub(crate) fn schedule_ready_continuations(state: AgentAppState) {
    tauri::async_runtime::spawn(async move {
        if let Err(error) = dispatch_ready_continuations(&state).await {
            tracing::warn!(error = %error, "Agent Org continuation dispatcher failed");
        }
    });
}

pub(crate) async fn dispatch_ready_continuations(state: &AgentAppState) -> Result<usize, String> {
    let dispatches = tokio::task::spawn_blocking(|| {
        crate::coordination::agent_org_pause::list_dispatchable_continuations(
            CONTINUATION_DISPATCH_LIMIT,
        )
    })
    .await
    .map_err(|error| format!("continuation reader failed: {error}"))??;
    let mut dispatched = 0usize;
    for dispatch in dispatches {
        let episode_id = dispatch.episode_id.clone();
        let turn_intent_id = dispatch.turn_intent_id.clone();
        let claimed = tokio::task::spawn_blocking(move || {
            crate::coordination::agent_org_pause::claim_continuation_dispatch(
                &episode_id,
                &turn_intent_id,
            )
        })
        .await
        .map_err(|error| format!("continuation receipt worker failed: {error}"))??;
        if !claimed {
            continue;
        }
        if let Err(error) = dispatch_one_continuation(state, &dispatch).await {
            let episode_id = dispatch.episode_id.clone();
            let turn_intent_id = dispatch.turn_intent_id.clone();
            if let Err(requeue_error) = tokio::task::spawn_blocking(move || {
                crate::coordination::agent_org_pause::requeue_continuation_dispatch(
                    &episode_id,
                    &turn_intent_id,
                )
            })
            .await
            .map_err(|join_error| join_error.to_string())
            .and_then(|result| result.map_err(|persist_error| persist_error.to_string()))
            {
                tracing::warn!(error = %requeue_error, "failed to requeue continuation after dispatch error");
            }
            return Err(error);
        }
        dispatched += 1;
    }
    Ok(dispatched)
}

async fn dispatch_one_continuation(
    state: &AgentAppState,
    dispatch: &ContinuationDispatch,
) -> Result<(), String> {
    let mode = if dispatch.turn_kind == "task_execution" {
        super::super::message::resolve_agent_org_wake_mode(
            &dispatch.session_id,
            &dispatch.run_id,
            &dispatch.turn_intent_id,
        )?
        .map(|value| value.as_str().to_string())
    } else {
        None
    };
    // Resume continues the captured formal Turn; it is not a new user
    // submission. Empty content prevents a synthetic transcript row. The
    // processor derives a transient provider nudge from the durable receipt.
    let content = String::new();
    super::super::message::send_message_impl(
        state,
        dispatch.session_id.clone(),
        content,
        None,
        IdentityOverrides::default(),
        mode,
        None,
        None,
        true,
        None,
        None,
        false,
        Some(dispatch.turn_intent_id.clone()),
        Some(dispatch.turn_intent_id.clone()),
        None,
        None,
        Some(dispatch.run_id.clone()),
        TurnIntentBridgeSource::Resume,
    )
    .await?;
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AgentOrgWakeReason {
    UnreadInbox,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct AgentOrgWakeTarget {
    pub(super) member_id: String,
    pub(super) reason: AgentOrgWakeReason,
}

pub(super) fn should_wake_member_for_progress(has_unread: bool) -> Option<AgentOrgWakeReason> {
    has_unread.then_some(AgentOrgWakeReason::UnreadInbox)
}

pub(super) fn collect_run_progress_wake_targets(
    run_id: &str,
    member_ids: &[String],
) -> Result<Vec<AgentOrgWakeTarget>, String> {
    let mut targets = Vec::new();
    for member_id in member_ids {
        if let Some(reason) = should_wake_member_for_progress(
            AgentInboxStore::has_unread_for_member(member_id, run_id)?,
        ) {
            targets.push(AgentOrgWakeTarget {
                member_id: member_id.clone(),
                reason,
            });
        }
    }
    Ok(targets)
}

pub(super) fn org_progress_member_ids(context: &AgentOrgRunContext) -> Vec<String> {
    std::iter::once(COORDINATOR_MEMBER_ID.to_string())
        .chain(
            context
                .members
                .iter()
                .map(|member| member.member_id.clone()),
        )
        .collect()
}

pub(super) fn wake_agent_org_member(app_handle: tauri::AppHandle, member_id: &str, run_id: &str) {
    use crate::core::tools::impls::orchestration::inbox_wake::AppHandleInboxWakeHook;
    use crate::tools::impls::orchestration::org_send_message::InboxWakeHook;
    AppHandleInboxWakeHook::new(app_handle).wake_member(member_id, run_id);
}

fn schedule_non_continuation_progress_wakes(
    app_handle: tauri::AppHandle,
    context: AgentOrgRunContext,
    episode_id: String,
) {
    tauri::async_runtime::spawn(async move {
        let run_id = context.run_id.clone();
        let member_ids = org_progress_member_ids(&context);
        let query_run_id = run_id.clone();
        let result = tokio::task::spawn_blocking(move || {
            let continued =
                crate::coordination::agent_org_pause::continuation_participant_ids(&episode_id)?
                    .into_iter()
                    .collect::<std::collections::HashSet<_>>();
            let candidates = member_ids
                .into_iter()
                .filter(|member_id| !continued.contains(member_id))
                .collect::<Vec<_>>();
            collect_run_progress_wake_targets(&query_run_id, &candidates)
        })
        .await;
        match result {
            Ok(Ok(targets)) => {
                for target in targets {
                    wake_agent_org_member(app_handle.clone(), &target.member_id, &run_id);
                }
            }
            Ok(Err(error)) => {
                tracing::warn!(run_id = %run_id, error = %error, "failed to collect Resume wake targets")
            }
            Err(error) => {
                tracing::warn!(run_id = %run_id, error = %error, "Resume wake-target worker failed")
            }
        }
    });
}
