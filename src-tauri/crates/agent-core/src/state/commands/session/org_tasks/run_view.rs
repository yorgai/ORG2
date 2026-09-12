//! Agent Org Run View assembly.
//!
//! The Run View is the frequently-polled operational snapshot of a run: its
//! members, a bounded task window, recent inbox previews and immutable plan
//! revision history, plus the derived run phase. This module owns the view DTOs, the
//! `agent_org_session_run_view` command, and every projection helper that turns
//! durable rows into the bridge payload.

use std::collections::HashMap;

use database::db::get_connection;
use rusqlite::params;
use serde::Serialize;

use crate::coordination::agent_inbox::{
    AgentInboxPreviewRecord, AgentInboxRecipientCounts, AgentInboxRecord, AgentInboxStore,
    AgentInboxUnreadRecipientCounts, AgentMessage, SYSTEM_SENDER_ID, USER_SENDER_ID,
};
use crate::coordination::agent_member_interventions::{
    AgentMemberInterventionRecord, AgentMemberInterventionStore, MemberInterventionStatus,
};
use crate::coordination::agent_org_plan_approvals::{
    AgentOrgPlanDecisionStatus, AgentOrgPlanRevisionStore, AgentOrgPlanRevisionSummary,
};
use crate::coordination::agent_org_run_blockers::{AgentOrgRunBlocker, AgentOrgRunWorkState};
use crate::coordination::agent_org_runs::{
    AgentOrgContextMember, AgentOrgRunContext, AgentOrgRunStatus, AgentOrgRunStore,
    WorkerSessionRuntime, COORDINATOR_MEMBER_ID,
};
use crate::coordination::agent_org_tasks::{
    AgentOrgTaskStore, Task, TaskExecutionMode, TaskOutputSummary, TaskPageBucket,
    TaskPageDirection, TaskSummary,
};
use crate::state::AgentAppState;

