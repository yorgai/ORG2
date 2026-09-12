//! Final-report retry command for a completed Agent Org run.
//!
//! A retry never edits the completion certificate or its evidence. It creates
//! the next immutable attempt only after the backend has revalidated the
//! failed attempt and evidence digest, then rings the existing Coordinator
//! dispatcher doorbell.

use crate::coordination::agent_org_final_summary::FinalSummaryReceipt;
use crate::coordination::agent_org_runs::COORDINATOR_MEMBER_ID;
use crate::state::AgentAppState;

use super::context::session_org_read_context;
use super::lifecycle::wake_agent_org_member;

#[tauri::command]
pub async fn agent_org_final_summary_retry(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AgentAppState>,
    session_id: String,
    certificate_id: String,
    failed_attempt: i64,
    request_id: String,
) -> Result<FinalSummaryReceipt, String> {
    crate::coordination::agent_org_runs::require_agent_org_enabled()?;
    let read_context = session_org_read_context(&state, &session_id)
        .await?
        .ok_or_else(|| format!("Session {session_id} is not part of an Agent Org run"))?;
    let context = read_context
        .context
        .ok_or_else(|| format!("Session {session_id} has no Agent Org context"))?;
    let run_id = context.run_id.clone();
    let retry_run_id = run_id.clone();
    let receipt = tokio::task::spawn_blocking(move || {
        crate::coordination::agent_org_final_summary::retry_failed(
            &retry_run_id,
            &certificate_id,
            failed_attempt,
            &request_id,
        )
    })
    .await
    .map_err(|error| format!("Agent Org final summary retry worker failed: {error}"))??;

    wake_agent_org_member(app_handle, COORDINATOR_MEMBER_ID, &run_id);
    crate::coordination::agent_org_run_events::notify_agent_org_run_changed(&run_id);
    Ok(receipt)
}
