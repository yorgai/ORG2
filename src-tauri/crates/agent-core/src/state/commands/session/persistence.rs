//! Persistence commands for session data.

use std::sync::Arc;

use crate::coordination::agent_org_ownership::AgentOrgTeamOwnership as AgentOrgSessionDeletePlan;
use crate::coordination::agent_org_runs::AgentOrgRunStore;
use crate::interaction::plan_approval::persistence::PlanApprovalStore;
use crate::persistence::db_helpers as shared;
use crate::persistence::session_snapshots;
use crate::session::persistence as session_persistence;
use crate::session::SessionListFilter;
use crate::state::control_flow::CancelReason;
use crate::state::{AgentAppState, AgentSession};
use crate::tools::file_history;
use database::db::{get_connection, with_sessions_writer};
use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;

use super::common::review_session_ids;

/// Load conversation messages for a session.
#[tauri::command]
pub async fn agent_load_messages(session_id: String) -> Result<Vec<serde_json::Value>, String> {
    shared::spawn_blocking_cmd(move || {
        let messages = session_persistence::load_messages(&session_id)?;
        messages.into_iter().map(shared::to_json_value).collect()
    })
    .await
}

/// Get a single session record by ID.
#[tauri::command]
pub async fn agent_get_session(session_id: String) -> Result<Option<serde_json::Value>, String> {
    shared::spawn_blocking_cmd(move || {
        session_persistence::get_session(&session_id)?
            .map(shared::to_json_value)
            .transpose()
    })
    .await
}