use super::context::{require_session_member_id, session_org_read_context};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentOrgTaskRuntime {
    #[serde(flatten)]
    pub task: Task,
    /// The frequently-polled Run View carries only a description preview.
    /// Fetch `task_get` when this flag is true and full task context is needed.
    pub description_truncated: bool,
    pub blocks_truncated: bool,
    pub blocked_by_truncated: bool,
    pub dependencies_satisfied: bool,
    pub execution_mode: TaskExecutionMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_summary: Option<TaskOutputSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner_member: Option<AgentOrgContextMember>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner_runtime: Option<WorkerSessionRuntime>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_handoff:
        Option<crate::coordination::agent_org_task_handoffs::TaskExecutionHandoffReceipt>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentOrgRunMemberView {
    pub member_id: String,
    pub name: String,
    pub role: String,
    pub agent_id: String,
    pub is_coordinator: bool,
    pub writer_capable: bool,
    pub session_runtime: Option<WorkerSessionRuntime>,
    pub unread_inbox_count: usize,
    pub inbox_activity_count: usize,
    pub active_task_count: usize,
    pub pending_task_count: usize,
    pub in_progress_task_count: usize,
    pub completed_task_count: usize,
    pub queued_user_directed_count: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activity: Option<AgentOrgMemberActivity>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intervention: Option<AgentMemberInterventionRecord>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentOrgMemberActivityKind {
    Yielding,
    UserIntervention,
    SideQuest,
    YieldTimeout,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentOrgMemberActivity {
    pub kind: AgentOrgMemberActivityKind,
    pub source: &'static str,
    pub intervention_receipt_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentOrgInboxRuntimeRow {
    #[serde(flatten)]
    pub row: AgentInboxRecord,
    pub recipient_name: String,
    pub sender_name: String,
    pub display_text: String,
}

/// Lightweight inbox activity projected in the frequently-polled Run View.
/// The durable `payload_json` remains in `agent_inbox` and in direct command
/// responses, but is deliberately omitted here so a large message is not
/// copied over the Tauri bridge on every refresh.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentOrgInboxPreviewRow {
    pub id: i64,
    pub recipient_agent_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recipient_member_id: Option<String>,
    pub sender_agent_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sender_member_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub org_run_id: Option<String>,
    pub payload_kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivery_resolution: Option<String>,
    pub recipient_name: String,
    pub sender_name: String,
    pub display_text: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentOrgRunView {
    pub context: AgentOrgRunContext,
    pub run_status: String,
    pub run_phase: AgentOrgRunPhase,
    pub coordinator_work_state: AgentOrgCoordinatorWorkState,
    pub completion: AgentOrgRunCompletionView,
    pub final_summary: Option<crate::coordination::agent_org_final_summary::FinalSummaryReceipt>,
    pub formal_activity: crate::coordination::agent_org_formal_triggers::FormalTriggerActivity,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pause_handoff: Option<crate::coordination::agent_org_pause::PauseHandoffSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archive_teardown: Option<crate::coordination::agent_org_archive::ArchiveTeardownSummary>,
    pub current_member_id: Option<String>,
    pub members: Vec<AgentOrgRunMemberView>,
    pub tasks: Vec<AgentOrgTaskRuntime>,
    pub execution_handoffs:
        Vec<crate::coordination::agent_org_task_handoffs::TaskExecutionHandoffReceipt>,
    pub task_overview: AgentOrgRunTaskOverview,
    /// Bounded, batched current-state overlay for immutable task event cards.
    pub task_state_window: AgentOrgTaskStateWindow,
    /// Typed operational dimensions derived from the same quiescence facts
    /// used by completion certification.
    pub work_state: AgentOrgRunWorkState,
    pub blockers: Vec<AgentOrgRunBlocker>,
    pub inbox: Vec<AgentOrgInboxPreviewRow>,
    /// All unresolved unread rows retained for durable Inbox history.
    pub unread_inbox_count: usize,
    /// Unresolved Inbox work that can still affect runtime convergence.
    /// Routine lifecycle history such as stale `member_idle` rows is excluded.
    pub blocking_unread_inbox_count: usize,
    pub plan_revisions: Vec<AgentOrgPlanRevisionSummary>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentOrgCoordinatorWorkState {
    Active,
    WaitingForOrgEvent,
    Inactive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentOrgRunCompletionState {
    None,
    NeedsAttention,
    Certified,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentOrgRunCompletionView {
    pub state: AgentOrgRunCompletionState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome: Option<crate::coordination::agent_org_run_completion::RunCompletionOutcome>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub certificate_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub work_revision: Option<i64>,
}

/// Exact task totals plus the bounded window carried by the frequently-polled
/// Run View. Full task detail remains available through `task_get`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentOrgRunTaskOverview {
    pub total: usize,
    pub pending: usize,
    pub in_progress: usize,
    pub completed: usize,
    pub failed: usize,
    pub cancelled: usize,
    pub corrupt: usize,
    pub visible: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentOrgTaskStateProjection {
    pub task_id: String,
    pub status: crate::coordination::agent_org_tasks::TaskStatus,
    pub owner_member_id: Option<String>,
    pub activation_generation: i64,
    pub replaces_task_id: Option<String>,
    pub replacement_task_id: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentOrgTaskStateWindow {
    pub tasks: Vec<AgentOrgTaskStateProjection>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentOrgRunPhase {
    Starting,
    Coordinating,
    Dispatching,
    MembersWorking,
    Waiting,
    AwaitingPlanApproval,
    Finalizing,
    Draining,
    Paused,
    Idle,
    Failed,
    Archived,
}

/// The Run View is a live operational snapshot, not an inbox-history API.
/// Keep the bridge payload bounded; durable history remains available through
/// the explicitly paginated inbox/history surfaces.
const RUN_VIEW_INBOX_LIMIT: usize = 200;
const RUN_VIEW_TASK_LIMIT: usize = 50;
const RUN_VIEW_TASK_STATE_LIMIT: usize = 200;

#[tauri::command]
pub async fn agent_org_session_run_view(
    state: tauri::State<'_, AgentAppState>,
    session_id: String,
) -> Result<Option<AgentOrgRunView>, String> {
    agent_org_session_run_view_impl(&state, &session_id).await
}

pub async fn agent_org_session_run_view_impl(
    state: &AgentAppState,
    session_id: &str,
) -> Result<Option<AgentOrgRunView>, String> {
    crate::coordination::agent_org_runs::require_agent_org_enabled()?;
    let Some(read_context) = session_org_read_context(state, session_id).await? else {
        return Ok(None);
    };
    let Some(context) = read_context.context.as_ref() else {
        return Ok(None);
    };
    let current_member_id = require_session_member_id(&read_context, session_id)?;
    let context = context.clone();

    // Group Chat polls this command while it is visible. SQLite reads and
    // snapshot projection are synchronous, so keep them off the async/Tauri
    // executor. This remains a pure read: reconciliation belongs to the
    // watchdog or an explicit completion command.
    let view =
        tokio::task::spawn_blocking(move || build_agent_org_run_view(&context, current_member_id))
            .await
            .map_err(|err| format!("Agent Org Run View worker failed: {err}"))??;

    Ok(Some(view))
}

pub(super) fn build_agent_org_run_view(
    context: &AgentOrgRunContext,
    current_member_id: String,
) -> Result<AgentOrgRunView, String> {
    let mut conn = get_connection().map_err(|err| err.to_string())?;
    let tx = conn
        .transaction_with_behavior(rusqlite::TransactionBehavior::Deferred)
        .map_err(|err| err.to_string())?;
    let quiescence = AgentOrgRunStore::quiescence_assessment_with_connection(&tx, &context.run_id)?;
    let run_status_value = quiescence
        .facts
        .run_status
        .ok_or_else(|| format!("Agent Org run {} no longer exists", context.run_id))?;
    let run_status = run_status_value.as_str().to_string();

    let task_page = AgentOrgTaskStore::list_task_page_with_connection(
        &tx,
        &context.run_id,
        TaskPageBucket::Current,
        None,
        None,
        TaskPageDirection::Forward,
        RUN_VIEW_TASK_LIMIT,
    )?;
    let (failed_task_count, cancelled_task_count): (i64, i64) = tx
        .query_row(
            "SELECT
                COALESCE(SUM(CASE WHEN status='failed' THEN 1 ELSE 0 END),0),
                COALESCE(SUM(CASE WHEN status='cancelled' THEN 1 ELSE 0 END),0)
             FROM agent_org_runtime_tasks WHERE org_run_id=?1",
            params![&context.run_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|error| error.to_string())?;
    let task_overview = AgentOrgRunTaskOverview {
        total: quiescence.facts.task_count,
        pending: quiescence.facts.pending_task_count,
        in_progress: quiescence.facts.in_progress_task_count,
        completed: quiescence.facts.completed_task_count,
        failed: failed_task_count.max(0) as usize,
        cancelled: cancelled_task_count.max(0) as usize,
        corrupt: quiescence.facts.corrupt_task_count,
        visible: task_page.tasks.len(),
        truncated: task_page.has_more,
    };
    let task_state_window = load_task_state_window(&tx, &context.run_id)?;
    let work_state = crate::coordination::agent_org_run_blockers::work_state(&quiescence);
    let blockers = crate::coordination::agent_org_run_blockers::build_with_connection(
        &tx,
        &context.run_id,
        &quiescence,
    )?;
    let member_task_counts = task_counts_by_owner_with_connection(&tx, &context.run_id)?;
    let inbox_records = AgentInboxStore::list_recent_previews_by_run_with_connection(
        &tx,
        &context.run_id,
        RUN_VIEW_INBOX_LIMIT,
    )?;
    let unread_inbox_counts =
        AgentInboxStore::unread_counts_by_recipient_with_connection(&tx, &context.run_id)?;
    let inbox_counts = bounded_run_view_inbox_counts(&inbox_records, &unread_inbox_counts);
    let member_ids: Vec<String> = context
        .members
        .iter()
        .map(|member| member.member_id.clone())
        .collect();
    let member_runtimes: HashMap<String, WorkerSessionRuntime> =
        AgentOrgRunStore::list_worker_sessions_by_member_ids_with_connection(
            &tx,
            &context.run_id,
            &member_ids,
        )?
        .into_iter()
        .filter_map(|session| {
            session
                .member_id
                .clone()
                .map(|member_id| (member_id, session))
        })
        .collect();
    let active_interventions: HashMap<String, AgentMemberInterventionRecord> =
        AgentMemberInterventionStore::list_active_with_connection(&tx, &context.run_id)?
            .into_iter()
            .map(|record| (record.member_id.clone(), record))
            .collect();
    let coordinator_runtime =
        AgentOrgRunStore::find_coordinator_session_by_member_id_with_connection(
            &tx,
            &context.run_id,
            COORDINATOR_MEMBER_ID,
        )?
        .map(|session| WorkerSessionRuntime {
            agent_definition_id: Some(context.coordinator_agent_id.clone()),
            cli_agent_type: None,
            member_id: Some(COORDINATOR_MEMBER_ID.to_string()),
            session_id: session.session_id,
            parent_session_id: None,
            status: session.status,
            updated_at: session.updated_at,
            intervention: None,
        });

    let coordinator_work_state = if coordinator_runtime.as_ref().is_some_and(|runtime| {
        matches!(
            runtime.status,
            crate::session::SessionStatus::Running
                | crate::session::SessionStatus::WaitingForUser
                | crate::session::SessionStatus::WaitingForFunds
        )
    }) {
        AgentOrgCoordinatorWorkState::Active
    } else {
        if latest_coordinator_is_waiting_for_org_event(
            &tx,
            &context.run_id,
            quiescence.facts.activation_generation,
        )? {
            AgentOrgCoordinatorWorkState::WaitingForOrgEvent
        } else {
            AgentOrgCoordinatorWorkState::Inactive
        }
    };

    let handoffs = match quiescence.facts.activation_generation {
        Some(generation) => {
            crate::coordination::agent_org_task_handoffs::list_current_with_connection(
                &tx,
                &context.run_id,
                generation,
            )?
        }
        None => Vec::new(),
    };
    let mut handoffs_by_task = HashMap::new();
    for receipt in &handoffs {
        handoffs_by_task.insert(receipt.old_task_id.clone(), receipt.clone());
        if let Some(replacement_task_id) = receipt.replacement_task_id.as_ref() {
            handoffs_by_task.insert(replacement_task_id.clone(), receipt.clone());
        }
    }

    let tasks = tasks_for_context(
        context,
        task_page.tasks,
        &member_runtimes,
        &handoffs_by_task,
    );

    let completion = match (
        quiescence.facts.completion_certificate.as_ref(),
        quiescence.facts.progress.as_ref(),
    ) {
        (Some(certificate), Some(progress))
            if certificate.work_revision == progress.work_revision
                && final_summary_has_terminal_publication(
                    quiescence.facts.final_summary_receipt.as_ref(),
                ) =>
        {
            AgentOrgRunCompletionView {
                state: AgentOrgRunCompletionState::Certified,
                outcome: Some(certificate.outcome),
                certificate_id: Some(certificate.id.clone()),
                work_revision: Some(certificate.work_revision),
            }
        }
        (Some(_), _) => AgentOrgRunCompletionView {
            state: AgentOrgRunCompletionState::NeedsAttention,
            outcome: None,
            certificate_id: None,
            work_revision: None,
        },
        (None, _)
            if quiescence.facts.task_count > 0
                && (quiescence.facts.unresolved_task_count == 0
                    || quiescence.facts.unresolved_handoff_count > 0) =>
        {
            AgentOrgRunCompletionView {
                state: AgentOrgRunCompletionState::NeedsAttention,
                outcome: None,
                certificate_id: None,
                work_revision: None,
            }
        }
        (None, _) => AgentOrgRunCompletionView {
            state: AgentOrgRunCompletionState::None,
            outcome: None,
            certificate_id: None,
            work_revision: None,
        },
    };

    let mut members = Vec::with_capacity(context.members.len() + 1);
    members.push(coordinator_member_view(
        context,
        coordinator_runtime,
        &member_task_counts,
        &inbox_counts,
        &active_interventions,
    )?);
    for member in &context.members {
        members.push(member_view(
            member,
            context
                .capability_index
                .is_additional_writer(&member.member_id),
            member_runtimes.get(&member.member_id).cloned(),
            &member_task_counts,
            &inbox_counts,
            &active_interventions,
        )?);
    }

    let inbox = enrich_inbox_preview_rows(context, inbox_records);
    let plan_revisions = AgentOrgPlanRevisionStore::list_revision_summaries_by_run_with_connection(
        &tx,
        &context.run_id,
        100,
    )?;
    let formal_activity = crate::coordination::agent_org_formal_triggers::activity_with_connection(
        &tx,
        &context.run_id,
        50,
    )?;

    // Running Teams are polled while active; keep pause-receipt aggregation
    // entirely off that hot path. Paused Teams receive push updates only.
    let pause_handoff = if run_status_value == AgentOrgRunStatus::Paused {
        crate::coordination::agent_org_pause::pause_summary_with_connection(&tx, &context.run_id)?
    } else {
        None
    };
    let archive_teardown = if run_status_value == AgentOrgRunStatus::Archived {
        crate::coordination::agent_org_archive::summary_for_run_with_connection(
            &tx,
            &context.run_id,
        )?
    } else {
        None
    };
    let run_phase = if run_status_value == AgentOrgRunStatus::Paused
        && pause_handoff
            .as_ref()
            .is_some_and(|summary| summary.draining_count > 0)
    {
        AgentOrgRunPhase::Draining
    } else {
        project_run_phase(
            run_status_value,
            &members,
            &task_overview,
            quiescence.facts.blocking_unread_inbox_count,
            &plan_revisions,
            quiescence.facts.final_summary_receipt.as_ref(),
        )
    };

    tx.commit().map_err(|err| err.to_string())?;

    Ok(AgentOrgRunView {
        current_member_id: Some(current_member_id),
        context: context.clone(),
        run_status,
        run_phase,
        coordinator_work_state,
        completion,
        final_summary: quiescence.facts.final_summary_receipt.clone(),
        formal_activity,
        pause_handoff,
        archive_teardown,
        members,
        tasks,
        execution_handoffs: handoffs,
        task_overview,
        task_state_window,
        work_state,
        blockers,
        inbox,
        unread_inbox_count: quiescence.facts.unread_inbox_count,
        blocking_unread_inbox_count: quiescence.facts.blocking_unread_inbox_count,
        plan_revisions,
    })
}

pub(super) fn load_task_state_window(
    conn: &rusqlite::Connection,
    org_run_id: &str,
) -> Result<AgentOrgTaskStateWindow, String> {
    let mut statement = conn
        .prepare(
            "SELECT task.id,task.status,task.owner,task.activation_generation,
                    task.replaces_task_id,
                    (SELECT replacement.id
                     FROM agent_org_runtime_tasks replacement
                     WHERE replacement.org_run_id=task.org_run_id
                       AND replacement.replaces_task_id=task.id
                     ORDER BY replacement.created_at,replacement.id LIMIT 1),
                    task.updated_at
             FROM agent_org_runtime_tasks task
             WHERE task.org_run_id=?1
             ORDER BY task.updated_at DESC,task.id
             LIMIT ?2",
        )
        .map_err(|error| error.to_string())?;
    let mut tasks = statement
        .query_map(
            params![org_run_id, (RUN_VIEW_TASK_STATE_LIMIT + 1) as i64],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, String>(6)?,
                ))
            },
        )
        .map_err(|error| error.to_string())?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|error| error.to_string())?;
    let truncated = tasks.len() > RUN_VIEW_TASK_STATE_LIMIT;
    tasks.truncate(RUN_VIEW_TASK_STATE_LIMIT);
    Ok(AgentOrgTaskStateWindow {
        tasks: tasks
            .into_iter()
            .map(
                |(
                    task_id,
                    status,
                    owner_member_id,
                    activation_generation,
                    replaces_task_id,
                    replacement_task_id,
                    updated_at,
                )| {
                    Ok(AgentOrgTaskStateProjection {
                        task_id,
                        status: crate::coordination::agent_org_tasks::TaskStatus::from_wire(
                            &status,
                        )?,
                        owner_member_id,
                        activation_generation,
                        replaces_task_id,
                        replacement_task_id,
                        updated_at,
                    })
                },
            )
            .collect::<Result<Vec<_>, String>>()?,
        truncated,
    })
}

fn final_summary_has_terminal_publication(
    receipt: Option<&crate::coordination::agent_org_final_summary::FinalSummaryReceipt>,
) -> bool {
    receipt.is_some_and(|receipt| {
        matches!(
            receipt.status,
            crate::coordination::agent_org_final_summary::FinalSummaryStatus::Persisted
                | crate::coordination::agent_org_final_summary::FinalSummaryStatus::Failed
        )
    })
}

#[cfg(test)]
mod final_summary_tests;
#[cfg(test)]
mod phase_tests;

pub(super) fn latest_coordinator_is_waiting_for_org_event(
    conn: &rusqlite::Connection,
    org_run_id: &str,
    activation_generation: Option<i64>,
) -> Result<bool, String> {
    conn.query_row(
        "SELECT COALESCE((
            SELECT context.terminal_reason='waiting_for_org_event'
                   AND intent.status IN ('completed','failed','cancelled','abandoned')
            FROM agent_org_runtime_turn_contexts context
            JOIN session_turn_intents intent
              ON intent.session_id=context.session_id
             AND intent.turn_intent_id=context.turn_intent_id
            WHERE context.org_run_id=?1 AND context.turn_kind='coordinator'
              AND context.source_kind IN ('root_turn','group_root')
              AND context.activation_generation=?2
            ORDER BY context.context_id DESC
            LIMIT 1
         ),0)",
        params![org_run_id, activation_generation],
        |row| row.get(0),
    )
    .map_err(|error| error.to_string())
}

pub(super) fn project_run_phase(
    run_status: AgentOrgRunStatus,
    members: &[AgentOrgRunMemberView],
    task_overview: &AgentOrgRunTaskOverview,
    unread_inbox_count: usize,
    plan_revisions: &[AgentOrgPlanRevisionSummary],
    final_summary: Option<&crate::coordination::agent_org_final_summary::FinalSummaryReceipt>,
) -> AgentOrgRunPhase {
    match run_status {
        AgentOrgRunStatus::Starting => AgentOrgRunPhase::Starting,
        AgentOrgRunStatus::Paused => AgentOrgRunPhase::Paused,
        AgentOrgRunStatus::Idle => AgentOrgRunPhase::Idle,
        AgentOrgRunStatus::Failed => AgentOrgRunPhase::Failed,
        AgentOrgRunStatus::Archived => AgentOrgRunPhase::Archived,
        AgentOrgRunStatus::Running => {
            if final_summary.is_some_and(|receipt| receipt.status.is_finalizing()) {
                return AgentOrgRunPhase::Finalizing;
            }
            let any_member_working = members.iter().any(|member| {
                member.session_runtime.as_ref().is_some_and(|runtime| {
                    matches!(
                        runtime.status,
                        crate::session::SessionStatus::Running
                            | crate::session::SessionStatus::WaitingForUser
                            | crate::session::SessionStatus::WaitingForFunds
                    )
                })
            });
            if any_member_working {
                return AgentOrgRunPhase::MembersWorking;
            }
            if plan_revisions
                .iter()
                .any(|revision| revision.status == AgentOrgPlanDecisionStatus::Pending)
            {
                return AgentOrgRunPhase::AwaitingPlanApproval;
            }
            if unread_inbox_count > 0 {
                return AgentOrgRunPhase::Dispatching;
            }
            if task_overview.pending > 0
                || task_overview.in_progress > 0
                || task_overview.corrupt > 0
            {
                AgentOrgRunPhase::Waiting
            } else {
                AgentOrgRunPhase::Coordinating
            }
        }
    }
}

pub(super) fn tasks_for_context(
    context: &AgentOrgRunContext,
    tasks: Vec<TaskSummary>,
    owner_runtimes: &HashMap<String, WorkerSessionRuntime>,
    handoffs_by_task: &HashMap<
        String,
        crate::coordination::agent_org_task_handoffs::TaskExecutionHandoffReceipt,
    >,
) -> Vec<AgentOrgTaskRuntime> {
    let members_by_id: HashMap<String, AgentOrgContextMember> = context
        .members
        .iter()
        .cloned()
        .map(|member| (member.member_id.clone(), member))
        .collect();

    tasks
        .into_iter()
        .map(|summary| {
            let output_summary = summary.output.clone();
            let owner_member = summary
                .owner
                .as_ref()
                .and_then(|owner| members_by_id.get(owner).cloned());
            let owner_runtime = summary
                .owner
                .as_ref()
                .and_then(|owner| owner_runtimes.get(owner).cloned());
            let execution_mode = summary.execution_mode;
            let execution_handoff = handoffs_by_task.get(&summary.id).cloned();
            let task = Task {
                id: summary.id,
                org_run_id: context.run_id.clone(),
                activation_generation: summary.activation_generation,
                subject: summary.subject,
                description: summary.description,
                active_form: summary.active_form,
                owner: summary.owner,
                status: summary.status,
                execution_mode,
                blocks: summary.blocks,
                blocked_by: summary.blocked_by,
                // Eligibility, role and output summaries are available from
                // `task_list`; full metadata/output content is intentionally
                // detail-only and never crosses the polling bridge.
                metadata: None,
                output: None,
                failure_reason: summary.failure_reason,
                cancel_reason: summary.cancel_reason,
                created_by_participant_id: String::new(),
                source_turn_intent_id: String::new(),
                originating_message_id: None,
                replaces_task_id: summary.replaces_task_id,
                created_at: summary.created_at,
                updated_at: summary.updated_at,
            };

            AgentOrgTaskRuntime {
                task,
                description_truncated: summary.description_truncated,
                blocks_truncated: summary.blocks_truncated,
                blocked_by_truncated: summary.blocked_by_truncated,
                dependencies_satisfied: summary.dependencies_satisfied,
                execution_mode,
                output_summary,
                owner_member,
                owner_runtime,
                execution_handoff,
            }
        })
        .collect()
}

#[derive(Debug, Clone, Copy, Default)]
struct MemberTaskCounts {
    pending: usize,
    in_progress: usize,
    completed: usize,
}

fn task_counts_by_owner_with_connection(
    conn: &rusqlite::Connection,
    org_run_id: &str,
) -> Result<HashMap<String, MemberTaskCounts>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT owner,
                    COALESCE(SUM(CASE WHEN status='pending' THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN status='in_progress' THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN status='completed' THEN 1 ELSE 0 END), 0)
             FROM agent_org_runtime_tasks
             WHERE org_run_id=?1 AND owner IS NOT NULL
             GROUP BY owner",
        )
        .map_err(|err| err.to_string())?;
    let rows = stmt
        .query_map(params![org_run_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                MemberTaskCounts {
                    pending: row.get::<_, i64>(1)?.max(0) as usize,
                    in_progress: row.get::<_, i64>(2)?.max(0) as usize,
                    completed: row.get::<_, i64>(3)?.max(0) as usize,
                },
            ))
        })
        .map_err(|err| err.to_string())?;
    rows.map(|row| row.map_err(|err| err.to_string())).collect()
}

