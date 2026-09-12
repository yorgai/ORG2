//! Agent Org Group Chat Member-targeted message ingress.
//!
//! Group Chat is the user's durable message channel into a run. This module
//! owns the multi-target message command and the single-transaction
//! persistence that writes targeted Member Inbox sources. The unified Group
//! read model lives in `group_projection`; Coordinator messages live in
//! `group_actions` and reuse the canonical Root dispatcher.

use database::db::{get_connection, with_sessions_writer};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::coordination::agent_inbox::{
    AgentInboxRecord, AgentInboxStore, AgentMessage, InsertInboxParams, USER_SENDER_ID,
};
use crate::coordination::agent_org_runs::{
    AgentOrgRunContext, AgentOrgRunStatus, COORDINATOR_MEMBER_ID,
};
use crate::coordination::agent_org_turn_contexts::TURN_CONTEXT_INVARIANT_PREFIX;
use crate::coordination::agent_org_user_directed_work::{
    self, NewUserDirectedDelivery, UserDirectedDeliveryStatus, UserDirectedSourceKind,
    DEFAULT_MAX_GROUP_TARGETS, DEFAULT_MAX_PENDING_PER_MEMBER,
};
use crate::foundation::session_bridge::TurnIntentBridgeSource;
use crate::state::commands::session::identity::IdentityOverrides;
use crate::state::AgentAppState;

