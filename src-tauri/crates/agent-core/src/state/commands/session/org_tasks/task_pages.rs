//! Demand-driven Task history, detail, and annotation commands.
//!
//! These reads deliberately stay outside the frequently-polled Run View. They
//! use bounded keyset pages and never reconcile, wake, or mutate the run.

use std::collections::HashMap;

use serde::Serialize;

use crate::coordination::agent_org_tasks::{
    AgentOrgTaskStore, Task, TaskAnnotationPage, TaskPageBucket, TaskPageDirection, TaskStatus,
};
use crate::state::AgentAppState;

use super::context::{require_session_member_id, session_org_read_context};
use super::run_view::{tasks_for_context, AgentOrgTaskRuntime};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentOrgTaskPage {
    pub bucket: TaskPageBucket,
    pub status: Option<TaskStatus>,
    pub tasks: Vec<AgentOrgTaskRuntime>,
    pub has_more: bool,
    pub next_cursor: Option<String>,
    pub previous_cursor: Option<String>,
}

#[tauri::command]
pub async fn agent_org_session_task_page(
    state: tauri::State<'_, AgentAppState>,
    session_id: String,
    bucket: TaskPageBucket,
    status: Option<TaskStatus>,
    cursor: Option<String>,
    direction: Option<TaskPageDirection>,
    limit: Option<usize>,
) -> Result<AgentOrgTaskPage, String> {
    crate::coordination::agent_org_runs::require_agent_org_enabled()?;
    let read_context = session_org_read_context(&state, &session_id)
        .await?
        .ok_or_else(|| format!("Agent Org context not found for session {session_id}"))?;
    require_session_member_id(&read_context, &session_id)?;
    let context = read_context
        .context
        .ok_or_else(|| format!("Agent Org run context not found for session {session_id}"))?;
    tokio::task::spawn_blocking(move || {
        let conn = database::db::get_connection().map_err(|error| error.to_string())?;
        let tx = conn
            .unchecked_transaction()
            .map_err(|error| error.to_string())?;
        let page = AgentOrgTaskStore::list_task_page_with_connection(
            &tx,
            &context.run_id,
            bucket,
            status,
            cursor.as_deref(),
            direction.unwrap_or(TaskPageDirection::Forward),
            limit.unwrap_or(50),
        )?;
        let tasks = tasks_for_context(&context, page.tasks, &HashMap::new(), &HashMap::new());
        tx.commit().map_err(|error| error.to_string())?;
        Ok(AgentOrgTaskPage {
            bucket,
            status,
            tasks,
            has_more: page.has_more,
            next_cursor: page.next_cursor,
            previous_cursor: page.previous_cursor,
        })
    })
    .await
    .map_err(|error| format!("Agent Org Task page worker failed: {error}"))?
}

#[tauri::command]
pub async fn agent_org_session_task_detail(
    state: tauri::State<'_, AgentAppState>,
    session_id: String,
    task_id: String,
) -> Result<Task, String> {
    crate::coordination::agent_org_runs::require_agent_org_enabled()?;
    let read_context = session_org_read_context(&state, &session_id)
        .await?
        .ok_or_else(|| format!("Agent Org context not found for session {session_id}"))?;
    require_session_member_id(&read_context, &session_id)?;
    let run_id = read_context
        .context
        .ok_or_else(|| format!("Agent Org run context not found for session {session_id}"))?
        .run_id;
    tokio::task::spawn_blocking(move || {
        AgentOrgTaskStore::get(&run_id, &task_id)?
            .ok_or_else(|| format!("task_not_found: {task_id} in run {run_id}"))
    })
    .await
    .map_err(|error| format!("Agent Org Task detail worker failed: {error}"))?
}

#[tauri::command]
pub async fn agent_org_session_task_annotation_page(
    state: tauri::State<'_, AgentAppState>,
    session_id: String,
    task_id: String,
    cursor: Option<String>,
    limit: Option<usize>,
) -> Result<TaskAnnotationPage, String> {
    crate::coordination::agent_org_runs::require_agent_org_enabled()?;
    let read_context = session_org_read_context(&state, &session_id)
        .await?
        .ok_or_else(|| format!("Agent Org context not found for session {session_id}"))?;
    require_session_member_id(&read_context, &session_id)?;
    let run_id = read_context
        .context
        .ok_or_else(|| format!("Agent Org run context not found for session {session_id}"))?
        .run_id;
    tokio::task::spawn_blocking(move || {
        if AgentOrgTaskStore::get(&run_id, &task_id)?.is_none() {
            return Err(format!("task_not_found: {task_id} in run {run_id}"));
        }
        AgentOrgTaskStore::list_annotation_page(
            &run_id,
            &task_id,
            cursor.as_deref(),
            limit.unwrap_or(50),
        )
    })
    .await
    .map_err(|error| format!("Agent Org annotation page worker failed: {error}"))?
}