fn inbox_display_name(
    context: &AgentOrgRunContext,
    member_id: Option<&str>,
    system_fallback: &str,
) -> String {
    match member_id {
        Some(member_id) => context
            .participant_display_name(member_id)
            .unwrap_or_else(|| member_id.to_string()),
        None => system_fallback.to_string(),
    }
}

fn plain_payload_text(row: &AgentInboxRecord) -> String {
    match serde_json::from_str::<AgentMessage>(&row.payload_json) {
        Ok(AgentMessage::Plain { text, .. }) => text.trim().to_string(),
        _ => String::new(),
    }
}

fn inbox_display_text(row: &AgentInboxRecord, recipient_name: &str) -> String {
    let text = plain_payload_text(row);
    if row.sender_agent_id != USER_SENDER_ID || row.payload_kind != "plain" {
        return text;
    }
    if row.recipient_member_id.as_deref() == Some(COORDINATOR_MEMBER_ID) || text.starts_with('@') {
        return text;
    }
    format!("@{} {}", recipient_name.trim(), text)
        .trim()
        .to_string()
}

#[cfg(test)]
pub(super) fn enrich_inbox_rows(
    context: &AgentOrgRunContext,
    rows: Vec<AgentInboxRecord>,
) -> Vec<AgentOrgInboxRuntimeRow> {
    rows.into_iter()
        .map(|row| enrich_inbox_row(context, row))
        .collect()
}