/// List all sessions from both OS and SDE, merged into one array.
#[tauri::command]
pub async fn agent_list_all_sessions() -> Result<Vec<serde_json::Value>, String> {
    shared::spawn_blocking_cmd(move || {
        let filter = SessionListFilter::default();
        let records = session_persistence::list_sessions(&filter)?;
        records.into_iter().map(shared::to_json_value).collect()
    })
    .await
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteSessionReceipt {
    pub deleted_session_ids: Vec<String>,
}

/// Delete a session and all related data.
#[tauri::command]
pub async fn agent_delete_session(
    state: tauri::State<'_, AgentAppState>,
    session_id: String,
) -> Result<DeleteSessionReceipt, String> {
    let planned_session_id = session_id.clone();
    let plan = tokio::task::spawn_blocking(move || {
        let conn = get_connection().map_err(|err| err.to_string())?;
        load_agent_org_session_delete_plan(&conn, &planned_session_id)
    })
    .await
    .map_err(|err| format!("session deletion planning worker failed: {err}"))??;

    let Some(plan) = plan else {
        crate::memory::background::cancel_memory_jobs_for_session(&session_id);
        let deleted_session_id = session_id.clone();
        tokio::task::spawn_blocking(move || {
            session_persistence::delete_session(&deleted_session_id).map_err(|err| err.to_string())
        })
        .await
        .map_err(|err| format!("session deletion worker failed: {err}"))??;
        state.remove_session(&session_id).await;
        return Ok(DeleteSessionReceipt {
            deleted_session_ids: vec![session_id],
        });
    };

    Err(format!(
        "agent_org_team_delete_required: session {} belongs to Agent Org Team {} and must use Team Delete",
        session_id, plan.run_id
    ))
}

/// Permanently delete one already-Archived Team after its bounded runtime
/// teardown receipt proves every captured owner is quiesced.
#[tauri::command]
pub async fn agent_org_delete_team(
    state: tauri::State<'_, AgentAppState>,
    session_id: String,
) -> Result<DeleteSessionReceipt, String> {
    crate::coordination::agent_org_runs::require_agent_org_enabled()?;
    let planned_session_id = session_id.clone();
    let plan = tokio::task::spawn_blocking(move || {
        let conn = get_connection().map_err(|err| err.to_string())?;
        load_agent_org_session_delete_plan(&conn, &planned_session_id)
    })
    .await
    .map_err(|err| format!("Team deletion planning worker failed: {err}"))??
    .ok_or_else(|| format!("agent_org_team_not_found: session {session_id} has no Team"))?;

    validate_agent_org_delete_ready(&plan)?;
    let runtime_sessions = acquire_agent_org_runtime_delete_fence(&state, &plan).await?;
    let planned_session_ids = plan
        .sessions
        .iter()
        .map(|node| node.session_id.clone())
        .collect::<Vec<_>>();
    let background_blockers =
        crate::tools::impls::coding::exec::registry::execution_blockers_for_sessions(
            &planned_session_ids,
            16,
        );
    if !background_blockers.is_empty() {
        for (_, session) in &runtime_sessions {
            session.clear_team_delete_runtime_fence();
        }
        let evidence = background_blockers
            .iter()
            .map(|job| {
                format!(
                    "{}:{}:{}:{}",
                    job.session_id, job.kind, job.handle, job.execution_state
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        return Err(format!(
            "team_background_jobs_not_quiesced: Team {} still owns executing background jobs: {evidence}",
            plan.run_id
        ));
    }

    let delete_plan = plan.clone();
    let delete_result =
        tokio::task::spawn_blocking(move || delete_agent_org_session_hierarchy(&delete_plan))
            .await
            .map_err(|err| format!("Agent Org Team deletion worker failed: {err}"))
            .and_then(|result| result);
    let receipt = match delete_result {
        Ok(receipt) => receipt,
        Err(error) => {
            for (_, session) in &runtime_sessions {
                session.clear_team_delete_runtime_fence();
            }
            return Err(error);
        }
    };

    let purged_jobs =
        crate::tools::impls::coding::exec::registry::purge_deleted_sessions(
            &receipt.deleted_session_ids,
        )
        .map_err(|error| {
            format!(
                "team_deleted_but_background_job_purge_failed: Team {} was deleted from the database but its in-memory job registry could not be purged: {error}",
                plan.run_id
            )
        })?;
    if purged_jobs.live_jobs > 0 || purged_jobs.tombstones > 0 {
        tracing::info!(
            live_jobs = purged_jobs.live_jobs,
            tombstones = purged_jobs.tombstones,
            "Team Delete purged background-job registry state for physically deleted sessions"
        );
    }
    state.remove_sessions(&receipt.deleted_session_ids).await;
    let forgotten_memory_job_seals = receipt
        .deleted_session_ids
        .iter()
        .filter(|session_id| {
            crate::memory::background::forget_memory_job_seal_for_deleted_session(session_id)
        })
        .count();
    if forgotten_memory_job_seals > 0 {
        tracing::info!(
            forgotten_memory_job_seals,
            "Team Delete released memory-job seals for physically deleted sessions"
        );
    }
    if let Some(app_handle) = state.app_handle.as_ref() {
        for deleted_session_id in &receipt.deleted_session_ids {
            crate::bus::event_pipeline_bridge::evict_session(app_handle, deleted_session_id);
        }
    }
    Ok(receipt)
}

fn load_agent_org_session_delete_plan(
    conn: &Connection,
    session_id: &str,
) -> Result<Option<AgentOrgSessionDeletePlan>, String> {
    crate::coordination::agent_org_ownership::resolve_team_for_session(conn, session_id)
}

/// OrgTrack rows whose lifetime is exactly the owning Session's history.
///
/// Team Delete already removes the canonical Agent Org/EventStore/Session
/// rows in one transaction. These projections live in the same SQLite file,
/// so leaving them behind would retain the deleted Team's file-edit and
/// resource-access history even though the UI promises that all Team history
/// is permanently removed. Shared resource identities/revision clocks are not
/// listed here: they can be referenced by other Sessions and contain no
/// deleted Session id.
const AGENT_ORG_SESSION_HISTORY_TABLES: &[&str] = &[
    "orgtrack_core_activities",
    "orgtrack_core_file_changes",
    "orgtrack_core_edit_artifacts",
    "orgtrack_core_diff_chunks",
    "orgtrack_core_final_diffs",
    "orgtrack_core_session_checkpoints",
    "orgtrack_core_checkpoint_file_states",
    "orgtrack_core_session_signals",
    "orgtrack_core_interaction_import_checkpoints",
    "orgtrack_core_session_usage",
];

fn delete_agent_org_session_history_with_connection(
    conn: &Connection,
    session_id: &str,
) -> rusqlite::Result<()> {
    for table in AGENT_ORG_SESSION_HISTORY_TABLES {
        conn.execute(
            &format!("DELETE FROM {table} WHERE session_id=?1"),
            [session_id],
        )?;
    }
    conn.execute(
        "DELETE FROM orgtrack_core_resource_interactions
         WHERE session_id=?1 OR source_session_id=?1",
        [session_id],
    )?;
    conn.execute(
        "DELETE FROM orgtrack_core_session_actors
         WHERE session_id=?1 OR source_session_id=?1 OR transcript_session_id=?1",
        [session_id],
    )?;
    conn.execute(
        "DELETE FROM orgtrack_core_sessions
         WHERE session_id=?1 OR source_session_id=?1",
        [session_id],
    )?;
    conn.execute(
        "DELETE FROM orgtrack_core_commit_links
         WHERE EXISTS (
             SELECT 1
             FROM json_each(orgtrack_core_commit_links.payload_json, '$.sessionIds')
             WHERE json_each.value=?1
         )",
        [session_id],
    )?;
    Ok(())
}

fn first_agent_org_session_history_residual(
    conn: &Connection,
    session_id: &str,
) -> rusqlite::Result<Option<String>> {
    for table in AGENT_ORG_SESSION_HISTORY_TABLES {
        let exists = conn.query_row(
            &format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE session_id=?1)"),
            [session_id],
            |row| row.get::<_, bool>(0),
        )?;
        if exists {
            return Ok(Some((*table).to_string()));
        }
    }
    for (table, predicate) in [
        (
            "orgtrack_core_resource_interactions",
            "session_id=?1 OR source_session_id=?1",
        ),
        (
            "orgtrack_core_session_actors",
            "session_id=?1 OR source_session_id=?1 OR transcript_session_id=?1",
        ),
        (
            "orgtrack_core_sessions",
            "session_id=?1 OR source_session_id=?1",
        ),
        (
            "orgtrack_core_commit_links",
            "EXISTS (
                 SELECT 1
                 FROM json_each(orgtrack_core_commit_links.payload_json, '$.sessionIds')
                 WHERE json_each.value=?1
             )",
        ),
    ] {
        let exists = conn.query_row(
            &format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE {predicate})"),
            [session_id],
            |row| row.get::<_, bool>(0),
        )?;
        if exists {
            return Ok(Some(table.to_string()));
        }
    }
    Ok(None)
}

fn validate_agent_org_delete_ready(plan: &AgentOrgSessionDeletePlan) -> Result<(), String> {
    let conn = get_connection().map_err(|error| error.to_string())?;
    validate_agent_org_delete_ready_with_connection(&conn, plan)
}

fn validate_agent_org_delete_ready_with_connection(
    conn: &Connection,
    plan: &AgentOrgSessionDeletePlan,
) -> Result<(), String> {
    if plan.run_status != crate::coordination::agent_org_runs::AgentOrgRunStatus::Archived {
        return Err(format!(
            "team_delete_requires_archived: Team {} status is {}",
            plan.run_id, plan.run_status
        ));
    }
    let receipt_id = plan.archive_receipt_id.as_deref().ok_or_else(|| {
        format!(
            "team_runtime_not_quiesced: Team {} has no Archive receipt",
            plan.run_id
        )
    })?;
    if plan.archived_at.is_none() {
        return Err(format!(
            "team_runtime_not_quiesced: Team {} has no Archive timestamp",
            plan.run_id
        ));
    }
    let summary = crate::coordination::agent_org_archive::summary_for_run_with_connection(
        conn,
        &plan.run_id,
    )?
    .ok_or_else(|| {
        format!(
            "team_runtime_not_quiesced: Team {} has no teardown receipt",
            plan.run_id
        )
    })?;
    if summary.receipt_id != receipt_id
        || summary.status != crate::coordination::agent_org_archive::ArchiveTeardownStatus::Quiesced
        || summary.retained_runtime_count != 0
    {
        return Err(format!(
            "team_runtime_not_quiesced: Team {} Archive teardown is {} with {} retained runtime(s)",
            plan.run_id,
            summary.status.as_str(),
            summary.retained_runtime_count
        ));
    }
    Ok(())
}

async fn agent_org_runtime_sessions(
    state: &AgentAppState,
    plan: &AgentOrgSessionDeletePlan,
) -> Vec<(String, Arc<AgentSession>)> {
    let sessions = state.sessions.lock().await;
    plan.sessions
        .iter()
        .filter_map(|node| {
            sessions
                .get(&node.session_id)
                .cloned()
                .map(|session| (node.session_id.clone(), session))
        })
        .collect()
}

async fn agent_org_runtime_blockers(
    runtime_sessions: &[(String, Arc<AgentSession>)],
) -> Vec<String> {
    let mut blockers = Vec::new();
    for (session_id, session) in runtime_sessions {
        let runtime_lease = session.runtime_lease_identity().await;
        let scheduler_processing = session.scheduler.is_processing();
        let pending_count = session.scheduler.pending_count();
        let active_turn = session.active_turn.lock().await.is_some();
        if runtime_lease.is_some() || active_turn || scheduler_processing || pending_count > 0 {
            blockers.push(format!(
                "{session_id}(runtime_lease={},active_turn={active_turn},scheduler_processing={scheduler_processing},pending={pending_count})",
                runtime_lease.is_some()
            ));
        }
    }
    blockers
}

async fn acquire_agent_org_runtime_delete_fence(
    state: &AgentAppState,
    plan: &AgentOrgSessionDeletePlan,
) -> Result<Vec<(String, Arc<AgentSession>)>, String> {
    let runtime_sessions = agent_org_runtime_sessions(state, plan).await;
    for (_, session) in &runtime_sessions {
        session.begin_team_delete_runtime_fence().await;
    }
    let blockers = agent_org_runtime_blockers(&runtime_sessions).await;
    if blockers.is_empty() {
        Ok(runtime_sessions)
    } else {
        for (_, session) in &runtime_sessions {
            session.clear_team_delete_runtime_fence();
        }
        Err(format!(
            "team_runtime_not_quiesced: Team {} still owns in-memory runtime state: {}",
            plan.run_id,
            blockers.join(", ")
        ))
    }
}

fn delete_agent_org_session_hierarchy(
    expected_plan: &AgentOrgSessionDeletePlan,
) -> Result<DeleteSessionReceipt, String> {
    for node in &expected_plan.sessions {
        session_persistence::prepare_session_delete(&node.session_id)
            .map_err(|err| format!("prepare session {} for deletion: {err}", node.session_id))?;
    }

    let outcome = with_sessions_writer(|| {
        let mut conn = get_connection().map_err(|err| err.to_string())?;
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|err| err.to_string())?;
        let current_plan = load_agent_org_session_delete_plan(&tx, &expected_plan.root_session_id)?
            .ok_or_else(|| {
                format!(
                    "Refusing to delete Agent Org run {}: root ownership changed before deletion",
                    expected_plan.run_id
                )
            })?;
        if current_plan != *expected_plan {
            return Err(format!(
                "Refusing to delete Agent Org run {}: session hierarchy or status changed before deletion",
                expected_plan.run_id
            ));
        }
        validate_agent_org_delete_ready_with_connection(&tx, &current_plan)?;

        // Run-owned coordination rows include Linked Inbox children whose
        // authority intentionally RESTRICTs deletion of their parent Turn and
        // Inbox. Remove that run-owned causal graph first; the Run store does
        // so leaf-first, while this same transaction still owns the complete
        // Team plan and can roll everything back on any later Session error.
        let outcome = AgentOrgRunStore::delete_by_id_with_connection(&tx, &expected_plan.run_id)?;
        if !outcome.deleted() {
            return Err(format!(
                "Refusing to commit Agent Org run {} deletion: run row disappeared during deletion",
                expected_plan.run_id
            ));
        }

        for node in &expected_plan.sessions {
            delete_agent_org_session_history_with_connection(&tx, &node.session_id).map_err(
                |err| {
                    format!(
                        "delete Session history {} for Team {}: {err}",
                        node.session_id, expected_plan.run_id
                    )
                },
            )?;
            session_persistence::delete_session_with_connection(&tx, &node.session_id)
                .map_err(|err| format!("delete session {}: {err}", node.session_id))?;
        }
        ensure_agent_org_hierarchy_absent(&tx, expected_plan)?;
        tx.commit().map_err(|err| err.to_string())?;
        Ok::<_, String>(outcome)
    })?;

    for node in &expected_plan.sessions {
        session_persistence::finish_session_delete(&node.session_id);
    }
    AgentOrgRunStore::finish_delete(&expected_plan.run_id, outcome);

    Ok(DeleteSessionReceipt {
        deleted_session_ids: expected_plan
            .sessions
            .iter()
            .map(|node| node.session_id.clone())
            .collect(),
    })
}

