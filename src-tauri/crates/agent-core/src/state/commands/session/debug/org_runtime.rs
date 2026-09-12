//! Debug-only Tauri commands for Agent Org runtime wiring.
//!
//! These commands let E2E specs assert the live Agent Org execution path:
//! org-run context capture, org-only tool registration, typed inbox delivery,
//! and task persistence. They intentionally expose only the Agent Org
//! orchestration tools, not arbitrary tool execution.

use std::sync::Arc;

use rusqlite::{params, OptionalExtension};
use serde::Serialize;
use serde_json::Value;

use crate::coordination::agent_inbox::{
    AgentInboxRecord, AgentInboxStore, AgentMessage, InsertInboxParams, MemberIdleReason,
    SYSTEM_SENDER_ID,
};
use crate::coordination::agent_org_runs::{
    AgentOrgRunContext, AgentOrgRunStore, COORDINATOR_MEMBER_ID,
};
use crate::coordination::agent_org_tasks::{AgentOrgTaskStore, Task};
use crate::definitions::orgs::AgentOrgsStore;
use crate::session::persistence;
use crate::state::AgentAppState;
use crate::tools::impls::orchestration::agent_org::tasks::{
    OrgRunCompleteTool, TaskCreateTool, TaskGetTool, TaskGraphCreateTool, TaskListTool,
    TaskToolsContext, TaskUpdateTool,
};
use crate::tools::impls::orchestration::org_send_message::{
    NoopInboxWakeHook, NoopSelfAbortHook, OrgSendMessageTool,
};
use crate::tools::names;
use crate::tools::traits::{Tool, ToolExecuteResult};

const DETACHED_ORG_RUNTIME_TOOL_NAMES: &[&str] = &[
    names::ORG_SEND_MESSAGE,
    names::TASK_CREATE,
    names::TASK_GRAPH_CREATE,
    names::TASK_UPDATE,
    names::TASK_LIST,
    names::TASK_GET,
    names::ORG_RUN_COMPLETE,
];