pub(super) fn enrich_inbox_row(
    context: &AgentOrgRunContext,
    row: AgentInboxRecord,
) -> AgentOrgInboxRuntimeRow {
    let recipient_name = inbox_display_name(
        context,
        row.recipient_member_id.as_deref(),
        &row.recipient_agent_id,
    );
    let sender_fallback = if row.sender_agent_id == SYSTEM_SENDER_ID {
        "system"
    } else if row.sender_agent_id == USER_SENDER_ID {
        "User"
    } else {
        row.sender_agent_id.as_str()
    };
    let sender_name = inbox_display_name(context, row.sender_member_id.as_deref(), sender_fallback);
    let display_text = inbox_display_text(&row, &recipient_name);
    AgentOrgInboxRuntimeRow {
        recipient_name,
        sender_name,
        display_text,
        row,
    }
}

fn enrich_inbox_preview_rows(
    context: &AgentOrgRunContext,
    rows: Vec<AgentInboxPreviewRecord>,
) -> Vec<AgentOrgInboxPreviewRow> {
    rows.into_iter()
        .map(|row| {
            let recipient_name = inbox_display_name(
                context,
                row.recipient_member_id.as_deref(),
                &row.recipient_agent_id,
            );
            let sender_fallback = if row.sender_agent_id == SYSTEM_SENDER_ID {
                "system"
            } else if row.sender_agent_id == USER_SENDER_ID {
                "User"
            } else {
                row.sender_agent_id.as_str()
            };
            let sender_name =
                inbox_display_name(context, row.sender_member_id.as_deref(), sender_fallback);
            let mut display_text = row.display_preview.unwrap_or_default().trim().to_string();
            if row.sender_agent_id == USER_SENDER_ID
                && row.payload_kind == "plain"
                && row.recipient_member_id.as_deref() != Some(COORDINATOR_MEMBER_ID)
                && !display_text.starts_with('@')
            {
                display_text = format!("@{} {}", recipient_name.trim(), display_text)
                    .trim()
                    .to_string();
            }
            AgentOrgInboxPreviewRow {
                id: row.id,
                recipient_agent_id: row.recipient_agent_id,
                recipient_member_id: row.recipient_member_id,
                sender_agent_id: row.sender_agent_id,
                sender_member_id: row.sender_member_id,
                org_run_id: row.org_run_id,
                payload_kind: row.payload_kind,
                request_id: row.request_id,
                created_at: row.created_at,
                read_at: row.read_at,
                delivery_resolution: row.delivery_resolution,
                recipient_name,
                sender_name,
                display_text,
            }
        })
        .collect()
}

