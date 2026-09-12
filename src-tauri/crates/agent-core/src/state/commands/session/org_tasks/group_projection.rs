//! Bounded, backend-authoritative public Agent Org Team timeline projection.
//!
//! The projection keeps conversation and lifecycle activity on one stable
//! cursor, one store, and one subscription. Direct Member work and internal
//! coordination remain private.

use serde::{Deserialize, Serialize};

use crate::coordination::agent_org_runs::{
    context_for_run_record, row_to_run, AgentOrgRunContext, COORDINATOR_MEMBER_ID,
};
use crate::state::AgentAppState;

const DEFAULT_PAGE_LIMIT: usize = 50;
const MAX_PAGE_LIMIT: usize = 100;
const MAX_PAGE_BYTES: usize = 1024 * 1024;
const MAX_CURSOR_BYTES: usize = 256;
const MAX_EVENT_JSON_BYTES: usize = 256 * 1024;
const MAX_VISIBLE_TEXT_CHARS: usize = 32_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentOrgGroupRoute {
    Coordinator,
    Member,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentOrgGroupConversationKind {
    UserMessage,
    AssistantReply,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentOrgGroupDisplayState {
    Queued,
    Running,
    Answered,
    Failed,
    Cancelled,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentOrgGroupRetryMode {
    Rekick,
    NewTurn,
    NewTurnWithConfirmation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AgentOrgGroupSourceRef {
    Event { id: String },
    Inbox { id: i64 },
    InitialInput { id: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentOrgGroupOrderKey {
    pub created_at: String,
    pub source_rank: u16,
    pub stable_source_id: String,
    pub item_ordinal: u8,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentOrgGroupConversationItem {
    pub id: String,
    pub kind: AgentOrgGroupConversationKind,
    pub order: AgentOrgGroupOrderKey,
    pub turn_intent_id: String,
    pub route: AgentOrgGroupRoute,
    pub target_member_id: String,
    pub target_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub responder_member_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub responder_name: Option<String>,
    pub source_ref: AgentOrgGroupSourceRef,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_to_item_id: Option<String>,
    pub text: String,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<AgentOrgGroupDisplayState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    pub can_stop: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_mode: Option<AgentOrgGroupRetryMode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentOrgGroupActivityKind {
    TaskCreated,
    TaskStarted,
    TaskCompleted,
    TaskFailed,
    TaskCancelled,
    TaskReassigned,
    TaskReplacementCreated,
    TeamPaused,
    TeamResumed,
    MemberReturned,
    CompletionCertified,
    FinalReportFailed,
    TeamArchived,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentOrgGroupActivityItem {
    pub id: String,
    pub kind: &'static str,
    pub order: AgentOrgGroupOrderKey,
    pub activity_kind: AgentOrgGroupActivityKind,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub member_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub member_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_member_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_member_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_subject: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replaced_task_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replaced_task_subject: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public_error_code: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentOrgGroupDiagnosticItem {
    pub id: String,
    pub kind: &'static str,
    pub order: AgentOrgGroupOrderKey,
    pub created_at: String,
    pub error_code: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum AgentOrgGroupProjectionItem {
    Conversation(AgentOrgGroupConversationItem),
    Activity(AgentOrgGroupActivityItem),
    Diagnostic(AgentOrgGroupDiagnosticItem),
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentOrgGroupProjectionPage {
    pub run_id: String,
    pub items: Vec<AgentOrgGroupProjectionItem>,
    pub has_more: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct GroupProjectionCursor {
    version: u8,
    created_at: String,
    source_rank: u16,
    stable_source_id: String,
    item_ordinal: u8,
}

#[derive(Debug, Clone)]
struct KeyedItem {
    order: AgentOrgGroupOrderKey,
    item: AgentOrgGroupProjectionItem,
}

#[path = "group_projection/timeline.rs"]
mod timeline;

#[tauri::command]
pub async fn agent_org_group_projection_page(
    state: tauri::State<'_, AgentAppState>,
    session_id: String,
    cursor: Option<String>,
    limit: Option<usize>,
) -> Result<AgentOrgGroupProjectionPage, String> {
    agent_org_group_projection_page_impl(&state, &session_id, cursor.as_deref(), limit).await
}

pub async fn agent_org_group_projection_page_impl(
    _state: &AgentAppState,
    session_id: &str,
    cursor: Option<&str>,
    limit: Option<usize>,
) -> Result<AgentOrgGroupProjectionPage, String> {
    crate::coordination::agent_org_runs::require_agent_org_enabled()?;
    let cursor = cursor.map(timeline::decode_cursor).transpose()?;
    let limit = limit.unwrap_or(DEFAULT_PAGE_LIMIT).clamp(1, MAX_PAGE_LIMIT);
    let session_id = session_id.to_string();
    tokio::task::spawn_blocking(move || timeline::load_projection_page(&session_id, cursor, limit))
        .await
        .map_err(|error| format!("Agent Org Group projection worker failed: {error}"))?
}

#[cfg(test)]
#[path = "group_projection/tests.rs"]
mod tests;
