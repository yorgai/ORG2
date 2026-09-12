//! Typed Group-origin submit, exact Stop, and exact Retry commands.

use database::db::{get_connection, with_sessions_writer};
use rusqlite::{params, OptionalExtension};
use serde::Serialize;

use crate::coordination::agent_org_runs::{
    AgentOrgRunStatus, AgentOrgRunStore, COORDINATOR_MEMBER_ID,
};
use crate::coordination::agent_org_user_directed_work::{self, ExactGroupCancellationState};
use crate::foundation::session_bridge::TurnIntentBridgeSource;
use crate::state::commands::session::identity::IdentityOverrides;
use crate::state::control_flow::CancelReason;
use crate::state::AgentAppState;

use super::context::session_org_read_context;
use super::group_chat::AgentOrgGroupDeliveryInput;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentOrgGroupRootMessageResponse {
    pub turn_intent_id: String,
    pub target_member_id: String,
    pub target_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentOrgGroupStopOutcome {
    QueuedCancelled,
    CancellationRequested,
    AlreadyTerminal,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentOrgGroupStopResponse {
    pub turn_intent_id: String,
    pub outcome: AgentOrgGroupStopOutcome,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentOrgGroupRetryOutcome {
    Rekicked,
    Created,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentOrgGroupRetryResponse {
    pub source_turn_intent_id: String,
    pub turn_intent_id: String,
    pub outcome: AgentOrgGroupRetryOutcome,
}

#[derive(Debug, Clone)]
struct GroupTurnIdentity {
    run_id: String,
    session_id: String,
    turn_intent_id: String,
    source_kind: String,
}

#[derive(Debug, Clone)]
struct RetryEnvelope {
    identity: GroupTurnIdentity,
    participant_id: String,
    source_id: String,
    client_message_id: Option<String>,
    status: String,
    has_exact_reply: bool,
    content: String,
    display_text: String,
    images: Option<Vec<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RetryDisposition {
    Rekick,
    Create,
}

#[tauri::command]
pub async fn agent_org_send_group_root_message(
    state: tauri::State<'_, AgentAppState>,
    session_id: String,
    turn_intent_id: String,
    client_message_id: String,
    content: String,
    display_text: Option<String>,
    images: Option<Vec<String>>,
) -> Result<AgentOrgGroupRootMessageResponse, String> {
    crate::coordination::agent_org_runs::require_agent_org_enabled()?;
    let read = session_org_read_context(&state, &session_id)
        .await?
        .ok_or_else(|| "agent_org_group_root_not_found".to_string())?;
    let context = read
        .context
        .ok_or_else(|| "agent_org_group_root_not_found".to_string())?;
    let root_session_id = context
        .root_session_id
        .clone()
        .ok_or_else(|| "agent_org_group_root_not_materialized".to_string())?;
    validate_required_id("turn_intent_id", &turn_intent_id)?;
    validate_required_id("client_message_id", &client_message_id)?;
    if content.trim().is_empty() {
        return Err("group_message_content_required".to_string());
    }
    let preflight_run_id = context.run_id.clone();
    let preflight_root_session_id = root_session_id.clone();
    tokio::task::spawn_blocking(move || {
        let run = AgentOrgRunStore::load(&preflight_run_id)?.ok_or_else(|| {
            format!("team_unavailable: Agent Org run {preflight_run_id} does not exist")
        })?;
        if run.root_session_id.as_deref() != Some(preflight_root_session_id.as_str()) {
            return Err("agent_org_group_root_not_materialized".to_string());
        }
        match run.status {
            AgentOrgRunStatus::Running | AgentOrgRunStatus::Idle => Ok(()),
            AgentOrgRunStatus::Archived => {
                Err("team_archived: Agent Org Team is read-only".to_string())
            }
            status => Err(format!(
                "agent_org_turn_context_invalid: Coordinator GroupRoot Turn requires a running or Idle Team, found {status}"
            )),
        }
    })
    .await
    .map_err(|error| format!("GroupRoot preflight worker failed: {error}"))??;

    let source_event_id = format!("user-message-{client_message_id}");
    ensure_group_root_source_event(
        &state,
        &root_session_id,
        &client_message_id,
        &source_event_id,
        &turn_intent_id,
        &content,
        display_text.as_deref(),
        images.as_deref(),
    )?;

    let send_result = super::super::message::send_message_impl_for_group_root(
        &state,
        root_session_id.clone(),
        context.run_id.clone(),
        turn_intent_id.clone(),
        client_message_id.clone(),
        source_event_id.clone(),
        content,
        display_text,
        images,
    )
    .await;
    if let Err(error) = send_result {
        let cleanup_session_id = root_session_id;
        let cleanup_turn_intent_id = turn_intent_id.clone();
        let cleanup_client_message_id = client_message_id;
        let cleanup_source_event_id = source_event_id;
        let cleanup_result = tokio::task::spawn_blocking(move || {
            remove_unadmitted_group_root_source_event(
                &cleanup_session_id,
                &cleanup_turn_intent_id,
                &cleanup_client_message_id,
                &cleanup_source_event_id,
            )
        })
        .await
        .map_err(|join_error| format!("GroupRoot cleanup worker failed: {join_error}"))?;
        if let Err(cleanup_error) = cleanup_result {
            tracing::error!(
                event = "agent_org_group_root_source_cleanup_failed",
                session_id = %session_id,
                turn_intent_id = %turn_intent_id,
                error = %cleanup_error,
                "failed to remove an unadmitted GroupRoot source event"
            );
            return Err("group_root_source_cleanup_failed".to_string());
        }
        return Err(error);
    }
    crate::coordination::agent_org_run_events::notify_agent_org_run_changed(&context.run_id);
    Ok(AgentOrgGroupRootMessageResponse {
        turn_intent_id,
        target_member_id: COORDINATOR_MEMBER_ID.to_string(),
        target_name: context.coordinator_name,
    })
}

/// The EventStore user fact must exist before typed GroupRoot admission can
/// prove its exact source identity. If runtime initialization fails before
/// admission, remove only that still-unowned fact. The shared writer lock and
/// immediate transaction serialize this check against duplicate submissions:
/// once any caller has established the context, the source is retained for
/// idempotent re-kick and Provider continuity.
fn remove_unadmitted_group_root_source_event(
    session_id: &str,
    turn_intent_id: &str,
    client_message_id: &str,
    source_event_id: &str,
) -> Result<(), String> {
    with_sessions_writer(|| {
        let mut conn = get_connection().map_err(|error| error.to_string())?;
        remove_unadmitted_group_root_source_event_with_connection(
            &mut conn,
            session_id,
            turn_intent_id,
            client_message_id,
            source_event_id,
        )
    })
}

fn remove_unadmitted_group_root_source_event_with_connection(
    conn: &mut rusqlite::Connection,
    session_id: &str,
    turn_intent_id: &str,
    client_message_id: &str,
    source_event_id: &str,
) -> Result<(), String> {
    let tx = conn
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;
    let admitted: bool = tx
        .query_row(
            "SELECT EXISTS(
                   SELECT 1 FROM agent_org_runtime_turn_contexts
                   WHERE session_id=?1 AND turn_intent_id=?2
                     AND source_kind='group_root' AND source_id=?3
                 )",
            params![session_id, turn_intent_id, source_event_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if !admitted {
        tx.execute(
            "DELETE FROM events
                 WHERE id=?1 AND session_id=?2
                   AND json_valid(result_json)
                   AND json_extract(result_json,'$.messageId')=?3
                   AND json_extract(result_json,'$.turnIntentId')=?4
                   AND NOT EXISTS(
                     SELECT 1 FROM events reply
                     WHERE reply.session_id=?2 AND json_valid(reply.result_json)
                       AND json_extract(
                         reply.result_json,
                         '$.agent_org_group_root_reply.source_event_id'
                       )=?1
                   )",
            params![
                source_event_id,
                session_id,
                client_message_id,
                turn_intent_id
            ],
        )
        .map_err(|error| error.to_string())?;
    }
    tx.commit().map_err(|error| error.to_string())
}

#[allow(clippy::too_many_arguments)]
fn ensure_group_root_source_event(
    state: &AgentAppState,
    root_session_id: &str,
    client_message_id: &str,
    source_event_id: &str,
    turn_intent_id: &str,
    content: &str,
    display_text: Option<&str>,
    images: Option<&[String]>,
) -> Result<(), String> {
    let conn = get_connection().map_err(|error| error.to_string())?;
    let existing: Option<(String, String, Option<String>)> = conn
        .query_row(
            "SELECT session_id,result_json,meta_json FROM events WHERE id=?1",
            params![source_event_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    if let Some((session_id, result_json, meta_json)) = existing {
        let result: serde_json::Value = serde_json::from_str(&result_json)
            .map_err(|_| "group_root_source_conflict".to_string())?;
        let meta = meta_json
            .as_deref()
            .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok());
        let expected_display = display_text
            .filter(|value| !value.is_empty())
            .unwrap_or(content);
        let expected_images = images.filter(|items| !items.is_empty());
        let actual_images = result
            .get("images")
            .and_then(|value| serde_json::from_value::<Vec<String>>(value.clone()).ok());
        let same = session_id == root_session_id
            && result.get("messageId").and_then(serde_json::Value::as_str)
                == Some(client_message_id)
            && result
                .get("turnIntentId")
                .and_then(serde_json::Value::as_str)
                == Some(turn_intent_id)
            && result
                .pointer("/message/content")
                .and_then(serde_json::Value::as_str)
                == Some(content)
            && meta
                .as_ref()
                .and_then(|value| value.get("displayText"))
                .and_then(serde_json::Value::as_str)
                == Some(expected_display)
            && actual_images.as_deref() == expected_images;
        return same
            .then_some(())
            .ok_or_else(|| "group_root_source_conflict".to_string());
    }
    let app_handle = state
        .app_handle
        .as_ref()
        .ok_or_else(|| "group_root_event_pipeline_unavailable".to_string())?;
    crate::bus::event_pipeline_bridge::persist_user_message_event(
        app_handle,
        root_session_id,
        client_message_id,
        content,
        display_text,
        images,
        crate::bus::event_pipeline_bridge::PersistedUserMessageSource::AgentOrgGroupRoot,
        turn_intent_id,
    )
}

#[tauri::command]
pub async fn agent_org_stop_group_delivery(
    state: tauri::State<'_, AgentAppState>,
    session_id: String,
    turn_intent_id: String,
) -> Result<AgentOrgGroupStopResponse, String> {
    crate::coordination::agent_org_runs::require_agent_org_enabled()?;
    let identity = resolve_group_turn(&state, &session_id, &turn_intent_id).await?;
    let persisted = if identity.source_kind == "group_mention" {
        let session_id = identity.session_id.clone();
        let turn_intent_id = identity.turn_intent_id.clone();
        tokio::task::spawn_blocking(move || {
            agent_org_user_directed_work::prepare_exact_group_cancellation(
                &session_id,
                &turn_intent_id,
            )
        })
        .await
        .map_err(|error| format!("Group Stop worker failed: {error}"))??
        .1
    } else {
        let identity = identity.clone();
        tokio::task::spawn_blocking(move || prepare_group_root_cancellation(&identity))
            .await
            .map_err(|error| format!("Group Stop worker failed: {error}"))??
    };

    let outcome = match persisted {
        ExactGroupCancellationState::QueuedCancelled => AgentOrgGroupStopOutcome::QueuedCancelled,
        ExactGroupCancellationState::AlreadyTerminal => AgentOrgGroupStopOutcome::AlreadyTerminal,
        ExactGroupCancellationState::Started => AgentOrgGroupStopOutcome::CancellationRequested,
    };
    if persisted != ExactGroupCancellationState::AlreadyTerminal {
        if let Some(session) = state.get_session(&identity.session_id).await {
            let already_claimed = session.scheduler.invalidate_turn(&identity.turn_intent_id);
            if already_claimed {
                session
                    .cancel_active_turn_if_intent(
                        &identity.turn_intent_id,
                        CancelReason::UserDirectedStop,
                    )
                    .await;
            }
        } else if persisted == ExactGroupCancellationState::Started {
            return Err("group_delivery_runtime_unavailable".to_string());
        }
    }
    crate::coordination::agent_org_run_events::notify_agent_org_run_changed(&identity.run_id);
    Ok(AgentOrgGroupStopResponse {
        turn_intent_id,
        outcome,
    })
}

#[tauri::command]
pub async fn agent_org_retry_group_delivery(
    state: tauri::State<'_, AgentAppState>,
    session_id: String,
    source_turn_intent_id: String,
    retry_turn_intent_id: Option<String>,
    acknowledge_possible_duplicate: bool,
) -> Result<AgentOrgGroupRetryResponse, String> {
    crate::coordination::agent_org_runs::require_agent_org_enabled()?;
    let identity = resolve_group_turn(&state, &session_id, &source_turn_intent_id).await?;
    let envelope = tokio::task::spawn_blocking(move || load_retry_envelope(identity))
        .await
        .map_err(|error| format!("Group Retry worker failed: {error}"))??;

    match retry_disposition(
        &envelope.status,
        envelope.has_exact_reply,
        acknowledge_possible_duplicate,
    )? {
        RetryDisposition::Rekick => {
            rekick_existing(&state, &envelope).await?;
            Ok(AgentOrgGroupRetryResponse {
                source_turn_intent_id: source_turn_intent_id.clone(),
                turn_intent_id: source_turn_intent_id,
                outcome: AgentOrgGroupRetryOutcome::Rekicked,
            })
        }
        RetryDisposition::Create => {
            let retry_turn_intent_id = retry_turn_intent_id
                .as_deref()
                .ok_or_else(|| "group_retry_turn_intent_id_required".to_string())?;
            validate_required_id("retry_turn_intent_id", retry_turn_intent_id)?;
            if retry_turn_intent_id == source_turn_intent_id {
                return Err("group_retry_turn_intent_id_must_be_new".to_string());
            }
            create_retry(&state, &envelope, retry_turn_intent_id).await?;
            Ok(AgentOrgGroupRetryResponse {
                source_turn_intent_id,
                turn_intent_id: retry_turn_intent_id.to_string(),
                outcome: AgentOrgGroupRetryOutcome::Created,
            })
        }
    }
}

fn retry_disposition(
    status: &str,
    has_exact_reply: bool,
    acknowledge_possible_duplicate: bool,
) -> Result<RetryDisposition, String> {
    match status {
        "pending" | "queued" | "optimistic" if has_exact_reply => {
            Err("group_delivery_already_answered".to_string())
        }
        "pending" | "queued" | "optimistic" => Ok(RetryDisposition::Rekick),
        "started" | "running" => Err("group_delivery_already_running".to_string()),
        "unknown" if !acknowledge_possible_duplicate => {
            Err("group_retry_possible_duplicate_confirmation_required".to_string())
        }
        "completed" if has_exact_reply => Err("group_delivery_already_answered".to_string()),
        "completed" | "failed" | "cancelled" | "abandoned" | "stale" | "unknown" => {
            Ok(RetryDisposition::Create)
        }
        _ => Err("group_delivery_invalid_status".to_string()),
    }
}

async fn resolve_group_turn(
    state: &AgentAppState,
    session_id: &str,
    turn_intent_id: &str,
) -> Result<GroupTurnIdentity, String> {
    validate_required_id("turn_intent_id", turn_intent_id)?;
    let read = session_org_read_context(state, session_id)
        .await?
        .ok_or_else(|| "group_delivery_not_found".to_string())?;
    let run_id = read
        .context
        .map(|context| context.run_id)
        .ok_or_else(|| "group_delivery_not_found".to_string())?;
    let run_id_for_query = run_id.clone();
    let turn_intent_id_for_query = turn_intent_id.to_string();
    tokio::task::spawn_blocking(move || {
        let conn = get_connection().map_err(|error| error.to_string())?;
        resolve_group_turn_with_connection(&conn, &run_id_for_query, &turn_intent_id_for_query)
    })
    .await
    .map_err(|error| format!("Group identity worker failed: {error}"))?
}

fn resolve_group_turn_with_connection(
    conn: &rusqlite::Connection,
    run_id: &str,
    turn_intent_id: &str,
) -> Result<GroupTurnIdentity, String> {
    let mut stmt = conn
        .prepare(
            "SELECT session_id,turn_intent_id,source_kind
             FROM agent_org_runtime_turn_contexts
             WHERE org_run_id=?1 AND turn_intent_id=?2
               AND source_kind IN ('group_root','group_mention')
             ORDER BY context_id ASC
             LIMIT 2",
        )
        .map_err(|error| error.to_string())?;
    let rows = stmt
        .query_map(params![run_id, turn_intent_id], |row| {
            Ok(GroupTurnIdentity {
                run_id: run_id.to_string(),
                session_id: row.get(0)?,
                turn_intent_id: row.get(1)?,
                source_kind: row.get(2)?,
            })
        })
        .map_err(|error| error.to_string())?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|error| error.to_string())?;
    match rows.as_slice() {
        [identity] => Ok(identity.clone()),
        [] => Err("group_delivery_not_found".to_string()),
        _ => Err("group_delivery_ambiguous".to_string()),
    }
}

fn prepare_group_root_cancellation(
    identity: &GroupTurnIdentity,
) -> Result<ExactGroupCancellationState, String> {
    with_sessions_writer(|| {
        let mut conn = get_connection().map_err(|error| error.to_string())?;
        prepare_group_root_cancellation_with_connection(&mut conn, identity)
    })
}

fn prepare_group_root_cancellation_with_connection(
    conn: &mut rusqlite::Connection,
    identity: &GroupTurnIdentity,
) -> Result<ExactGroupCancellationState, String> {
    let tx = conn
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;
    let status: String = tx
        .query_row(
            "SELECT intent.status
             FROM session_turn_intents intent
             JOIN agent_org_runtime_turn_contexts context
               ON context.session_id=intent.session_id
              AND context.turn_intent_id=intent.turn_intent_id
             WHERE context.org_run_id=?1 AND context.session_id=?2
               AND context.turn_intent_id=?3 AND context.source_kind='group_root'",
            params![
                &identity.run_id,
                &identity.session_id,
                &identity.turn_intent_id
            ],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "group_delivery_not_found".to_string())?;
    let state = match status.as_str() {
        "optimistic" | "queued" => {
            let now = chrono::Utc::now().to_rfc3339();
            let changed = tx
                .execute(
                    "UPDATE session_turn_intents SET status='cancelled',updated_at=?3
                     WHERE session_id=?1 AND turn_intent_id=?2
                       AND status IN ('optimistic','queued')",
                    params![&identity.session_id, &identity.turn_intent_id, &now],
                )
                .map_err(|error| error.to_string())?;
            if changed != 1 {
                return Err("group_delivery_stop_conflict".to_string());
            }
            ExactGroupCancellationState::QueuedCancelled
        }
        "running" => ExactGroupCancellationState::Started,
        _ => ExactGroupCancellationState::AlreadyTerminal,
    };
    tx.commit().map_err(|error| error.to_string())?;
    Ok(state)
}

fn load_retry_envelope(identity: GroupTurnIdentity) -> Result<RetryEnvelope, String> {
    let conn = get_connection().map_err(|error| error.to_string())?;
    load_retry_envelope_with_connection(&conn, identity)
}

fn load_retry_envelope_with_connection(
    conn: &rusqlite::Connection,
    identity: GroupTurnIdentity,
) -> Result<RetryEnvelope, String> {
    let raw = conn
        .query_row(
            "SELECT context.participant_id,context.source_id,intent.client_message_id,
                    COALESCE(delivery.status,intent.status),
                    delivery.dispatch_content,delivery.display_content,delivery.images_json,
                    event.result_json,event.meta_json,
                    CASE context.source_kind
                      WHEN 'group_root' THEN EXISTS (
                        SELECT 1 FROM events reply
                        WHERE reply.session_id=context.session_id
                          AND CASE WHEN json_valid(reply.result_json)
                                   THEN json_type(reply.result_json,'$.agent_org_group_root_reply.source_event_id')='text'
                                   ELSE 0 END
                          AND json_extract(reply.result_json,'$.agent_org_group_root_reply.source_event_id')=context.source_id
                      )
                      ELSE EXISTS (
                        SELECT 1 FROM events reply
                        WHERE reply.session_id=context.session_id
                          AND CASE WHEN json_valid(reply.result_json)
                                   THEN json_extract(reply.result_json,'$.agent_org_user_directed_reply.source_kind')='group_mention'
                                   ELSE 0 END
                          AND CAST(json_extract(reply.result_json,'$.agent_org_user_directed_reply.source_inbox_id') AS INTEGER)=CAST(context.source_id AS INTEGER)
                      )
                    END
             FROM agent_org_runtime_turn_contexts context
             JOIN session_turn_intents intent
               ON intent.session_id=context.session_id
              AND intent.turn_intent_id=context.turn_intent_id
             LEFT JOIN agent_org_runtime_user_directed_deliveries delivery
               ON delivery.session_id=context.session_id
              AND delivery.turn_intent_id=context.turn_intent_id
             LEFT JOIN events event
               ON context.source_kind='group_root' AND event.id=context.source_id
             WHERE context.org_run_id=?1 AND context.session_id=?2
               AND context.turn_intent_id=?3
               AND context.source_kind IN ('group_root','group_mention')",
            params![
                &identity.run_id,
                &identity.session_id,
                &identity.turn_intent_id
            ],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, Option<String>>(7)?,
                    row.get::<_, Option<String>>(8)?,
                    row.get::<_, i64>(9)? != 0,
                ))
            },
        )
        .optional()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "group_delivery_not_found".to_string())?;
    let (
        participant_id,
        source_id,
        client_message_id,
        status,
        dispatch,
        display,
        images_json,
        result_json,
        meta_json,
        has_exact_reply,
    ) = raw;
    let (content, display_text, images) = if identity.source_kind == "group_mention" {
        let content = dispatch.ok_or_else(|| "group_retry_frozen_payload_missing".to_string())?;
        let display_text = display.unwrap_or_else(|| content.clone());
        let images = images_json
            .as_deref()
            .map(serde_json::from_str::<Vec<String>>)
            .transpose()
            .map_err(|_| "group_retry_frozen_payload_invalid".to_string())?
            .filter(|items| !items.is_empty());
        (content, display_text, images)
    } else {
        let result: serde_json::Value = serde_json::from_str(
            result_json
                .as_deref()
                .ok_or_else(|| "group_retry_frozen_payload_missing".to_string())?,
        )
        .map_err(|_| "group_retry_frozen_payload_invalid".to_string())?;
        let content = result
            .pointer("/message/content")
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| "group_retry_frozen_payload_invalid".to_string())?
            .to_string();
        let images = result
            .get("images")
            .cloned()
            .map(serde_json::from_value::<Vec<String>>)
            .transpose()
            .map_err(|_| "group_retry_frozen_payload_invalid".to_string())?
            .filter(|items| !items.is_empty());
        let display_text = meta_json
            .as_deref()
            .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
            .and_then(|meta| {
                meta.get("displayText")
                    .and_then(serde_json::Value::as_str)
                    .map(ToOwned::to_owned)
            })
            .unwrap_or_else(|| content.clone());
        (content, display_text, images)
    };
    Ok(RetryEnvelope {
        identity,
        participant_id,
        source_id,
        client_message_id,
        status,
        has_exact_reply,
        content,
        display_text,
        images,
    })
}

async fn rekick_existing(state: &AgentAppState, envelope: &RetryEnvelope) -> Result<(), String> {
    if envelope.identity.source_kind == "group_root" {
        let client_message_id = envelope
            .client_message_id
            .clone()
            .ok_or_else(|| "group_retry_frozen_payload_missing".to_string())?;
        super::super::message::send_message_impl_for_group_root(
            state,
            envelope.identity.session_id.clone(),
            envelope.identity.run_id.clone(),
            envelope.identity.turn_intent_id.clone(),
            client_message_id,
            envelope.source_id.clone(),
            envelope.content.clone(),
            Some(envelope.display_text.clone()),
            envelope.images.clone(),
        )
        .await?;
    } else {
        super::super::message::send_message_impl(
            state,
            envelope.identity.session_id.clone(),
            envelope.content.clone(),
            Some(envelope.display_text.clone()),
            IdentityOverrides::default(),
            None,
            envelope.images.clone(),
            None,
            false,
            None,
            None,
            false,
            Some(envelope.identity.turn_intent_id.clone()),
            Some(envelope.identity.turn_intent_id.clone()),
            None,
            None,
            Some(envelope.identity.run_id.clone()),
            TurnIntentBridgeSource::AgentOrg,
        )
        .await?;
    }
    Ok(())
}

async fn create_retry(
    state: &AgentAppState,
    envelope: &RetryEnvelope,
    retry_turn_intent_id: &str,
) -> Result<(), String> {
    if envelope.identity.source_kind == "group_mention" {
        super::group_chat::agent_org_send_group_chat_message_impl_with_display(
            state,
            envelope.identity.session_id.clone(),
            vec![AgentOrgGroupDeliveryInput {
                target_member_id: envelope.participant_id.clone(),
                turn_intent_id: retry_turn_intent_id.to_string(),
            }],
            envelope.content.clone(),
            Some(envelope.display_text.clone()),
            envelope.images.clone(),
        )
        .await?;
        return Ok(());
    }

    let message_id = format!("group-root-retry-{retry_turn_intent_id}");
    let source_event_id = format!("user-message-{message_id}");
    ensure_group_root_source_event(
        state,
        &envelope.identity.session_id,
        &message_id,
        &source_event_id,
        retry_turn_intent_id,
        &envelope.content,
        Some(&envelope.display_text),
        envelope.images.as_deref(),
    )?;
    let send_result = super::super::message::send_message_impl_for_group_root(
        state,
        envelope.identity.session_id.clone(),
        envelope.identity.run_id.clone(),
        retry_turn_intent_id.to_string(),
        message_id.clone(),
        source_event_id.clone(),
        envelope.content.clone(),
        Some(envelope.display_text.clone()),
        envelope.images.clone(),
    )
    .await;
    if let Err(error) = send_result {
        let cleanup_session_id = envelope.identity.session_id.clone();
        let cleanup_turn_intent_id = retry_turn_intent_id.to_string();
        let cleanup_message_id = message_id;
        let cleanup_source_event_id = source_event_id;
        tokio::task::spawn_blocking(move || {
            remove_unadmitted_group_root_source_event(
                &cleanup_session_id,
                &cleanup_turn_intent_id,
                &cleanup_message_id,
                &cleanup_source_event_id,
            )
        })
        .await
        .map_err(|join_error| format!("GroupRoot Retry cleanup worker failed: {join_error}"))?
        .map_err(|cleanup_error| {
            tracing::error!(
                event = "agent_org_group_root_retry_source_cleanup_failed",
                error = %cleanup_error,
                "failed to remove an unadmitted GroupRoot Retry source event"
            );
            "group_root_source_cleanup_failed".to_string()
        })?;
        return Err(error);
    }
    Ok(())
}

fn validate_required_id(label: &str, value: &str) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > 512 {
        return Err(format!("group_delivery_invalid_{label}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn connection() -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().expect("open Group action database");
        conn.execute_batch(
            "CREATE TABLE session_turn_intents (
               session_id TEXT NOT NULL,
               turn_intent_id TEXT NOT NULL,
               client_message_id TEXT,
               org_run_id TEXT,
               source TEXT NOT NULL,
               status TEXT NOT NULL,
               created_at TEXT NOT NULL,
               updated_at TEXT NOT NULL,
               PRIMARY KEY(session_id,turn_intent_id)
             );
             CREATE TABLE agent_org_runtime_turn_contexts (
               context_id INTEGER PRIMARY KEY AUTOINCREMENT,
               session_id TEXT NOT NULL,
               turn_intent_id TEXT NOT NULL,
               org_run_id TEXT NOT NULL,
               participant_id TEXT NOT NULL,
               source_kind TEXT NOT NULL,
               source_id TEXT NOT NULL,
               UNIQUE(session_id,turn_intent_id)
             );
             CREATE TABLE agent_org_runtime_user_directed_deliveries (
               session_id TEXT NOT NULL,
               turn_intent_id TEXT NOT NULL,
               status TEXT NOT NULL,
               dispatch_content TEXT NOT NULL,
               display_content TEXT NOT NULL,
               images_json TEXT,
               PRIMARY KEY(session_id,turn_intent_id)
             );
             CREATE TABLE events (
               id TEXT PRIMARY KEY,
               session_id TEXT NOT NULL DEFAULT 'session-root',
               result_json TEXT NOT NULL,
               meta_json TEXT
             );",
        )
        .expect("create Group action schemas");
        conn
    }

    fn identity(turn_intent_id: &str) -> GroupTurnIdentity {
        GroupTurnIdentity {
            run_id: "run-actions".to_string(),
            session_id: "session-root".to_string(),
            turn_intent_id: turn_intent_id.to_string(),
            source_kind: "group_root".to_string(),
        }
    }

    fn insert_root_turn(
        conn: &rusqlite::Connection,
        turn_intent_id: &str,
        status: &str,
        source_id: &str,
    ) {
        conn.execute(
            "INSERT INTO session_turn_intents (
               session_id,turn_intent_id,client_message_id,org_run_id,source,status,
               created_at,updated_at
             ) VALUES ('session-root',?1,?2,'run-actions','user_submit',?3,'now','now')",
            params![turn_intent_id, format!("client-{turn_intent_id}"), status],
        )
        .expect("insert Root intent");
        conn.execute(
            "INSERT INTO agent_org_runtime_turn_contexts (
               session_id,turn_intent_id,org_run_id,participant_id,source_kind,source_id
             ) VALUES ('session-root',?1,'run-actions','coordinator','group_root',?2)",
            params![turn_intent_id, source_id],
        )
        .expect("insert Root context");
    }

    fn intent_status(conn: &rusqlite::Connection, turn_intent_id: &str) -> String {
        conn.query_row(
            "SELECT status FROM session_turn_intents
             WHERE session_id='session-root' AND turn_intent_id=?1",
            [turn_intent_id],
            |row| row.get(0),
        )
        .expect("read exact Turn status")
    }

    #[test]
    fn queued_group_root_stop_is_exact_persisted_first_and_idempotent() {
        let mut conn = connection();
        insert_root_turn(&conn, "turn-stop", "queued", "event-stop");
        insert_root_turn(&conn, "turn-next", "queued", "event-next");

        assert_eq!(
            prepare_group_root_cancellation_with_connection(&mut conn, &identity("turn-stop"))
                .expect("cancel exact queued Turn"),
            ExactGroupCancellationState::QueuedCancelled
        );
        assert_eq!(intent_status(&conn, "turn-stop"), "cancelled");
        assert_eq!(
            intent_status(&conn, "turn-next"),
            "queued",
            "Stop must not alter the next Coordinator FIFO item"
        );
        assert_eq!(
            prepare_group_root_cancellation_with_connection(&mut conn, &identity("turn-stop"))
                .expect("replay terminal Stop"),
            ExactGroupCancellationState::AlreadyTerminal
        );

        conn.execute(
            "UPDATE session_turn_intents SET status='running'
             WHERE turn_intent_id='turn-next'",
            [],
        )
        .expect("start next Turn");
        assert_eq!(
            prepare_group_root_cancellation_with_connection(&mut conn, &identity("turn-next"))
                .expect("prepare exact running cancellation"),
            ExactGroupCancellationState::Started
        );
        assert_eq!(intent_status(&conn, "turn-next"), "running");
    }

    #[test]
    fn retry_envelope_preserves_exact_root_and_member_frozen_payloads() {
        let conn = connection();
        insert_root_turn(&conn, "turn-root-retry", "failed", "event-root-retry");
        conn.execute(
            "INSERT INTO events (id,result_json,meta_json)
             VALUES ('event-root-retry',
               json_object('messageId','client-turn-root-retry',
                           'turnIntentId','turn-root-retry',
                           'message',json_object('content','Root frozen body'),
                           'images',json_array('root-image')),
               json_object('displayText','Root visible body'))",
            [],
        )
        .expect("insert frozen Root source");
        let root = load_retry_envelope_with_connection(&conn, identity("turn-root-retry"))
            .expect("load Root retry envelope");
        assert_eq!(
            root.client_message_id.as_deref(),
            Some("client-turn-root-retry")
        );
        assert_eq!(root.content, "Root frozen body");
        assert_eq!(root.display_text, "Root visible body");
        assert_eq!(root.images, Some(vec!["root-image".to_string()]));
        assert!(!root.has_exact_reply);
        conn.execute(
            "INSERT INTO events (id,session_id,result_json,meta_json)
             VALUES ('event-root-reply','session-root',
               json_object('agent_org_group_root_reply',
                           json_object('source_event_id','event-root-retry')),
               NULL)",
            [],
        )
        .expect("insert exact Root reply marker");
        let answered_root = load_retry_envelope_with_connection(&conn, identity("turn-root-retry"))
            .expect("reload answered Root retry envelope");
        assert!(answered_root.has_exact_reply);

        conn.execute(
            "INSERT INTO session_turn_intents (
               session_id,turn_intent_id,client_message_id,org_run_id,source,status,
               created_at,updated_at
             ) VALUES ('session-member','turn-member-retry','turn-member-retry',
                       'run-actions','agent_org','cancelled','now','now')",
            [],
        )
        .expect("insert Member intent");
        conn.execute(
            "INSERT INTO agent_org_runtime_turn_contexts (
               session_id,turn_intent_id,org_run_id,participant_id,source_kind,source_id
             ) VALUES ('session-member','turn-member-retry','run-actions','reviewer',
                       'group_mention','42')",
            [],
        )
        .expect("insert Member context");
        conn.execute(
            "INSERT INTO agent_org_runtime_user_directed_deliveries (
               session_id,turn_intent_id,status,dispatch_content,display_content,images_json
             ) VALUES ('session-member','turn-member-retry','cancelled',
                       'Member frozen body','@Reviewer visible body','[\"member-image\"]')",
            [],
        )
        .expect("insert frozen Member delivery");
        let member = load_retry_envelope_with_connection(
            &conn,
            GroupTurnIdentity {
                run_id: "run-actions".to_string(),
                session_id: "session-member".to_string(),
                turn_intent_id: "turn-member-retry".to_string(),
                source_kind: "group_mention".to_string(),
            },
        )
        .expect("load Member retry envelope");
        assert_eq!(member.participant_id, "reviewer");
        assert_eq!(member.source_id, "42");
        assert_eq!(member.content, "Member frozen body");
        assert_eq!(member.display_text, "@Reviewer visible body");
        assert_eq!(member.images, Some(vec!["member-image".to_string()]));
        assert!(!member.has_exact_reply);
    }

    #[test]
    fn retry_requires_exact_reply_evidence_instead_of_generic_completion() {
        assert_eq!(
            retry_disposition("completed", false, false),
            Ok(RetryDisposition::Create),
            "a terminal Turn without its causal reply must remain retryable"
        );
        assert_eq!(
            retry_disposition("completed", true, false),
            Err("group_delivery_already_answered".to_string()),
            "an exact causal reply is the server-owned answered fact"
        );
        assert_eq!(
            retry_disposition("unknown", false, false),
            Err("group_retry_possible_duplicate_confirmation_required".to_string())
        );
        assert_eq!(
            retry_disposition("unknown", false, true),
            Ok(RetryDisposition::Create)
        );
        assert_eq!(
            retry_disposition("cancelled", true, false),
            Ok(RetryDisposition::Create),
            "a cancelled Turn may retain a streamed preamble without having answered"
        );
        assert_eq!(
            retry_disposition("failed", true, false),
            Ok(RetryDisposition::Create),
            "a failed Turn may retain partial output without having answered"
        );
        assert_eq!(
            retry_disposition("unknown", true, false),
            Err("group_retry_possible_duplicate_confirmation_required".to_string()),
            "unknown completion remains confirmation-gated even with partial output"
        );
    }

    #[test]
    fn failed_pre_admission_cleanup_removes_only_an_unowned_exact_source() {
        let mut conn = connection();
        conn.execute(
            "INSERT INTO events (id,session_id,result_json,meta_json)
             VALUES ('source-unowned','session-root',
               json_object('messageId','client-unowned',
                           'turnIntentId','turn-unowned'),NULL)",
            [],
        )
        .expect("insert unowned GroupRoot source");

        remove_unadmitted_group_root_source_event_with_connection(
            &mut conn,
            "session-root",
            "turn-unowned",
            "client-unowned",
            "source-unowned",
        )
        .expect("remove exact unowned source");
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM events WHERE id='source-unowned'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .expect("count cleaned source"),
            0
        );

        insert_root_turn(&conn, "turn-owned", "queued", "source-owned");
        conn.execute(
            "INSERT INTO events (id,session_id,result_json,meta_json)
             VALUES ('source-owned','session-root',
               json_object('messageId','client-turn-owned',
                           'turnIntentId','turn-owned'),NULL)",
            [],
        )
        .expect("insert admitted GroupRoot source");
        remove_unadmitted_group_root_source_event_with_connection(
            &mut conn,
            "session-root",
            "turn-owned",
            "client-turn-owned",
            "source-owned",
        )
        .expect("preserve admitted source");
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM events WHERE id='source-owned'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .expect("count admitted source"),
            1
        );

        conn.execute(
            "INSERT INTO events (id,session_id,result_json,meta_json)
             VALUES
               ('source-replied','session-root',
                json_object('messageId','client-replied',
                            'turnIntentId','turn-replied'),NULL),
               ('reply-replied','session-root',
                json_object('agent_org_group_root_reply',
                            json_object('source_event_id','source-replied')),NULL)",
            [],
        )
        .expect("insert replied source without a context");
        remove_unadmitted_group_root_source_event_with_connection(
            &mut conn,
            "session-root",
            "turn-replied",
            "client-replied",
            "source-replied",
        )
        .expect("preserve source with exact reply evidence");
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM events WHERE id='source-replied'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .expect("count replied source"),
            1
        );
    }

    #[test]
    fn exact_group_action_identity_rejects_a_cross_session_turn_id_collision() {
        let conn = connection();
        insert_root_turn(&conn, "unique-turn", "queued", "source-unique");
        let exact = resolve_group_turn_with_connection(&conn, "run-actions", "unique-turn")
            .expect("resolve one exact Group delivery");
        assert_eq!(exact.session_id, "session-root");

        conn.execute(
            "INSERT INTO agent_org_runtime_turn_contexts (
               session_id,turn_intent_id,org_run_id,participant_id,source_kind,source_id
             ) VALUES ('session-member','unique-turn','run-actions','reviewer',
                       'group_mention','42')",
            [],
        )
        .expect("insert colliding Member Group delivery");
        assert_eq!(
            resolve_group_turn_with_connection(&conn, "run-actions", "unique-turn")
                .expect_err("ambiguous Turn id must fail closed"),
            "group_delivery_ambiguous"
        );
    }
}