fn coordinator_member_view(
    context: &AgentOrgRunContext,
    runtime: Option<WorkerSessionRuntime>,
    task_counts: &HashMap<String, MemberTaskCounts>,
    inbox_counts: &[AgentInboxRecipientCounts],
    active_interventions: &HashMap<String, AgentMemberInterventionRecord>,
) -> Result<AgentOrgRunMemberView, String> {
    member_view_from_parts(
        AgentOrgMemberViewIdentity {
            member_id: COORDINATOR_MEMBER_ID.to_string(),
            name: context.coordinator_name.clone(),
            role: context.coordinator_role.clone(),
            agent_id: context.coordinator_agent_id.clone(),
            is_coordinator: true,
            writer_capable: true,
        },
        runtime,
        task_counts,
        inbox_counts,
        active_interventions,
    )
}

fn member_view(
    member: &AgentOrgContextMember,
    writer_capable: bool,
    runtime: Option<WorkerSessionRuntime>,
    task_counts: &HashMap<String, MemberTaskCounts>,
    inbox_counts: &[AgentInboxRecipientCounts],
    active_interventions: &HashMap<String, AgentMemberInterventionRecord>,
) -> Result<AgentOrgRunMemberView, String> {
    member_view_from_parts(
        AgentOrgMemberViewIdentity {
            member_id: member.member_id.clone(),
            name: member.name.clone(),
            role: member.role.clone(),
            agent_id: member.agent_id.clone(),
            is_coordinator: false,
            writer_capable,
        },
        runtime,
        task_counts,
        inbox_counts,
        active_interventions,
    )
}

