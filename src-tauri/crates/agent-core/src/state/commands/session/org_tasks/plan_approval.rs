//! Agent Org plan-approval commands.
//!
//! When a run's plan-approval policy routes a plan revision to the user, these
//! commands fetch the revision detail and record the user's decision (approve
//! unchanged or request changes), then wake the affected members and
//! reconcile Team quiescence off the durable transaction.

use crate::coordination::agent_inbox::USER_SENDER_ID;
use crate::coordination::agent_org_plan_approvals::{
    AgentOrgPlanDecisionBy, AgentOrgPlanDecisionDelivery, AgentOrgPlanRevision,
    AgentOrgPlanRevisionStore,
};
use crate::coordination::agent_org_runs::AgentOrgRunStore;
use crate::state::AgentAppState;

use super::context::session_org_read_context;
use super::lifecycle::wake_agent_org_member;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentOrgPlanApprovalDecision {
    Approve,
    RequestChanges,
}

#[tauri::command]
pub async fn agent_org_plan_approval_detail(
    state: tauri::State<'_, AgentAppState>,
    session_id: String,
    approval_id: String,
    plan_revision_id: String,
) -> Result<AgentOrgPlanRevision, String> {
    crate::coordination::agent_org_runs::require_agent_org_enabled()?;
    let Some(read_context) = session_org_read_context(&state, &session_id).await? else {
        return Err(format!(
            "Session {session_id} is not part of an Agent Org run"
        ));
    };
    let context = read_context
        .context
        .ok_or_else(|| format!("Session {session_id} has no Agent Org context"))?;
    let lookup_approval_id = approval_id.clone();
    let lookup_revision_id = plan_revision_id.clone();
    let lookup_run_id = context.run_id.clone();
    let approval = tokio::task::spawn_blocking(move || {
        AgentOrgPlanRevisionStore::get_revision_for_run(
            &lookup_run_id,
            &lookup_approval_id,
            &lookup_revision_id,
        )
    })
    .await
    .map_err(|err| format!("Agent Org plan approval detail worker failed: {err}"))??
    .ok_or_else(|| {
        format!("Agent Org plan approval revision was not found: {approval_id}/{plan_revision_id}")
    })?;
    Ok(approval)
}

#[tauri::command]
// Tauri exposes command arguments as a flat invoke payload. Keeping these
// fields explicit preserves the stable frontend wire shape.
#[allow(clippy::too_many_arguments)]
pub async fn agent_org_plan_approval_respond(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AgentAppState>,
    session_id: String,
    approval_id: String,
    plan_revision_id: String,
    source_task_id: String,
    source_turn_intent_id: String,
    decision: AgentOrgPlanApprovalDecision,
    feedback: Option<String>,
) -> Result<AgentOrgPlanRevision, String> {
    crate::coordination::agent_org_runs::require_agent_org_enabled()?;
    let Some(read_context) = session_org_read_context(&state, &session_id).await? else {
        return Err(format!(
            "Session {session_id} is not part of an Agent Org run"
        ));
    };
    let context = read_context
        .context
        .ok_or_else(|| format!("Session {session_id} has no Agent Org context"))?;
    let feedback = match decision {
        AgentOrgPlanApprovalDecision::RequestChanges => Some(
            feedback
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| "request_changes requires non-empty feedback".to_string())?
                .to_string(),
        ),
        _ => None,
    };
    let run_id = context.run_id.clone();
    let blocking_run_id = run_id.clone();
    let blocking_approval_id = approval_id.clone();
    let blocking_revision_id = plan_revision_id.clone();
    let blocking_task_id = source_task_id.clone();
    let blocking_source_turn_id = source_turn_intent_id.clone();
    let blocking_decision_session_id = session_id.clone();
    let blocking_context = context.clone();

    // Approval decisions can touch SQLite and the plan artifact. Execute the
    // complete durable decision off Tokio's async executor; only dispatch
    // wake signals after the store transaction has committed.
    let (resolved, wake_member_ids, should_reconcile) = tokio::task::spawn_blocking(
        move || -> Result<(AgentOrgPlanRevision, Vec<String>, bool), String> {
            let approval =
                AgentOrgPlanRevisionStore::get(&blocking_approval_id)?.ok_or_else(|| {
                    format!("Agent Org plan approval {blocking_approval_id} was not found")
                })?;
            if approval.org_run_id != blocking_run_id {
                return Err("Agent Org plan approval does not belong to this run".to_string());
            }

            match decision {
                AgentOrgPlanApprovalDecision::Approve => {
                    let approved = AgentOrgPlanRevisionStore::approve(
                        &blocking_approval_id,
                        &blocking_revision_id,
                        &blocking_task_id,
                        &blocking_source_turn_id,
                        AgentOrgPlanDecisionBy::User,
                        &blocking_decision_session_id,
                        None,
                    )?;
                    Ok((approved.revision, approved.wake_member_ids, true))
                }
                AgentOrgPlanApprovalDecision::RequestChanges => {
                    let feedback = feedback.as_deref().ok_or_else(|| {
                        "request_changes feedback disappeared before commit".to_string()
                    })?;
                    let recipient_agent_id = blocking_context
                        .participant_agent_id(&approval.source_member_id)
                        .ok_or_else(|| {
                            format!(
                                "Agent Org plan source member {} is not in the run roster",
                                approval.source_member_id
                            )
                        })?;
                    let (changed, _) = AgentOrgPlanRevisionStore::request_changes(
                        &blocking_approval_id,
                        &blocking_revision_id,
                        &blocking_task_id,
                        &blocking_source_turn_id,
                        AgentOrgPlanDecisionBy::User,
                        feedback,
                        AgentOrgPlanDecisionDelivery {
                            recipient_agent_id,
                            sender_agent_id: USER_SENDER_ID.to_string(),
                            sender_member_id: None,
                        },
                    )?;
                    let source_member_id = changed.source_member_id.clone();
                    Ok((
                        changed,
                        vec![
                            source_member_id,
                            crate::coordination::agent_org_runs::COORDINATOR_MEMBER_ID.to_string(),
                        ],
                        false,
                    ))
                }
            }
        },
    )
    .await
    .map_err(|err| format!("Agent Org plan approval worker failed: {err}"))??;

    for member_id in wake_member_ids {
        wake_agent_org_member(app_handle.clone(), &member_id, &run_id);
    }
    if should_reconcile {
        let reconcile_run_id = run_id.clone();
        tokio::spawn(async move {
            match tokio::task::spawn_blocking(move || {
                let assessment = AgentOrgRunStore::assess_run_quiescence(&reconcile_run_id)?;
                let Some(generation) = assessment.facts.activation_generation else {
                    return Ok(false);
                };
                let Some(work_revision) = assessment
                    .facts
                    .progress
                    .as_ref()
                    .map(|progress| progress.work_revision)
                else {
                    return Ok(false);
                };
                AgentOrgRunStore::try_transition_working_to_idle(
                    &reconcile_run_id,
                    generation,
                    work_revision,
                )
            })
            .await
            {
                Ok(Ok(_)) => {}
                Ok(Err(err)) => tracing::warn!(
                    run_id,
                    error = %err,
                    "plan approval committed, but follow-up run reconciliation failed"
                ),
                Err(err) => tracing::warn!(
                    run_id,
                    error = %err,
                    "plan approval committed, but reconciliation worker failed"
                ),
            }
        });
    }
    Ok(resolved)
}