use super::run_view::{agent_org_session_run_view_impl, enrich_inbox_row, AgentOrgInboxRuntimeRow};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentOrgGroupDeliveryInput {
    pub target_member_id: String,
    pub turn_intent_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentOrgGroupDeliveryOutcome {
    Accepted,
    Existing,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentOrgGroupDeliveryResponse {
    pub target_member_id: String,
    pub target_member_name: String,
    pub turn_intent_id: String,
    pub source_inbox_id: i64,
    pub member_dispatch_sequence: i64,
    pub outcome: AgentOrgGroupDeliveryOutcome,
    pub inbox_row: AgentOrgInboxRuntimeRow,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentOrgGroupChatMessageResponse {
    pub deliveries: Vec<AgentOrgGroupDeliveryResponse>,
}

#[cfg(debug_assertions)]
static GROUP_DELIVERY_FAULT: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

#[cfg(debug_assertions)]
const GROUP_DELIVERY_FAULT_COMMIT_BEFORE_KICK: u8 = 1;
#[cfg(debug_assertions)]
const GROUP_DELIVERY_FAULT_RESPONSE_LOSS_AFTER_KICK: u8 = 2;

/// Arm one isolated BuildFast/debug fault. This is deliberately absent from
/// release builds and does not perform a user action; the packaged UI must
/// still submit or retry the real Group command that consumes the fault.
#[cfg(debug_assertions)]
pub fn arm_next_group_delivery_fault(mode: &str) -> Result<(), String> {
    let value = match mode {
        "commit_before_kick" => GROUP_DELIVERY_FAULT_COMMIT_BEFORE_KICK,
        "response_loss_after_kick" => GROUP_DELIVERY_FAULT_RESPONSE_LOSS_AFTER_KICK,
        "clear" => 0,
        _ => {
            return Err(
                "unknown Group delivery fault; expected commit_before_kick, response_loss_after_kick, or clear"
                    .to_string(),
            );
        }
    };
    GROUP_DELIVERY_FAULT.store(value, std::sync::atomic::Ordering::Release);
    Ok(())
}

#[cfg(debug_assertions)]
fn take_group_delivery_fault(expected: u8) -> bool {
    GROUP_DELIVERY_FAULT
        .compare_exchange(
            expected,
            0,
            std::sync::atomic::Ordering::AcqRel,
            std::sync::atomic::Ordering::Acquire,
        )
        .is_ok()
}

#[tauri::command]
pub async fn agent_org_send_group_chat_message(
    state: tauri::State<'_, AgentAppState>,
    session_id: String,
    deliveries: Vec<AgentOrgGroupDeliveryInput>,
    content: String,
    display_text: Option<String>,
    images: Option<Vec<String>>,
) -> Result<AgentOrgGroupChatMessageResponse, String> {
    agent_org_send_group_chat_message_impl_with_display(
        &state,
        session_id,
        deliveries,
        content,
        display_text,
        images,
    )
    .await
}

pub async fn agent_org_send_group_chat_message_impl(
    state: &AgentAppState,
    session_id: String,
    deliveries: Vec<AgentOrgGroupDeliveryInput>,
    content: String,
) -> Result<AgentOrgGroupChatMessageResponse, String> {
    agent_org_send_group_chat_message_impl_with_display(
        state, session_id, deliveries, content, None, None,
    )
    .await
}

pub(super) async fn agent_org_send_group_chat_message_impl_with_display(
    state: &AgentAppState,
    session_id: String,
    deliveries: Vec<AgentOrgGroupDeliveryInput>,
    content: String,
    display_text: Option<String>,
    images: Option<Vec<String>>,
) -> Result<AgentOrgGroupChatMessageResponse, String> {
    crate::coordination::agent_org_runs::require_agent_org_enabled()?;
    let content = content.trim();
    if content.is_empty() {
        return Err("Agent Org group chat message content is required".to_string());
    }
    validate_group_delivery_inputs(&deliveries)?;

    let view = agent_org_session_run_view_impl(state, &session_id)
        .await?
        .ok_or_else(|| format!("Session {session_id} is not part of an Agent Org run"))?;
    let durable_context = view.context.clone();
    let durable_content = content.to_string();
    let durable_display_text = display_text
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    if let Some(display_text) = durable_display_text.as_deref() {
        crate::coordination::agent_org_payload_limits::validate_required_text(
            "display_text",
            display_text,
            crate::coordination::agent_org_payload_limits::PLAIN_TEXT_MAX_CHARS,
            crate::coordination::agent_org_payload_limits::PLAIN_TEXT_MAX_BYTES,
        )?;
    }
    let durable_images = images.filter(|items| !items.is_empty());
    let persisted = tokio::task::spawn_blocking(move || {
        persist_group_chat_deliveries(
            &durable_context,
            &deliveries,
            &durable_content,
            durable_display_text.as_deref(),
            durable_images.as_deref(),
        )
    })
    .await
    .map_err(|err| format!("Agent Org group message worker failed: {err}"))??;

    #[cfg(debug_assertions)]
    if take_group_delivery_fault(GROUP_DELIVERY_FAULT_COMMIT_BEFORE_KICK) {
        return Err(
            "group_delivery_commit_before_kick_fault: durable deliveries are pending; quit and reopen the packaged app to exercise startup recovery"
                .to_string(),
        );
    }

    let mut responses = Vec::with_capacity(persisted.len());
    let mut kick_errors = Vec::new();
    for delivery in persisted {
        let target_name = view
            .members
            .iter()
            .find(|member| member.member_id == delivery.target_member_id)
            .map(|member| member.name.clone())
            .unwrap_or_else(|| delivery.target_member_id.clone());
        if delivery.status == UserDirectedDeliveryStatus::Pending {
            if let Err(error) =
                dispatch_group_delivery(state, &view.context.run_id, &delivery).await
            {
                kick_errors.push(format!("{}: {error}", delivery.target_member_id));
            }
        }
        responses.push(AgentOrgGroupDeliveryResponse {
            target_member_id: delivery.target_member_id,
            target_member_name: target_name,
            turn_intent_id: delivery.turn_intent_id,
            source_inbox_id: delivery.inbox_row.id,
            member_dispatch_sequence: delivery.member_dispatch_sequence,
            outcome: delivery.outcome,
            inbox_row: enrich_inbox_row(&view.context, delivery.inbox_row),
        });
    }
    if !kick_errors.is_empty() {
        return Err(format!(
            "group_delivery_kick_failed: durable deliveries are pending; retry the same envelope ({})",
            kick_errors.join(", ")
        ));
    }
    #[cfg(debug_assertions)]
    if take_group_delivery_fault(GROUP_DELIVERY_FAULT_RESPONSE_LOSS_AFTER_KICK) {
        return Err(
            "group_delivery_response_loss_after_kick_fault: the response was intentionally dropped after durable dispatch; retry the restored composer snapshot"
                .to_string(),
        );
    }
    Ok(AgentOrgGroupChatMessageResponse {
        deliveries: responses,
    })
}

#[derive(Debug)]
pub(super) struct PersistedGroupDelivery {
    pub(super) target_member_id: String,
    pub(super) session_id: String,
    pub(super) turn_intent_id: String,
    pub(super) member_dispatch_sequence: i64,
    pub(super) status: UserDirectedDeliveryStatus,
    pub(super) outcome: AgentOrgGroupDeliveryOutcome,
    pub(super) content: String,
    pub(super) display_text: String,
    pub(super) images: Option<Vec<String>>,
    pub(super) inbox_row: AgentInboxRecord,
}

fn validate_group_delivery_inputs(deliveries: &[AgentOrgGroupDeliveryInput]) -> Result<(), String> {
    if deliveries.is_empty() {
        return Err("group_target_required: select at least one Member".to_string());
    }
    if deliveries.len() as i64 > DEFAULT_MAX_GROUP_TARGETS {
        return Err(format!(
            "group_target_limit_exceeded: at most {DEFAULT_MAX_GROUP_TARGETS} Members may be selected"
        ));
    }
    let mut members = std::collections::HashSet::with_capacity(deliveries.len());
    let mut turns = std::collections::HashSet::with_capacity(deliveries.len());
    for delivery in deliveries {
        let member_id = delivery.target_member_id.trim();
        let turn_id = delivery.turn_intent_id.trim();
        if member_id.is_empty() || turn_id.is_empty() {
            return Err("group_delivery_invalid: target Member and Turn id are required".into());
        }
        if member_id == COORDINATOR_MEMBER_ID {
            return Err(
                "group_mixed_recipient_kind: Coordinator and Member sends must be submitted separately"
                    .into(),
            );
        }
        if !members.insert(member_id) {
            return Err(format!(
                "group_duplicate_target: Member {member_id} appears more than once"
            ));
        }
        if !turns.insert(turn_id) {
            return Err(format!(
                "group_duplicate_turn_id: Turn id {turn_id} was reused across targets"
            ));
        }
    }
    Ok(())
}

async fn dispatch_group_delivery(
    state: &AgentAppState,
    run_id: &str,
    delivery: &PersistedGroupDelivery,
) -> Result<(), String> {
    super::super::message::send_message_impl(
        state,
        delivery.session_id.clone(),
        delivery.content.clone(),
        Some(delivery.display_text.clone()),
        IdentityOverrides::default(),
        None,
        delivery.images.clone(),
        None,
        false,
        None,
        None,
        false,
        Some(delivery.turn_intent_id.clone()),
        Some(delivery.turn_intent_id.clone()),
        None,
        None,
        Some(run_id.to_string()),
        TurnIntentBridgeSource::AgentOrg,
    )
    .await?;
    Ok(())
}

pub(super) fn persist_group_chat_deliveries(
    context: &AgentOrgRunContext,
    deliveries: &[AgentOrgGroupDeliveryInput],
    content: &str,
    display_text: Option<&str>,
    images: Option<&[String]>,
) -> Result<Vec<PersistedGroupDelivery>, String> {
    validate_group_delivery_inputs(deliveries)?;
    let effective_display = display_text.unwrap_or(content);
    with_sessions_writer(|| -> Result<Vec<PersistedGroupDelivery>, String> {
        let mut conn = get_connection().map_err(|error| error.to_string())?;
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|error| error.to_string())?;
        validate_group_run_status(&tx, &context.run_id)?;

        let mut existing = Vec::with_capacity(deliveries.len());
        for input in deliveries {
            existing.push(load_existing_group_delivery(
                &tx,
                &context.run_id,
                input,
                content,
                effective_display,
                images,
            )?);
        }
        let existing_count = existing.iter().filter(|item| item.is_some()).count();
        if existing_count != 0 && existing_count != deliveries.len() {
            return Err(
                "group_idempotency_mixed: request contains both new and existing Turn ids"
                    .to_string(),
            );
        }
        if existing_count == deliveries.len() {
            let rows = existing
                .into_iter()
                .map(|item| item.expect("count proved every delivery exists"))
                .collect();
            tx.commit().map_err(|error| error.to_string())?;
            return Ok(rows);
        }

        let mut targets = Vec::with_capacity(deliveries.len());
        for input in deliveries {
            let target = resolve_group_target(&tx, &context.run_id, &input.target_member_id)?;
            let turn_id_conflict: bool = tx
                .query_row(
                    "SELECT
                         EXISTS(
                             SELECT 1 FROM agent_org_runtime_turn_contexts context
                             WHERE context.org_run_id=?1 AND context.turn_intent_id=?2
                         )
                         OR EXISTS(
                             SELECT 1 FROM session_turn_intents intent
                             WHERE intent.session_id=?3 AND intent.turn_intent_id=?2
                         )",
                    params![&context.run_id, &input.turn_intent_id, &target.0],
                    |row| row.get(0),
                )
                .map_err(|error| error.to_string())?;
            if turn_id_conflict {
                return Err(format!(
                    "group_idempotency_conflict: Turn {} already belongs to another durable envelope",
                    input.turn_intent_id
                ));
            }
            targets.push(target);
            let pending: i64 = tx
                .query_row(
                    "SELECT COUNT(*)
                     FROM agent_org_runtime_user_directed_deliveries
                     WHERE org_run_id=?1 AND dispatch_member_id=?2
                       AND status IN ('pending','started')",
                    params![&context.run_id, &input.target_member_id],
                    |row| row.get(0),
                )
                .map_err(|error| error.to_string())?;
            if pending >= DEFAULT_MAX_PENDING_PER_MEMBER {
                return Err(format!(
                    "user_directed_queue_full: Member {} already has {pending} pending/started deliveries (cap {DEFAULT_MAX_PENDING_PER_MEMBER})",
                    input.target_member_id
                ));
            }
        }

        let mut persisted = Vec::with_capacity(deliveries.len());
        for (input, (target_session_id, target_agent_id)) in deliveries.iter().zip(targets) {
            let inbox_row = AgentInboxStore::insert_in_tx_without_formal_trigger(
                &tx,
                InsertInboxParams {
                    recipient_agent_id: target_agent_id,
                    recipient_member_id: Some(input.target_member_id.clone()),
                    sender_agent_id: USER_SENDER_ID.to_string(),
                    sender_member_id: None,
                    org_run_id: Some(context.run_id.clone()),
                    message: AgentMessage::Plain {
                        summary: "User group mention".to_string(),
                        text: content.to_string(),
                    },
                },
            )?;
            tx.execute(
                "UPDATE agent_org_runtime_inbox
                 SET delivery_class='user_directed',display_text=?2
                 WHERE id=?1 AND delivery_class='formal_work'",
                params![inbox_row.id, effective_display],
            )
            .map_err(|error| error.to_string())?;

            let admission =
                crate::coordination::agent_org_turn_contexts::AgentOrgTurnAdmission::group_mention(
                    &context.run_id,
                    &target_session_id,
                    &input.turn_intent_id,
                    Some(input.turn_intent_id.clone()),
                    &input.target_member_id,
                    inbox_row.id,
                );
            let turn_context =
                crate::coordination::agent_org_turn_contexts::accept_with_connection(
                    &tx, &admission,
                )?;
            let sequence = turn_context.member_dispatch_sequence.ok_or_else(|| {
                format!("{TURN_CONTEXT_INVARIANT_PREFIX} Group mention has no Member FIFO sequence")
            })?;
            let root = NewUserDirectedDelivery {
                org_run_id: &context.run_id,
                session_id: &target_session_id,
                turn_intent_id: &input.turn_intent_id,
                root_authority_turn_id: &input.turn_intent_id,
                parent_delivery_id: None,
                parent_inbox_id: None,
                source_kind: UserDirectedSourceKind::GroupMention,
                source_event_id: None,
                source_inbox_id: Some(inbox_row.id),
                dispatch_member_id: &input.target_member_id,
                member_dispatch_sequence: sequence,
                depth: 0,
                delivery_ordinal: 1,
                dispatch_content: content,
                display_content: effective_display,
                images,
            };
            let (receipt, duplicate) =
                agent_org_user_directed_work::insert_root_delivery_with_connection(&tx, &root)?;
            if duplicate {
                return Err(
                    "group_idempotency_mixed: Turn appeared during the atomic insert".to_string(),
                );
            }
            persisted.push(PersistedGroupDelivery {
                target_member_id: input.target_member_id.clone(),
                session_id: target_session_id,
                turn_intent_id: input.turn_intent_id.clone(),
                member_dispatch_sequence: sequence,
                status: receipt.status,
                outcome: AgentOrgGroupDeliveryOutcome::Accepted,
                content: content.to_string(),
                display_text: effective_display.to_string(),
                images: images.map(ToOwned::to_owned),
                inbox_row,
            });
        }
        tx.commit().map_err(|error| error.to_string())?;
        Ok(persisted)
    })
}

fn validate_group_run_status(conn: &rusqlite::Connection, run_id: &str) -> Result<(), String> {
    let status: Option<String> = conn
        .query_row(
            "SELECT status FROM agent_org_runtime_runs WHERE id=?1",
            [run_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    match status.as_deref().and_then(AgentOrgRunStatus::parse) {
        Some(AgentOrgRunStatus::Running | AgentOrgRunStatus::Idle | AgentOrgRunStatus::Paused) => {
            Ok(())
        }
        Some(AgentOrgRunStatus::Starting) => Err(format!(
            "team_not_ready: Agent Org run {run_id} is still materializing"
        )),
        Some(AgentOrgRunStatus::Failed) => Err(format!(
            "team_unavailable: Agent Org run {run_id} has failed"
        )),
        Some(AgentOrgRunStatus::Archived) => Err(format!(
            "team_archived: Agent Org run {run_id} is read-only"
        )),
        None => Err(format!(
            "team_unavailable: Agent Org run {run_id} does not exist"
        )),
    }
}

fn resolve_group_target(
    conn: &rusqlite::Connection,
    run_id: &str,
    member_id: &str,
) -> Result<(String, String), String> {
    conn.query_row(
        "SELECT materialization.session_id,materialization.agent_id
         FROM agent_org_runtime_member_materializations materialization
         JOIN agent_sessions session ON session.session_id=materialization.session_id
         WHERE materialization.org_run_id=?1
           AND materialization.member_id=?2
           AND materialization.status='succeeded'
           AND session.org_member_id=?2
           AND session.agent_definition_id=materialization.agent_id
           AND session.status<>'archived'
           AND materialization.generation=(
               SELECT MAX(latest.generation)
               FROM agent_org_runtime_member_materializations latest
               WHERE latest.org_run_id=?1 AND latest.member_id=?2
                 AND latest.status='succeeded'
           )
         LIMIT 1",
        params![run_id, member_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .optional()
    .map_err(|error| error.to_string())?
    .ok_or_else(|| format!("group_target_unavailable: canonical Member {member_id} is unavailable"))
}

fn load_existing_group_delivery(
    conn: &rusqlite::Connection,
    run_id: &str,
    input: &AgentOrgGroupDeliveryInput,
    content: &str,
    display_text: &str,
    images: Option<&[String]>,
) -> Result<Option<PersistedGroupDelivery>, String> {
    type ExistingRow = (
        i64,
        String,
        String,
        i64,
        String,
        String,
        String,
        String,
        String,
    );
    let row: Option<ExistingRow> = conn
        .query_row(
            "SELECT delivery.source_inbox_id,delivery.session_id,
                    delivery.dispatch_member_id,delivery.member_dispatch_sequence,
                    delivery.status,delivery.dispatch_content,delivery.display_content,
                    delivery.images_json,delivery.source_kind
             FROM agent_org_runtime_user_directed_deliveries delivery
             WHERE delivery.org_run_id=?1 AND delivery.turn_intent_id=?2",
            params![run_id, &input.turn_intent_id],
            |row| {
                Ok((
                    row.get::<_, Option<i64>>(0)?
                        .ok_or(rusqlite::Error::InvalidQuery)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                    row.get(8)?,
                ))
            },
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let Some((
        inbox_id,
        session_id,
        member_id,
        sequence,
        status_raw,
        stored_content,
        stored_display,
        images_json,
        source_kind,
    )) = row
    else {
        return Ok(None);
    };
    let stored_images: Vec<String> =
        serde_json::from_str(&images_json).map_err(|error| error.to_string())?;
    if member_id != input.target_member_id
        || source_kind != UserDirectedSourceKind::GroupMention.as_str()
        || stored_content != content
        || stored_display != display_text
        || stored_images.as_slice() != images.unwrap_or(&[])
    {
        return Err(format!(
            "group_idempotency_conflict: Turn {} was reused with a different target or body",
            input.turn_intent_id
        ));
    }
    let status = match status_raw.as_str() {
        "pending" => UserDirectedDeliveryStatus::Pending,
        "started" => UserDirectedDeliveryStatus::Started,
        "completed" => UserDirectedDeliveryStatus::Completed,
        "failed" => UserDirectedDeliveryStatus::Failed,
        "cancelled" => UserDirectedDeliveryStatus::Cancelled,
        "abandoned" => UserDirectedDeliveryStatus::Abandoned,
        "unknown" => UserDirectedDeliveryStatus::Unknown,
        _ => return Err("group_idempotency_conflict: stored status is invalid".into()),
    };
    let inbox_row = load_inbox_record(conn, run_id, inbox_id)?;
    Ok(Some(PersistedGroupDelivery {
        target_member_id: member_id,
        session_id,
        turn_intent_id: input.turn_intent_id.clone(),
        member_dispatch_sequence: sequence,
        status,
        outcome: AgentOrgGroupDeliveryOutcome::Existing,
        content: content.to_string(),
        display_text: display_text.to_string(),
        images: images.map(ToOwned::to_owned),
        inbox_row,
    }))
}

fn load_inbox_record(
    conn: &rusqlite::Connection,
    run_id: &str,
    inbox_id: i64,
) -> Result<AgentInboxRecord, String> {
    conn.query_row(
        "SELECT id,recipient_agent_id,recipient_member_id,sender_agent_id,
                sender_member_id,org_run_id,payload_kind,payload_json,request_id,
                created_at,read_at
         FROM agent_org_runtime_inbox
         WHERE id=?1 AND org_run_id=?2 AND delivery_class='user_directed'",
        params![inbox_id, run_id],
        |row| {
            Ok(AgentInboxRecord {
                id: row.get(0)?,
                recipient_agent_id: row.get(1)?,
                recipient_member_id: row.get(2)?,
                sender_agent_id: row.get(3)?,
                sender_member_id: row.get(4)?,
                org_run_id: row.get(5)?,
                payload_kind: row.get(6)?,
                payload_json: row.get(7)?,
                request_id: row.get(8)?,
                created_at: row.get(9)?,
                read_at: row.get(10)?,
            })
        },
    )
    .optional()
    .map_err(|error| error.to_string())?
    .ok_or_else(|| format!("group_idempotency_conflict: Inbox source {inbox_id} is missing"))
}