struct AgentOrgMemberViewIdentity {
    member_id: String,
    name: String,
    role: String,
    agent_id: String,
    is_coordinator: bool,
    writer_capable: bool,
}

fn member_view_from_parts(
    identity: AgentOrgMemberViewIdentity,
    session_runtime: Option<WorkerSessionRuntime>,
    task_counts: &HashMap<String, MemberTaskCounts>,
    inbox_counts: &[AgentInboxRecipientCounts],
    active_interventions: &HashMap<String, AgentMemberInterventionRecord>,
) -> Result<AgentOrgRunMemberView, String> {
    let AgentOrgMemberViewIdentity {
        member_id,
        name,
        role,
        agent_id,
        is_coordinator,
        mut writer_capable,
    } = identity;
    // The coordinator is always a writer. Additional writer capability is
    // filled by the caller below from the immutable Run context.
    writer_capable |= is_coordinator;
    let (inbox_activity_count, unread_inbox_count) = inbox_counts
        .iter()
        // member_id is the only canonical Agent Org identity. A legacy row
        // without it remains visible in the bounded Run Inbox, but is not
        // copied onto every roster member that happens to share agent_id.
        .filter(|counts| counts.recipient_member_id.as_deref() == Some(member_id.as_str()))
        .fold((0usize, 0usize), |(activity, unread), counts| {
            (
                activity.saturating_add(counts.activity_count),
                unread.saturating_add(counts.unread_count),
            )
        });
    let task_owner_id = if is_coordinator {
        COORDINATOR_MEMBER_ID
    } else {
        member_id.as_str()
    };
    let counts = task_counts.get(task_owner_id).copied().unwrap_or_default();
    let pending_task_count = counts.pending;
    let in_progress_task_count = counts.in_progress;
    let active_task_count = pending_task_count + in_progress_task_count;
    let completed_task_count = counts.completed;
    let intervention = match session_runtime
        .as_ref()
        .and_then(|runtime| runtime.intervention.clone())
    {
        Some(record) => Some(record),
        None => active_interventions.get(&member_id).cloned(),
    };
    let activity = intervention.as_ref().map(|record| AgentOrgMemberActivity {
        kind: match record.status {
            MemberInterventionStatus::YieldRequested if record.yield_timed_out_at.is_some() => {
                AgentOrgMemberActivityKind::YieldTimeout
            }
            MemberInterventionStatus::YieldRequested => AgentOrgMemberActivityKind::Yielding,
            MemberInterventionStatus::Active | MemberInterventionStatus::ReturnRequested => {
                if record.original_task_id.is_some() && record.original_turn_intent_id.is_some() {
                    AgentOrgMemberActivityKind::UserIntervention
                } else {
                    AgentOrgMemberActivityKind::SideQuest
                }
            }
            MemberInterventionStatus::Cleared | MemberInterventionStatus::Failed => {
                unreachable!("Run View only receives active intervention receipts")
            }
        },
        source: "direct_member",
        intervention_receipt_id: record.intervention_receipt_id.clone(),
    });
    let queued_user_directed_count = intervention
        .as_ref()
        .map(|record| record.queued_user_directed_count)
        .unwrap_or(0);

    Ok(AgentOrgRunMemberView {
        member_id,
        name,
        role,
        agent_id,
        is_coordinator,
        writer_capable,
        session_runtime,
        unread_inbox_count,
        inbox_activity_count,
        active_task_count,
        pending_task_count,
        in_progress_task_count,
        completed_task_count,
        queued_user_directed_count,
        activity,
        intervention,
    })
}