fn ensure_agent_org_hierarchy_absent(
    conn: &Connection,
    plan: &AgentOrgSessionDeletePlan,
) -> Result<(), String> {
    for node in &plan.sessions {
        let residual: Option<String> = conn
            .query_row(
                "SELECT session_id
                 FROM agent_sessions
                 WHERE session_id=?1 OR parent_session_id=?1
                 ORDER BY session_id
                 LIMIT 1",
                [&node.session_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|err| err.to_string())?;
        if let Some(session_id) = residual {
            return Err(format!(
                "Refusing to commit Agent Org run {} deletion: residual session hierarchy row {session_id} references deleted session {}",
                plan.run_id, node.session_id
            ));
        }
        if let Some(table) = first_agent_org_session_history_residual(conn, &node.session_id)
            .map_err(|err| err.to_string())?
        {
            return Err(format!(
                "Refusing to commit Agent Org run {} deletion: residual Session history in {table} references deleted session {}",
                plan.run_id, node.session_id
            ));
        }
    }
    let run_exists = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM agent_org_runtime_runs WHERE id=?1)",
            [&plan.run_id],
            |row| row.get::<_, bool>(0),
        )
        .map_err(|err| err.to_string())?;
    if run_exists {
        return Err(format!(
            "Refusing to commit Agent Org run {} deletion: run row still exists",
            plan.run_id
        ));
    }
    Ok(())
}

