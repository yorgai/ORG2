use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

use serde::Serialize;
use tauri::AppHandle;

use crate::coordination::agent_org_formal_triggers::list_missing_doorbells_with_connection;
use crate::coordination::agent_org_runs::COORDINATOR_MEMBER_ID;
use crate::tools::impls::orchestration::inbox_wake::AppHandleInboxWakeHook;
use crate::tools::impls::orchestration::org_send_message::InboxWakeHook;

use super::{WATCHDOG_INTERVAL_SECS, WATCHDOG_MAX_RECEIPTS, WATCHDOG_TEAM_BUDGET};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoorbellRepairReport {
    pub scanned_assignments: usize,
    pub repaired_assignments: usize,
    pub scanned_receipts: usize,
    pub affected_runs: usize,
    pub repaired_receipts: usize,
    pub receipt_ids: Vec<String>,
}

pub fn spawn(app_handle: AppHandle) {
    if !crate::coordination::agent_org_runs::agent_org_enabled() {
        return;
    }
    tauri::async_runtime::spawn(async move {
        let mut interval =
            tokio::time::interval(std::time::Duration::from_secs(WATCHDOG_INTERVAL_SECS));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            let handle = app_handle.clone();
            match tokio::task::spawn_blocking(move || repair_missing_doorbells(handle)).await {
                Ok(Ok(report)) if report.repaired_receipts > 0 => tracing::debug!(
                    requested_receipts = report.repaired_receipts,
                    affected_runs = report.affected_runs,
                    "[agent_org_watchdog] requested durable doorbell repair"
                ),
                Ok(Ok(_)) => {}
                Ok(Err(error)) => tracing::warn!(
                    error = %error,
                    "[agent_org_watchdog] missing-doorbell scan failed"
                ),
                Err(error) => tracing::warn!(
                    error = %error,
                    "[agent_org_watchdog] missing-doorbell task failed"
                ),
            }
        }
    });
}

pub fn repair_missing_doorbells(app_handle: AppHandle) -> Result<DoorbellRepairReport, String> {
    let wake_hook = AppHandleInboxWakeHook::new(app_handle);
    repair_missing_doorbells_with_hook(wake_hook.as_ref())
}

pub(super) fn repair_missing_doorbells_with_hook(
    wake_hook: &dyn InboxWakeHook,
) -> Result<DoorbellRepairReport, String> {
    let assignment_repairs =
        crate::coordination::agent_org_tasks::AgentOrgTaskStore::repair_lost_assignment_doorbells(
            WATCHDOG_MAX_RECEIPTS,
        )?;
    let receipts = {
        let conn = database::db::get_connection().map_err(|error| error.to_string())?;
        list_missing_doorbells_with_connection(&conn, WATCHDOG_MAX_RECEIPTS)?
    };
    if receipts.is_empty() && assignment_repairs.is_empty() {
        return Ok(DoorbellRepairReport::default());
    }

    let mut by_run = BTreeMap::<String, BTreeSet<String>>::new();
    let mut report = DoorbellRepairReport {
        scanned_assignments: assignment_repairs.len(),
        repaired_assignments: assignment_repairs.len(),
        scanned_receipts: receipts.len(),
        ..DoorbellRepairReport::default()
    };
    for repair in assignment_repairs {
        wake_hook.wake_member(&repair.owner_member_id, &repair.org_run_id);
        by_run
            .entry(repair.org_run_id)
            .or_default()
            .insert(repair.coordinator_receipt_id);
    }
    for receipt in receipts {
        by_run
            .entry(receipt.org_run_id)
            .or_default()
            .insert(receipt.receipt_id);
    }
    report.affected_runs = by_run.len();
    for (run_id, receipt_ids) in by_run {
        let deadline = Instant::now() + WATCHDOG_TEAM_BUDGET;
        let receipt_ids = receipt_ids
            .into_iter()
            .take_while(|_| Instant::now() < deadline)
            .collect::<Vec<_>>();
        if receipt_ids.is_empty() {
            continue;
        }
        wake_hook.wake_member_for_formal_receipts(COORDINATOR_MEMBER_ID, &run_id, &receipt_ids);
        report.repaired_receipts = report.repaired_receipts.saturating_add(receipt_ids.len());
        report.receipt_ids.extend(receipt_ids);
    }
    Ok(report)
}