fn bounded_run_view_inbox_counts(
    recent_rows: &[AgentInboxPreviewRecord],
    unread_counts: &[AgentInboxUnreadRecipientCounts],
) -> Vec<AgentInboxRecipientCounts> {
    let mut counts_by_recipient: HashMap<(String, Option<String>), AgentInboxRecipientCounts> =
        HashMap::new();

    // Activity is intentionally the bounded Run View window, not an
    // unbounded lifetime total. Full history belongs to the paginated Inbox
    // surface; this projection is polled every few seconds.
    for row in recent_rows {
        let key = (
            row.recipient_agent_id.clone(),
            row.recipient_member_id.clone(),
        );
        let counts = counts_by_recipient
            .entry(key)
            .or_insert_with(|| AgentInboxRecipientCounts {
                recipient_agent_id: row.recipient_agent_id.clone(),
                recipient_member_id: row.recipient_member_id.clone(),
                activity_count: 0,
                unread_count: 0,
            });
        counts.activity_count = counts.activity_count.saturating_add(1);
    }

    // Unread totals must remain exact even when an old unread row falls
    // outside the recent activity window, so merge the unread-only index
    // query separately.
    for unread in unread_counts {
        let key = (
            unread.recipient_agent_id.clone(),
            unread.recipient_member_id.clone(),
        );
        let counts = counts_by_recipient
            .entry(key)
            .or_insert_with(|| AgentInboxRecipientCounts {
                recipient_agent_id: unread.recipient_agent_id.clone(),
                recipient_member_id: unread.recipient_member_id.clone(),
                activity_count: 0,
                unread_count: 0,
            });
        counts.unread_count = unread.unread_count;
    }

    let mut counts = counts_by_recipient.into_values().collect::<Vec<_>>();
    counts.sort_by(|left, right| {
        left.recipient_member_id
            .cmp(&right.recipient_member_id)
            .then_with(|| left.recipient_agent_id.cmp(&right.recipient_agent_id))
    });
    counts
}