/// Clear all messages for a session.
#[tauri::command]
pub async fn agent_clear_messages(session_id: String) -> Result<i64, String> {
    shared::spawn_blocking_cmd(move || session_persistence::clear_messages(&session_id)).await
}

/// Truncate messages at or after an anchor message.
///
/// The anchor is resolved to a `(sequence, created_at)` pair **from the
/// anchor row itself** — `sequence` drives the transcript truncation
/// (the only safe coordinate; see `truncate_messages_from_sequence`),
/// while the row's own `created_at` rewinds the timestamp-keyed side
/// stores (file-history, session snapshots). Resolution is fail-loud:
/// if neither `message_id` nor `created_at` matches an existing row, the
/// command errors instead of guessing — a silently-wrong anchor is how
/// the 2026-06-11 transcript wipe happened.
///
/// When `revert_files` is true (default behavior for edit/regenerate flows),
/// also rewinds the per-session file-history so edited files are restored to
/// their pre-turn state. When false (e.g. "continue with changes"), file
/// contents are left as-is and only message rows are dropped.
#[tauri::command]
pub async fn agent_truncate_after_message(
    state: tauri::State<'_, AgentAppState>,
    session_id: String,
    created_at: String,
    revert_files: Option<bool>,
    message_id: Option<String>,
) -> Result<i64, String> {
    if let Some(session) = state.get_session(&session_id).await {
        session.scheduler.invalidate_pending();
        session
            .cancel_active_turn(CancelReason::ModeSwitchAbort)
            .await;
    }

    let should_revert = revert_files.unwrap_or(true);
    tokio::task::spawn_blocking(move || -> Result<i64, String> {
        let anchor = match message_id.as_deref() {
            Some(message_id) => session_persistence::message_anchor(&session_id, message_id)
                .map_err(|err| err.to_string())?
                .ok_or_else(|| {
                    format!(
                        "Refusing to truncate session {session_id}: anchor message {message_id} not found"
                    )
                })?,
            None => session_persistence::anchor_at_or_after_created_at(&session_id, &created_at)
                .map_err(|err| err.to_string())?
                .ok_or_else(|| {
                    format!(
                        "Refusing to truncate session {session_id}: no message at or after {created_at}"
                    )
                })?,
        };
        let review_session_ids = review_session_ids(&session_id);
        if should_revert {
            for review_session_id in &review_session_ids {
                let stats = file_history::rewind_to_message(review_session_id, &anchor.created_at)
                    .map_err(|err| format!("file-history rewind failed for {review_session_id}: {err}"))?;
                tracing::info!(
                    "[agent_truncate] file-history rewind: session={} restored={} deleted={} skipped={} failed={}",
                    review_session_id,
                    stats.restored,
                    stats.deleted,
                    stats.skipped_unchanged,
                    stats.failed,
                );
            }
        }

        for review_session_id in &review_session_ids {
            session_snapshots::truncate_snapshots_after(review_session_id, &anchor.created_at)
                .map_err(|err| err.to_string())?;
        }
        PlanApprovalStore::delete_by_session(&session_id).map_err(|err| err.to_string())?;
        session_persistence::truncate_messages_from_sequence(&session_id, anchor.sequence)
            .map_err(|err| err.to_string())
    })
    .await
    .map_err(|err| err.to_string())?
}

