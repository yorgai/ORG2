use rusqlite::{params, Connection};

use database::db::{get_connection, with_sessions_writer};

use super::super::quiescence::load_and_assess;
use super::super::{AgentOrgQuiescenceAssessment, AgentOrgQuiescenceDecision, AgentOrgRunStatus};
use super::AgentOrgRunStore;

impl AgentOrgRunStore {
    pub fn assess_run_quiescence(run_id: &str) -> Result<AgentOrgQuiescenceAssessment, String> {
        let mut conn = get_connection().map_err(|err| err.to_string())?;
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Deferred)
            .map_err(|err| err.to_string())?;
        let assessment = load_and_assess(&tx, run_id)?;
        tx.commit().map_err(|err| err.to_string())?;
        Ok(assessment)
    }

    /// Atomically commit the only automatic lifecycle transition managed by
    /// run quiescence: Running to Idle. Both snapshot certificates are required
    /// so a stale finalizer or watchdog pass cannot idle a newer activation or
    /// newer work graph.
    pub fn try_transition_working_to_idle(
        run_id: &str,
        expected_generation: i64,
        expected_work_revision: i64,
    ) -> Result<bool, String> {
        let changed = with_sessions_writer(|| -> Result<bool, String> {
            let mut conn = get_connection().map_err(|err| err.to_string())?;
            let tx = conn
                .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
                .map_err(|err| err.to_string())?;
            let assessment = load_and_assess(&tx, run_id)?;
            if assessment.facts.run_status != Some(AgentOrgRunStatus::Running)
                || assessment.facts.activation_generation != Some(expected_generation)
                || assessment
                    .facts
                    .progress
                    .as_ref()
                    .map(|progress| progress.work_revision)
                    != Some(expected_work_revision)
                || assessment.decision != AgentOrgQuiescenceDecision::Quiescent
            {
                tx.commit().map_err(|err| err.to_string())?;
                return Ok(false);
            }
            let now = chrono::Utc::now().to_rfc3339();
            let completion_summary = assessment
                .facts
                .completion_certificate
                .as_ref()
                .map(|certificate| certificate.summary.as_str());
            let last_activity_outcome = assessment
                .facts
                .completion_certificate
                .as_ref()
                .map(|certificate| certificate.outcome.last_activity_outcome());
            let changed = tx
                .execute(
                    "UPDATE agent_org_runtime_runs
                         SET status='idle',
                             summary=COALESCE(?1, summary),
                             last_activity_outcome=?2,
                             updated_at=?3,
                             idled_at=?3
                         WHERE id=?4 AND status='running'
                           AND activation_generation=?5
                           AND EXISTS (
                               SELECT 1 FROM agent_org_runtime_run_progress progress
                               WHERE progress.org_run_id=agent_org_runtime_runs.id
                                 AND progress.work_revision=?6
                           )",
                    params![
                        completion_summary,
                        last_activity_outcome,
                        &now,
                        run_id,
                        expected_generation,
                        expected_work_revision,
                    ],
                )
                .map_err(|err| err.to_string())?;
            tx.commit().map_err(|err| err.to_string())?;
            Ok(changed == 1)
        })?;
        if changed {
            crate::coordination::agent_org_run_events::notify_agent_org_run_changed(run_id);
        }
        Ok(changed)
    }

    /// Read the canonical quiescence facts and decision from an existing
    /// connection or read transaction. Run View and task-list projections use
    /// this to keep all of their independently-shaped rows on one SQLite
    /// snapshot instead of opening a fresh connection for each block.
    pub(crate) fn quiescence_assessment_with_connection(
        conn: &Connection,
        run_id: &str,
    ) -> Result<AgentOrgQuiescenceAssessment, String> {
        load_and_assess(conn, run_id)
    }
}
