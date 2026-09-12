//! Durable Agent Org run envelopes.
//!
//! A run records that an Agent Org launched through the normal Rust session
//! stack, while the root session remains the transcript source of truth.

mod availability;
mod helpers;
mod materialization;
mod progress;
mod quiescence;
mod store;
mod worker;

#[cfg(test)]
mod quiescence_blocking_inbox_tests;
#[cfg(test)]
mod tests;

pub use availability::{
    enable_for_webdriver_test as enable_agent_org_for_webdriver_test,
    is_enabled as agent_org_enabled, require_enabled as require_agent_org_enabled,
};
pub(crate) use helpers::{context_for_run_record, row_to_run};
pub use materialization::{
    AgentOrgInitialInput, AgentOrgInitialInputStatus, AgentOrgMaterializationAuthority,
    AgentOrgMaterializationIntent, AgentOrgMaterializationStatus, CreateAgentOrgInitialInput,
    CreateAgentOrgMaterializationIntent,
};
pub use progress::AgentOrgRunProgress;
pub(crate) use progress::{bump_work_revision_in_tx, current_work_revision_in_tx};
pub(crate) use quiescence::guaranteed_current_turn_effects_with_connection;
pub use quiescence::{
    AgentOrgGuaranteedTurnEffects, AgentOrgQuiescenceAssessment, AgentOrgQuiescenceBlocker,
    AgentOrgQuiescenceDecision, AgentOrgQuiescenceFacts, AgentOrgQuiescenceProjection,
    AgentOrgQuiescenceSessionFact,
};
pub use store::AgentOrgRunStore;
pub use worker::{WorkerSessionInfo, WorkerSessionRuntime};

use rusqlite::{Connection, Result as SqliteResult};
use serde::Serialize;

use crate::definitions::orgs::{
    AgentOrgCapabilityIndex, AgentOrgLaunchSnapshot, PlanApprovalPolicy,
};

pub const COORDINATOR_MEMBER_ID: &str = "coordinator";
pub(crate) const DEFAULT_COORDINATOR_DISPLAY_NAME: &str = "Coordinator";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentOrgRunEntryMode {
    StandaloneSession,
}

impl AgentOrgRunEntryMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::StandaloneSession => "standalone_session",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "standalone_session" => Some(Self::StandaloneSession),
            _ => None,
        }
    }
}

impl std::fmt::Display for AgentOrgRunEntryMode {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentOrgRunStatus {
    Starting,
    Running,
    Paused,
    Idle,
    Failed,
    Archived,
}

impl AgentOrgRunStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Idle => "idle",
            Self::Failed => "failed",
            Self::Archived => "archived",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "starting" => Some(Self::Starting),
            "running" => Some(Self::Running),
            "paused" => Some(Self::Paused),
            "idle" => Some(Self::Idle),
            "failed" => Some(Self::Failed),
            "archived" => Some(Self::Archived),
            _ => None,
        }
    }
}