/// Check whether rewinding to a message would modify files on disk. Used by
/// the frontend to decide whether to show a "keep or revert changes" dialog
/// before regenerating / editing a past message.
#[tauri::command]
pub async fn agent_check_snapshot_changes(
    session_id: String,
    created_at: String,
) -> Result<bool, String> {
    tokio::task::spawn_blocking(move || {
        for review_session_id in review_session_ids(&session_id) {
            let has_changes =
                file_history::has_changes_after_message(&review_session_id, &created_at).map_err(
                    |e| format!("file-history check failed for {review_session_id}: {e}"),
                )?;
            if has_changes {
                return Ok(true);
            }
            let modified_files = session_snapshots::get_session_modified_files_after(
                &review_session_id,
                &created_at,
            )
            .map_err(|e| format!("file-change check failed for {review_session_id}: {e}"))?;
            if !modified_files.is_empty() {
                return Ok(true);
            }
        }
        Ok(false)
    })
    .await
    .map_err(|err| format!("Task error: {}", err))?
}

/// Update session status.
#[tauri::command]
pub async fn agent_update_session_status(
    session_id: String,
    status: String,
) -> Result<bool, String> {
    // Reject unknown status strings instead of silently downgrading to
    // `Idle` — that previously made stuck-state rows invisible (a row
    // wedged in a malformed terminal state would silently look idle to
    // the lifecycle manager).
    let parsed = crate::session::SessionStatus::parse(&status)
        .ok_or_else(|| format!("Unknown session status: {status:?}"))?;
    shared::spawn_blocking_cmd(move || session_persistence::update_status(&session_id, parsed))
        .await
}