const SESSION_ORG_RUNTIME_TOOL_NAMES: &[&str] = &[
    names::ORG_SEND_MESSAGE,
    names::TASK_CREATE,
    names::TASK_GRAPH_CREATE,
    names::TASK_UPDATE,
    names::TASK_LIST,
    names::TASK_GET,
    names::ORG_RUN_COMPLETE,
    names::CREATE_PLAN,
];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DebugOrgToolResult {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<ToolExecuteResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl DebugOrgToolResult {
    fn from_tool_result(result: Result<ToolExecuteResult, String>) -> Self {
        match result {
            Ok(result) => Self {
                ok: true,
                result: Some(result),
                error: None,
            },
            Err(error) => Self {
                ok: false,
                result: None,
                error: Some(error),
            },
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionOrgRuntimeSnapshot {
    pub session_id: String,
    pub agent_id: String,
    pub member_id: Option<String>,
    pub is_coordinator: bool,
    pub is_org_member: bool,
    pub org_context: Option<AgentOrgRunContext>,
    pub registered_org_tool_names: Vec<String>,
    pub requested_exec_mode: Option<String>,
    pub has_plan_slot: bool,
    pub has_pre_plan_mode: bool,
}

/// Test configuration only: mutation tools still pass through their real
/// provider -> dispatcher -> CallContext -> Store path when this availability
/// gate is enabled. The callee rejects ordinary non-WebDriver binaries.
#[tauri::command]
pub async fn debug_agent_org_enable() -> Result<(), String> {
    crate::coordination::agent_org_runs::enable_agent_org_for_webdriver_test()
}

#[tauri::command]
pub async fn debug_session_org_runtime_snapshot(
    state: tauri::State<'_, AgentAppState>,
    session_id: String,
) -> Result<SessionOrgRuntimeSnapshot, String> {
    let session = state
        .get_session(&session_id)
        .await
        .ok_or_else(|| format!("session not found: {session_id}"))?;

    let runtime = session
        .get_runtime()
        .await
        .ok_or_else(|| format!("session runtime not initialized: {session_id}"))?;

    let agent_id = session.definition.id.clone();
    let org_context = runtime.agent_org_context.clone();
    let member_id = persistence::get_session(&session_id)
        .map_err(|err| err.to_string())?
        .and_then(|record| record.org_member_id);
    let is_coordinator =
        member_id.as_deref() == Some(crate::coordination::agent_org_runs::COORDINATOR_MEMBER_ID);
    let is_org_member = org_context.is_some() && member_id.is_some() && !is_coordinator;

    let mut registered_org_tool_names: Vec<String> = SESSION_ORG_RUNTIME_TOOL_NAMES
        .iter()
        .copied()
        .filter(|name| runtime.tool_registry.has(name))
        .map(str::to_string)
        .collect();
    registered_org_tool_names.sort();

    let requested_exec_mode = session
        .requested_exec_mode_cache
        .peek(&session.id)
        .map(|mode| mode.as_str().to_string());
    let has_plan_slot = session.plan_slot_cache.get(&session.id).is_some();
    let has_pre_plan_mode = session.pre_plan_mode_cache.get(&session.id).is_some();

    Ok(SessionOrgRuntimeSnapshot {
        session_id,
        agent_id,
        member_id,
        is_coordinator,
        is_org_member,
        org_context,
        registered_org_tool_names,
        requested_exec_mode,
        has_plan_slot,
        has_pre_plan_mode,
    })
}

#[tauri::command]
pub async fn debug_session_execute_tool(
    state: tauri::State<'_, AgentAppState>,
    session_id: String,
    tool_name: String,
    params: Value,
) -> Result<DebugOrgToolResult, String> {
    if tool_name.as_str() != names::READ_FILE {
        return Err(format!(
            "debug_session_execute_tool only allows audited tools [read_file]; got '{tool_name}'"
        ));
    }

    let session = state
        .get_session(&session_id)
        .await
        .ok_or_else(|| format!("session not found: {session_id}"))?;

    let runtime = session
        .get_runtime()
        .await
        .ok_or_else(|| format!("session runtime not initialized: {session_id}"))?;

    // Enforce the same per-turn policy composition the LLM path uses
    // (exec-mode layer over the base policy). Without this the debug
    // hook silently bypasses mode restrictions and E2E runs prove
    // nothing about the gated surface.
    let session_record = crate::session::persistence::get_session(&session_id)
        .map_err(|err| format!("failed to load session record {session_id}: {err}"))?;
    let record_exec_mode = session_record
        .as_ref()
        .and_then(|record| record.agent_exec_mode.as_deref())
        .and_then(crate::session::AgentExecMode::parse)
        .unwrap_or_default();
    let effective_policy = runtime.policy.with_exec_mode(record_exec_mode);
    if !effective_policy.is_allowed(&tool_name) {
        return Err(format!(
            "tool '{tool_name}' is denied by the session's effective policy \
             (exec_mode={record_exec_mode:?})"
        ));
    }

    let mut result = runtime
        .tool_registry
        .execute(
            &tool_name,
            params.clone(),
            &crate::tools::call_context::CallContext::default(),
        )
        .await
        .map_err(|err| err.to_string());

    if tool_name == names::READ_FILE {
        if let Ok(ref mut rich_result) = result {
            if let Some(path) = params.get("path").and_then(|value| value.as_str()) {
                if let Some(extra) = runtime
                    .policy_context_activator
                    .as_deref()
                    .and_then(|activator| activator.augment_for_read_paths(&[path.to_string()]))
                {
                    rich_result.text.push_str(&extra);
                }
            }
        }
    }

    Ok(DebugOrgToolResult::from_tool_result(result))
}

#[tauri::command]
pub async fn debug_session_execute_org_tool(
    state: tauri::State<'_, AgentAppState>,
    session_id: String,
    tool_name: String,
    params: Value,
) -> Result<DebugOrgToolResult, String> {
    if !SESSION_ORG_RUNTIME_TOOL_NAMES
        .iter()
        .any(|name| *name == tool_name)
    {
        return Err(format!(
            "debug_session_execute_org_tool only allows Agent Org session tools [{}]; got '{tool_name}'",
            SESSION_ORG_RUNTIME_TOOL_NAMES.join(", ")
        ));
    }

    let session = state
        .get_session(&session_id)
        .await
        .ok_or_else(|| format!("session not found: {session_id}"))?;

    let runtime = session
        .get_runtime()
        .await
        .ok_or_else(|| format!("session runtime not initialized: {session_id}"))?;

    if runtime.agent_org_context.is_none() {
        return Err(format!(
            "session '{session_id}' is not participating in an Agent Org run"
        ));
    }

    let call_context = crate::tools::call_context::CallContext::new(
        format!("debug-session-org-tool:{tool_name}"),
        session_id,
    );

    Ok(DebugOrgToolResult::from_tool_result(
        runtime
            .tool_registry
            .execute(&tool_name, params, &call_context)
            .await
            .map_err(|err| err.to_string()),
    ))
}

#[tauri::command]
pub async fn debug_agent_org_execute_tool_as_agent(
    _org_store: tauri::State<'_, std::sync::Arc<AgentOrgsStore>>,
    run_id: String,
    sender_member_id: String,
    tool_name: String,
    params: Value,
) -> Result<DebugOrgToolResult, String> {
    if !DETACHED_ORG_RUNTIME_TOOL_NAMES
        .iter()
        .any(|name| *name == tool_name)
    {
        return Err(format!(
            "debug_agent_org_execute_tool_as_agent only allows detached Agent Org tools [{}]; got '{tool_name}'",
            DETACHED_ORG_RUNTIME_TOOL_NAMES.join(", ")
        ));
    }

    let org_context = AgentOrgRunStore::context_for_run(&run_id)?
        .ok_or_else(|| format!("Agent Org run not found: {run_id}"))?;
    let sender = org_context
        .participant_by_member_id(&sender_member_id)
        .ok_or_else(|| {
            format!("sender_member_id '{sender_member_id}' not found in this Agent Org")
        })?;
    let sender_agent_id = sender.agent_id.clone();
    let org_context = Arc::new(org_context);
    let default_call_context = crate::tools::call_context::CallContext::default();
    let task_call_context = match tool_name.as_str() {
        names::TASK_CREATE | names::TASK_GRAPH_CREATE
            if sender_member_id == COORDINATOR_MEMBER_ID =>
        {
            Some(detached_task_call_context(
                &run_id,
                &sender_member_id,
                None,
            )?)
        }
        names::TASK_UPDATE => Some(detached_task_call_context(
            &run_id,
            &sender_member_id,
            params.get("id").and_then(Value::as_str),
        )?),
        _ => None,
    };
    let task_call_context = task_call_context.as_ref().unwrap_or(&default_call_context);

    let result = match tool_name.as_str() {
        names::ORG_SEND_MESSAGE => OrgSendMessageTool::with_hooks(
            Arc::clone(&org_context),
            sender_member_id.clone(),
            Arc::new(NoopInboxWakeHook),
            Arc::new(NoopSelfAbortHook),
        )
        .execute(params, &default_call_context)
        .await
        .map_err(|err| err.to_string()),
        names::TASK_CREATE => {
            let context =
                task_tools_context(org_context, sender_agent_id, sender_member_id.clone());
            TaskCreateTool::new(context)
                .execute(params, task_call_context)
                .await
                .map_err(|err| err.to_string())
        }
        names::TASK_GRAPH_CREATE => {
            let context =
                task_tools_context(org_context, sender_agent_id, sender_member_id.clone());
            TaskGraphCreateTool::new(context)
                .execute(params, task_call_context)
                .await
                .map_err(|err| err.to_string())
        }
        names::TASK_UPDATE => {
            let context =
                task_tools_context(org_context, sender_agent_id, sender_member_id.clone());
            TaskUpdateTool::new(context)
                .execute(params, task_call_context)
                .await
                .map_err(|err| err.to_string())
        }
        names::TASK_LIST => {
            let context =
                task_tools_context(org_context, sender_agent_id, sender_member_id.clone());
            TaskListTool::new(context)
                .execute(params, &default_call_context)
                .await
                .map_err(|err| err.to_string())
        }
        names::TASK_GET => {
            let context =
                task_tools_context(org_context, sender_agent_id, sender_member_id.clone());
            TaskGetTool::new(context)
                .execute(params, &default_call_context)
                .await
                .map_err(|err| err.to_string())
        }
        names::ORG_RUN_COMPLETE => {
            let context =
                task_tools_context(org_context, sender_agent_id, sender_member_id.clone());
            OrgRunCompleteTool::new(context)
                .execute(params, &default_call_context)
                .await
                .map_err(|err| err.to_string())
        }
        _ => Err(format!("unsupported Agent Org tool: {tool_name}")),
    };

    Ok(DebugOrgToolResult::from_tool_result(result))
}

/// Detached WebDriver tool calls still cross the same typed Turn authority as
/// provider calls. Resolve an already-admitted current-generation Turn instead
/// of manufacturing a bypass identity for the test fixture.
fn detached_task_call_context(
    run_id: &str,
    sender_member_id: &str,
    task_id: Option<&str>,
) -> Result<crate::tools::call_context::CallContext, String> {
    let turn_kind = if sender_member_id == COORDINATOR_MEMBER_ID {
        "coordinator"
    } else {
        "task_execution"
    };
    let conn = database::db::get_connection().map_err(|error| error.to_string())?;
    let identity: Option<(String, String)> = conn
        .query_row(
            "SELECT context.turn_intent_id,context.session_id
             FROM agent_org_runtime_turn_contexts context
             JOIN agent_org_runtime_runs run ON run.id=context.org_run_id
             JOIN session_turn_intents intent
               ON intent.session_id=context.session_id
              AND intent.turn_intent_id=context.turn_intent_id
             WHERE context.org_run_id=?1
               AND context.participant_id=?2
               AND context.turn_kind=?3
               AND context.activation_generation=run.activation_generation
               AND run.status='running'
               AND (?4 IS NULL OR context.task_id=?4)
             ORDER BY CASE intent.status
                        WHEN 'running' THEN 0
                        WHEN 'queued' THEN 1
                        ELSE 2
                      END,
                      context.context_id DESC
             LIMIT 1",
            params![run_id, sender_member_id, turn_kind, task_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let (turn_intent_id, session_id) = identity.ok_or_else(|| {
        format!(
            "debug Agent Org task tool has no current typed {turn_kind} context for member {sender_member_id}"
        )
    })?;
    Ok(crate::tools::call_context::CallContext::for_turn(
        format!("debug-detached-org-tool:{sender_member_id}"),
        session_id,
        turn_intent_id,
        Vec::new(),
    ))
}

fn task_tools_context(
    org_context: Arc<AgentOrgRunContext>,
    caller_agent_id: String,
    caller_member_id: String,
) -> Arc<TaskToolsContext> {
    Arc::new(TaskToolsContext {
        org_context,
        caller_agent_id,
        caller_member_id,
        wake_hook: Arc::new(NoopInboxWakeHook),
        app_state: None,
    })
}

#[tauri::command]
pub async fn debug_agent_org_emit_member_idle(
    _org_store: tauri::State<'_, std::sync::Arc<AgentOrgsStore>>,
    run_id: String,
    member_id: String,
    reason: String,
    failure_reason: Option<String>,
    current_mode: Option<String>,
) -> Result<Value, String> {
    let reason = match reason.as_str() {
        "available" => MemberIdleReason::Available,
        "interrupted" => MemberIdleReason::Interrupted,
        "failed" => MemberIdleReason::Failed,
        other => {
            return Err(format!(
                "reason must be one of available|interrupted|failed (got {other:?})"
            ));
        }
    };
    if reason == MemberIdleReason::Failed
        && failure_reason
            .as_deref()
            .map(str::trim)
            .unwrap_or_default()
            .is_empty()
    {
        return Err("failure_reason is required when reason is failed".to_string());
    }
    let current_mode = match current_mode.as_deref() {
        Some(value) => Some(crate::session::AgentExecMode::parse(value).ok_or_else(|| {
            format!("current_mode must be a known AgentExecMode (got {value:?})")
        })?),
        None => None,
    };
    let context = AgentOrgRunStore::context_for_run(&run_id)?
        .ok_or_else(|| format!("Agent Org run not found: {run_id}"))?;
    let member = context
        .participant_by_member_id(&member_id)
        .ok_or_else(|| format!("member_id '{member_id}' not found in Agent Org run '{run_id}'"))?;
    let member_name = if member.member_id == COORDINATOR_MEMBER_ID {
        context.coordinator_name.clone()
    } else {
        context
            .members
            .iter()
            .find(|candidate| candidate.member_id == member.member_id)
            .map(|candidate| candidate.name.clone())
            .unwrap_or_else(|| member.member_id.clone())
    };
    let message = AgentMessage::MemberIdle {
        member_id: member.member_id.clone(),
        member_name,
        reason,
        current_mode,
        summary: None,
        failure_reason,
        unfinished_task_ids: Vec::new(),
    };
    message.validate()?;

    let before_count = AgentInboxStore::count_by_run(&run_id)?;
    AgentInboxStore::insert(InsertInboxParams {
        recipient_agent_id: context.coordinator_agent_id.clone(),
        recipient_member_id: Some(COORDINATOR_MEMBER_ID.to_string()),
        sender_agent_id: SYSTEM_SENDER_ID.to_string(),
        sender_member_id: None,
        org_run_id: Some(run_id.clone()),
        message,
    })?;
    let after_count = AgentInboxStore::count_by_run(&run_id)?;

    Ok(serde_json::json!({
        "ok": true,
        "emitted": after_count > before_count,
        "beforeCount": before_count,
        "afterCount": after_count,
    }))
}

#[tauri::command]
pub async fn debug_agent_org_inbox_list(run_id: String) -> Result<Vec<AgentInboxRecord>, String> {
    AgentInboxStore::list_page_by_run(&run_id, None, 100).map(|page| page.rows)
}

#[tauri::command]
pub async fn debug_agent_org_tasks_list(run_id: String) -> Result<Vec<Task>, String> {
    AgentOrgTaskStore::list(&run_id)
}