impl std::fmt::Display for AgentOrgRunStatus {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Stable write-fence error shared by every Agent Org commit boundary.
/// Archived is product-visible and irreversible, so it receives its own
/// machine-readable prefix; other lifecycle states retain the existing
/// not-mutable contract.
pub(crate) fn mutation_blocked_error(run_id: &str, status: &str) -> String {
    if status == AgentOrgRunStatus::Archived.as_str() {
        format!("team_archived: Agent Org run {run_id} is read-only")
    } else {
        format!("agent_org_run_not_mutable: run {run_id} is {status}")
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentOrgContextMember {
    pub member_id: String,
    pub name: String,
    pub role: String,
    pub agent_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentOrgParticipant {
    pub member_id: String,
    pub agent_id: String,
    pub is_coordinator: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum AgentOrgCompletionRequestOutcome {
    Recorded { progress: Box<AgentOrgRunProgress> },
    OpenTasks { unresolved_task_ids: Vec<String> },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentOrgRunContext {
    pub run_id: String,
    pub org_id: String,
    pub org_name: String,
    pub org_role: String,
    /// Stable agent_id of the coordinator session. Routing uses
    /// `COORDINATOR_MEMBER_ID`; this field is only the runtime definition id.
    pub coordinator_agent_id: String,
    /// Display name of the coordinator participant. This is intentionally
    /// distinct from `org_name`; chat cards and inbox routing should name the
    /// recipient role, not the Agent Org session title.
    pub coordinator_name: String,
    /// Role label of the coordinator (e.g. "lead engineer"). Mirror of
    /// `OrgDefinition.role`. Informational only — not used for routing.
    pub coordinator_role: String,
    /// Worker roster. Does **not** include the coordinator — addressing
    /// logic explicitly considers `{coordinator} ∪ members` as the
    /// eligible recipient set.
    pub members: Vec<AgentOrgContextMember>,
    /// Plan-approval policy captured in the launch snapshot.
    pub plan_approval_policy: PlanApprovalPolicy,
    /// Compiled capability facts frozen at Team launch. Writer authority is
    /// resolved from this snapshot; mutable prompts, links, and definitions
    /// cannot grant it to an already-running Team.
    #[serde(skip)]
    pub capability_index: AgentOrgCapabilityIndex,
    /// Session ID of the coordinator (root) session for this run. Used by
    /// the frontend to navigate directly to the coordinator's chat history
    /// when the run is paused or the coordinator is not the active session.
    /// `None` only for runs that have not yet materialized a coordinator
    /// session (e.g. created but never started).
    pub root_session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoutingDecision {
    Allowed,
    Blocked(String),
}

impl AgentOrgRunContext {
    pub fn coordinator_participant(&self) -> AgentOrgParticipant {
        AgentOrgParticipant {
            member_id: COORDINATOR_MEMBER_ID.to_string(),
            agent_id: self.coordinator_agent_id.clone(),
            is_coordinator: true,
        }
    }

    pub fn participants(&self) -> Vec<AgentOrgParticipant> {
        let mut participants = Vec::with_capacity(self.members.len() + 1);
        participants.push(self.coordinator_participant());
        participants.extend(self.members.iter().map(|member| AgentOrgParticipant {
            member_id: member.member_id.clone(),
            agent_id: member.agent_id.clone(),
            is_coordinator: false,
        }));
        participants
    }

    pub fn participant_by_member_id(&self, member_id: &str) -> Option<AgentOrgParticipant> {
        if member_id == COORDINATOR_MEMBER_ID {
            return Some(self.coordinator_participant());
        }
        self.members
            .iter()
            .find(|member| member.member_id == member_id)
            .map(|member| AgentOrgParticipant {
                member_id: member.member_id.clone(),
                agent_id: member.agent_id.clone(),
                is_coordinator: false,
            })
    }

    pub fn participant_display_name(&self, member_id: &str) -> Option<String> {
        if member_id == COORDINATOR_MEMBER_ID {
            return Some(self.coordinator_name.clone());
        }
        self.members
            .iter()
            .find(|member| member.member_id == member_id)
            .map(|member| member.name.clone())
    }

    pub fn participant_agent_id(&self, member_id: &str) -> Option<String> {
        self.participant_by_member_id(member_id)
            .map(|participant| participant.agent_id)
    }

    pub fn require_participant(&self, member_id: &str) -> Result<AgentOrgParticipant, String> {
        self.participant_by_member_id(member_id).ok_or_else(|| {
            format!("member_id '{member_id}' is not a participant in this Agent Org run")
        })
    }

    pub fn require_participant_display_name(&self, member_id: &str) -> Result<String, String> {
        self.participant_display_name(member_id).ok_or_else(|| {
            format!("member_id '{member_id}' is not a participant in this Agent Org run")
        })
    }

    pub fn require_participant_agent_id(&self, member_id: &str) -> Result<String, String> {
        self.participant_agent_id(member_id).ok_or_else(|| {
            format!("member_id '{member_id}' is not a participant in this Agent Org run")
        })
    }

    pub fn allowed_recipient_member_ids_for(&self, sender_member_id: &str) -> Vec<String> {
        if self.participant_by_member_id(sender_member_id).is_none() {
            return Vec::new();
        }

        let mut allowed = if sender_member_id == COORDINATOR_MEMBER_ID {
            self.members
                .iter()
                .map(|member| member.member_id.clone())
                .collect::<Vec<_>>()
        } else {
            vec![COORDINATOR_MEMBER_ID.to_string()]
        };
        allowed.sort();
        allowed.dedup();
        allowed
    }

    /// Static schema surface for `org_send_message`. Member tools expose the
    /// Coordinator plus peers linked in the immutable launch snapshot; the
    /// exact persisted Turn still decides which subset is legal at execution.
    pub fn user_directed_recipient_member_ids_for(&self, sender_member_id: &str) -> Vec<String> {
        if self.participant_by_member_id(sender_member_id).is_none() {
            return Vec::new();
        }
        if sender_member_id == COORDINATOR_MEMBER_ID {
            return self.allowed_recipient_member_ids_for(sender_member_id);
        }
        let mut allowed = vec![COORDINATOR_MEMBER_ID.to_string()];
        allowed.extend(
            self.members
                .iter()
                .filter(|member| {
                    member.member_id != sender_member_id
                        && self
                            .capability_index
                            .members_can_communicate(sender_member_id, &member.member_id)
                })
                .map(|member| member.member_id.clone()),
        );
        allowed.sort();
        allowed.dedup();
        allowed
    }

    pub fn user_directed_can_message(
        &self,
        sender_member_id: &str,
        recipient_member_id: &str,
    ) -> bool {
        self.user_directed_recipient_member_ids_for(sender_member_id)
            .iter()
            .any(|member_id| member_id == recipient_member_id)
    }

    /// Task assignees that `caller_member_id` is authorized to manage.
    ///
    /// - coordinator: itself plus every roster member;
    /// - configured graph writer: itself plus every roster member;
    /// - ordinary member: itself.
    ///
    /// This is the task-governance source of truth. It must not be replaced by
    /// `allowed_recipient_member_ids_for`: permission to talk to a peer is not
    /// permission to assign that peer work.
    pub fn allowed_task_target_member_ids_for(&self, caller_member_id: &str) -> Vec<String> {
        if self.participant_by_member_id(caller_member_id).is_none() {
            return Vec::new();
        }

        let mut allowed = if caller_member_id == COORDINATOR_MEMBER_ID
            || self.capability_index.is_additional_writer(caller_member_id)
        {
            self.participants()
                .into_iter()
                .map(|participant| participant.member_id)
                .collect::<Vec<_>>()
        } else {
            vec![caller_member_id.to_string()]
        };
        allowed.sort();
        allowed.dedup();
        allowed
    }

    pub fn can_assign_task_to(&self, caller_member_id: &str, target_member_id: &str) -> bool {
        self.allowed_task_target_member_ids_for(caller_member_id)
            .iter()
            .any(|member_id| member_id == target_member_id)
    }

    pub fn check_routing(&self, from_member_id: &str, to_member_id: &str) -> RoutingDecision {
        if self
            .allowed_recipient_member_ids_for(from_member_id)
            .iter()
            .any(|member_id| member_id == to_member_id)
        {
            return RoutingDecision::Allowed;
        }

        RoutingDecision::Blocked(format!(
            "recipient_member_id '{to_member_id}' is not currently routable from sender_member_id '{from_member_id}'; formal message routing does not permit this member-to-member route. Allowed recipient_member_id values: {}",
            self.allowed_recipient_member_ids_for(from_member_id).join(", ")
        ))
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentOrgRunRecord {
    pub id: String,
    pub org_id: String,
    pub coordinator_agent_id: String,
    pub root_session_id: Option<String>,
    pub org_snapshot_json: Option<String>,
    pub entry_mode: AgentOrgRunEntryMode,
    pub status: AgentOrgRunStatus,
    pub activation_generation: i64,
    pub has_initial_work: bool,
    pub work_item_id: Option<String>,
    pub project_slug: Option<String>,
    pub routine_fire_id: Option<String>,
    pub summary: Option<String>,
    pub last_error: Option<String>,
    pub failure_json: Option<String>,
    pub last_activity_outcome: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub idled_at: Option<String>,
    pub archived_at: Option<String>,
    pub archive_receipt_id: Option<String>,
}

/// One-shot persisted work produced before AgentAppState is installed.
/// Only the exact receipt IDs in this plan may ring a post-init doorbell.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentOrgStartupRecoveryPlan {
    pub inspected_runs: usize,
    pub inspected_terminal_members: usize,
    pub skipped_without_unique_execution: usize,
    pub recovered_tasks: Vec<crate::coordination::agent_org_tasks::TaskExecutionRecovery>,
    pub failures: Vec<AgentOrgStartupRecoveryFailure>,
}

impl AgentOrgStartupRecoveryPlan {
    pub fn recovered_task_count(&self) -> usize {
        self.recovered_tasks.len()
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentOrgStartupRecoveryFailure {
    pub org_run_id: String,
    pub session_id: String,
    pub member_id: Option<String>,
    pub error: String,
}

#[derive(Debug, Clone)]
pub struct CreateAgentOrgRunParams {
    pub org_id: String,
    pub coordinator_agent_id: String,
    pub root_session_id: Option<String>,
    pub org_snapshot: AgentOrgLaunchSnapshot,
    pub entry_mode: AgentOrgRunEntryMode,
    pub status: AgentOrgRunStatus,
    pub work_item_id: Option<String>,
    pub project_slug: Option<String>,
    pub routine_fire_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CreateStartingAgentOrgRunParams {
    pub org_id: String,
    pub coordinator_agent_id: String,
    pub root_session_id: String,
    pub org_snapshot: AgentOrgLaunchSnapshot,
    pub entry_mode: AgentOrgRunEntryMode,
    pub work_item_id: Option<String>,
    pub project_slug: Option<String>,
    pub routine_fire_id: Option<String>,
    pub materialization_intents: Vec<CreateAgentOrgMaterializationIntent>,
    pub initial_input: Option<CreateAgentOrgInitialInput>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentOrgStartingFailure {
    pub code: String,
    pub message: String,
}

impl AgentOrgStartingFailure {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

/// Initialize the canonical runtime run envelope in an already-isolated
/// namespace. Production startup uses the complete schema coordinator; this
/// narrower entry point remains available to focused unit tests.
pub fn init_schema(conn: &Connection) -> SqliteResult<()> {
    create_schema(conn)
}

pub(crate) fn create_schema(conn: &Connection) -> SqliteResult<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS agent_org_runtime_runs (
            id TEXT PRIMARY KEY,
            org_id TEXT NOT NULL,
            coordinator_agent_id TEXT NOT NULL,
            root_session_id TEXT,
            org_snapshot_json TEXT,
            entry_mode TEXT NOT NULL,
            status TEXT NOT NULL CHECK(status IN (
                'starting', 'running', 'paused', 'idle', 'failed', 'archived'
            )),
            activation_generation INTEGER NOT NULL DEFAULT 1
                CHECK(activation_generation >= 1),
            has_initial_work INTEGER NOT NULL DEFAULT 0
                CHECK(has_initial_work IN (0, 1)),
            work_item_id TEXT,
            project_slug TEXT,
            routine_fire_id TEXT,
            summary TEXT,
            last_error TEXT,
            failure_json TEXT,
            last_activity_outcome TEXT CHECK(last_activity_outcome IN (
                'completed', 'failed', 'cancelled'
            )),
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            idled_at TEXT,
            archived_at TEXT,
            archive_receipt_id TEXT UNIQUE,
            CHECK(
                (status='archived' AND archived_at IS NOT NULL AND archive_receipt_id IS NOT NULL)
                OR
                (status<>'archived' AND archived_at IS NULL AND archive_receipt_id IS NULL)
            )
        );
        CREATE INDEX IF NOT EXISTS idx_agent_org_runtime_runs_org_updated
            ON agent_org_runtime_runs(org_id, updated_at);
        CREATE INDEX IF NOT EXISTS idx_agent_org_runtime_runs_root_session
            ON agent_org_runtime_runs(root_session_id);
        CREATE INDEX IF NOT EXISTS idx_agent_org_runtime_runs_work_item
            ON agent_org_runtime_runs(work_item_id);
        CREATE INDEX IF NOT EXISTS idx_agent_org_runtime_runs_status
            ON agent_org_runtime_runs(status);",
    )?;
    materialization::init_schema(conn)?;
    progress::init_schema(conn)?;
    Ok(())
}