/// Return the `workspace_path` for a session. Used by the frontend to resolve
/// file paths for the WorkStation diff view when opening a session's changes
/// from the Group Chat feed.
#[tauri::command]
pub async fn agent_get_session_workspace_path(
    session_id: String,
) -> Result<Option<String>, String> {
    shared::spawn_blocking_cmd(move || {
        crate::persistence::session_snapshots::get_session_workspace_path(&session_id)
    })
    .await
}

/// Save (upsert) a session record.
#[tauri::command]
pub async fn agent_save_session(session: serde_json::Value) -> Result<(), String> {
    let record: session_persistence::UnifiedSessionRecord = serde_json::from_value(session)
        .map_err(|err| format!("Failed to deserialize session: {}", err))?;
    shared::spawn_blocking_cmd(move || session_persistence::upsert_session(&record)).await
}

#[tauri::command]
pub async fn agent_link_session_to_work_item(
    app: tauri::AppHandle,
    session_id: String,
    org_id: Option<String>,
    project_slug: String,
    work_item_id: String,
    agent_role: Option<String>,
) -> Result<serde_json::Value, String> {
    let updated_record = tokio::task::spawn_blocking(move || {
        link_session_to_work_item_sync(
            &session_id,
            org_id.as_deref(),
            &project_slug,
            &work_item_id,
            agent_role.as_deref(),
        )
    })
    .await
    .map_err(|err| err.to_string())??;

    {
        use tauri::Emitter;
        let ts = chrono::Utc::now().to_rfc3339();
        let _ = app.emit(
            project_management::projects::events::DATA_CHANGED_EVENT,
            &ts,
        );
    }

    shared::to_json_value(updated_record).map_err(|err| err.to_string())
}

