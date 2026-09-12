//! Direct Member intervention inspection and explicit Return to Work.
//!
//! Direct work itself enters through `agent_send_message`; this module does
//! not own a second sender or a synthetic enter command.

use serde::Serialize;

use crate::coordination::agent_inbox::AgentInboxStore;
use crate::coordination::agent_member_interventions::{
    AgentMemberInterventionRecord, AgentMemberInterventionStore, ReturnToWorkOutcome,
    ReturnToWorkResult,
};
use crate::core::tools::impls::orchestration::inbox_wake::AppHandleInboxWakeHook;
use crate::foundation::session_bridge::TurnIntentBridgeSource;
use crate::state::commands::session::identity::IdentityOverrides;
use crate::state::AgentAppState;
use crate::tools::impls::orchestration::org_send_message::InboxWakeHook;

use super::context::{require_session_member_id, session_org_read_context};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentOrgSessionInterventionState {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intervention: Option<AgentMemberInterventionRecord>,
}

#[tauri::command]
pub async fn agent_org_session_intervention_state(
    state: tauri::State<'_, AgentAppState>,
    session_id: String,
) -> Result<AgentOrgSessionInterventionState, String> {
    crate::coordination::agent_org_runs::require_agent_org_enabled()?;
    let Some(read_context) = session_org_read_context(&state, &session_id).await? else {
        return Ok(AgentOrgSessionInterventionState { intervention: None });
    };
    let Some(ref context) = read_context.context else {
        return Ok(AgentOrgSessionInterventionState { intervention: None });
    };
    let member_id = require_session_member_id(&read_context, &session_id)?;
    Ok(AgentOrgSessionInterventionState {
        intervention: AgentMemberInterventionStore::active_for_member(&context.run_id, &member_id)?,
    })
}

#[tauri::command]
pub async fn agent_org_session_return_to_work(
    state: tauri::State<'_, AgentAppState>,
    session_id: String,
    intervention_receipt_id: String,
    request_id: String,
) -> Result<ReturnToWorkResult, String> {
    agent_org_session_return_to_work_impl(&state, session_id, intervention_receipt_id, request_id)
        .await
}

pub async fn agent_org_session_return_to_work_impl(
    state: &AgentAppState,
    session_id: String,
    intervention_receipt_id: String,
    request_id: String,
) -> Result<ReturnToWorkResult, String> {
    crate::coordination::agent_org_runs::require_agent_org_enabled()?;
    let read_context = session_org_read_context(state, &session_id)
        .await?
        .ok_or_else(|| format!("Session {session_id} is not part of an Agent Org run"))?;
    let member_id = require_session_member_id(&read_context, &session_id)?;
    let receipt = AgentMemberInterventionStore::get_by_receipt(&intervention_receipt_id)?
        .ok_or_else(|| format!("intervention_receipt_not_found: {intervention_receipt_id}"))?;
    if receipt.member_id != member_id {
        return Err("return_request_invalid: receipt belongs to another Member".to_string());
    }

    let return_session_id = session_id.clone();
    let return_receipt_id = intervention_receipt_id.clone();
    let result = tokio::task::spawn_blocking(move || {
        AgentMemberInterventionStore::return_to_work(
            &return_session_id,
            &return_receipt_id,
            &request_id,
        )
    })
    .await
    .map_err(|error| format!("Return to Work transaction worker failed: {error}"))??;

    if let Some(turn_intent_id) = result.continuation_turn_intent_id.as_deref() {
        dispatch_return_continuation(state, &receipt, turn_intent_id).await?;
    } else {
        wake_pending_formal_work_after_return(state, &receipt, &result).await;
    }
    Ok(result)
}

pub(super) fn pending_formal_wake_target(
    receipt: &AgentMemberInterventionRecord,
    result: &ReturnToWorkResult,
) -> Result<Option<(String, String)>, String> {
    if result.outcome == ReturnToWorkOutcome::AlreadyApplied
        || result.continuation_turn_intent_id.is_some()
        || !AgentInboxStore::has_unread_for_member(&receipt.member_id, &receipt.org_run_id)?
    {
        return Ok(None);
    }
    Ok(Some((
        receipt.member_id.clone(),
        receipt.org_run_id.clone(),
    )))
}

async fn wake_pending_formal_work_after_return(
    state: &AgentAppState,
    receipt: &AgentMemberInterventionRecord,
    result: &ReturnToWorkResult,
) {
    let receipt = receipt.clone();
    let result = result.clone();
    let target = match tokio::task::spawn_blocking(move || {
        pending_formal_wake_target(&receipt, &result)
    })
    .await
    {
        Ok(Ok(target)) => target,
        Ok(Err(error)) => {
            tracing::warn!(error = %error, "Return committed, but the pending formal Inbox probe failed");
            return;
        }
        Err(error) => {
            tracing::warn!(error = %error, "Return committed, but the pending formal Inbox probe worker failed");
            return;
        }
    };
    let (Some(app_handle), Some((member_id, run_id))) = (state.app_handle.clone(), target) else {
        return;
    };
    AppHandleInboxWakeHook::new(app_handle).wake_member(&member_id, &run_id);
}

pub(crate) async fn dispatch_return_continuation(
    state: &AgentAppState,
    receipt: &AgentMemberInterventionRecord,
    turn_intent_id: &str,
) -> Result<(), String> {
    let session_id = receipt.session_id.clone();
    let turn_intent_id = turn_intent_id.to_string();
    if !AgentMemberInterventionStore::continuation_is_dispatchable(&session_id, &turn_intent_id)? {
        return Ok(());
    }
    let dispatch = super::super::message::send_message_impl(
        state,
        session_id,
        String::new(),
        None,
        IdentityOverrides::default(),
        None,
        None,
        None,
        true,
        None,
        None,
        false,
        Some(format!("udw-return:{}", receipt.intervention_receipt_id)),
        Some(turn_intent_id.clone()),
        None,
        None,
        Some(receipt.org_run_id.clone()),
        TurnIntentBridgeSource::Resume,
    )
    .await;
    if let Err(error) = dispatch {
        if error.starts_with("Failed to enqueue message:") {
            let requeue_session_id = receipt.session_id.clone();
            let requeue_turn_intent_id = turn_intent_id.to_string();
            tokio::task::spawn_blocking(move || {
                AgentMemberInterventionStore::requeue_return_continuation_after_enqueue_failure(
                    &requeue_session_id,
                    &requeue_turn_intent_id,
                )
            })
            .await
            .map_err(|join_error| {
                format!("{error}; Return continuation requeue worker failed: {join_error}")
            })??;
        }
        return Err(error);
    }
    Ok(())
}