/// `Track this` (orgtrack/v1 §7.2, Build→Project) and
/// `Convert to Project` (Plan→Project): switch the session onto the
/// Project product mode, derive the runtime exec mode the same way the
/// composer picker does (project → build), invalidate Plan mode's
/// snapshot/restore state, and create-or-replay the root WorkItem from
/// the already-recorded first user input. Earlier turns stay untouched
/// as provenance. Returns `{ productMode, agentExecMode, workItemId }`.
#[tauri::command]
pub async fn agent_track_session_as_project(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::state::AgentAppState>,
    session_id: String,
) -> Result<serde_json::Value, String> {
    let sid = session_id.clone();
    let (work_item_id, exec_mode) = tokio::task::spawn_blocking(move || {
        let record = session_persistence::get_session(&sid)
            .map_err(|err| err.to_string())?
            .ok_or_else(|| format!("Session not found: {sid}"))?;

        // Same derivation the ModePill applies: Project pins the exec
        // mode to Build (a read-only Plan session would otherwise keep
        // its deny layer while claiming to do project work).
        let exec_mode = crate::session::AgentExecMode::Build;
        session_persistence::update_mode_axes(&sid, "project", exec_mode.as_str())
            .map_err(|err| format!("track session: set Project mode axes: {err}"))?;

        // Root creation at conversion time, from the recorded first
        // user input. An empty session converts mode-only; the
        // first-submission bootstrap covers the root later.
        let content = record.user_input.clone().unwrap_or_default();
        let work_item_id = if record.work_item_id.is_some() {
            record.work_item_id
        } else if content.trim().is_empty() {
            None
        } else {
            super::message::project_bootstrap::bootstrap_root_work_item(&sid, &content)?
        };
        Ok::<_, String>((work_item_id, exec_mode))
    })
    .await
    .map_err(|err| err.to_string())??;

    // Convert to Project invalidates the Plan snapshot/restore state so
    // a pending approval can't bounce later turns back to the old mode.
    if let Some(session) = state.get_session(&session_id).await {
        let had_slot = session.plan_slot_cache.get(&session_id).is_some();
        let _ = session.pre_plan_mode_cache.take(&session_id);
        session.plan_slot_cache.clear(&session_id);
        if had_slot {
            crate::bus::broadcast_event(
                "agent:exit_plan_mode",
                serde_json::json!({
                    "sessionId": &session_id,
                    "source": "convert_to_project",
                    "nextMode": exec_mode.as_str(),
                }),
            );
        }
    }

    {
        use tauri::Emitter;
        let ts = chrono::Utc::now().to_rfc3339();
        let _ = app.emit(
            project_management::projects::events::DATA_CHANGED_EVENT,
            &ts,
        );
    }

    Ok(serde_json::json!({
        "productMode": "project",
        "agentExecMode": exec_mode.as_str(),
        "workItemId": work_item_id,
    }))
}

fn link_session_to_work_item_sync(
    session_id: &str,
    org_id: Option<&str>,
    project_slug: &str,
    work_item_id: &str,
    agent_role: Option<&str>,
) -> Result<session_persistence::UnifiedSessionRecord, String> {
    let session = session_persistence::get_session(session_id)
        .map_err(|err| err.to_string())?
        .ok_or_else(|| format!("Session not found: {session_id}"))?;

    let project = project_management::projects::io::read_project(project_slug)
        .map_err(|err| format!("Failed to read project {project_slug}: {err}"))?;
    if let Some(supplied_org_id) = org_id {
        if supplied_org_id != project.meta.org_id {
            return Err(format!(
                "Project {project_slug} belongs to org {}, not {}",
                project.meta.org_id, supplied_org_id
            ));
        }
    }

    if session.project_slug.as_deref() != Some(project_slug)
        || session.work_item_id.as_deref() != Some(work_item_id)
    {
        if let (Some(old_project_slug), Some(old_work_item_id)) = (
            session.project_slug.as_deref(),
            session.work_item_id.as_deref(),
        ) {
            if old_project_slug != project_slug || old_work_item_id != work_item_id {
                remove_linked_session_from_work_item(
                    old_project_slug,
                    old_work_item_id,
                    session_id,
                )?;
            }
        }
    }

    session_persistence::update_work_item_link(
        session_id,
        &project.meta.org_id,
        Some(&project.meta.id),
        Some(&project.meta.name),
        project_slug,
        work_item_id,
        agent_role,
    )
    .map_err(|err| err.to_string())?
    .then_some(())
    .ok_or_else(|| format!("Session not found: {session_id}"))?;

    session_persistence::linked_work_item::upsert_linked_session_on_work_item(
        project_slug,
        work_item_id,
        &session,
        agent_role,
    )?;

    session_persistence::get_session(session_id)
        .map_err(|err| err.to_string())?
        .ok_or_else(|| format!("Session not found after link: {session_id}"))
}

fn remove_linked_session_from_work_item(
    project_slug: &str,
    work_item_id: &str,
    session_id: &str,
) -> Result<(), String> {
    project_management::projects::io::update_work_item_atomic(
        project_slug,
        work_item_id,
        |frontmatter, _body| {
            let original_len = frontmatter.linked_sessions.len();
            frontmatter
                .linked_sessions
                .retain(|linked| linked.session_id != session_id);
            if frontmatter.linked_sessions.len() != original_len {
                frontmatter.updated_at = chrono::Utc::now().to_rfc3339();
            }
            Ok(())
        },
    )
    .map(|_| ())
}

#[cfg(test)]
#[path = "persistence_tests.rs"]
mod tests;
