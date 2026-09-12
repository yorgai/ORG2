//! Fixed-budget repair for lost Agent Org formal-trigger doorbells.
//!
//! The watchdog never invents business facts, reminder messages, Task
//! mutations, finality, or retry episodes. It only finds an existing pending
//! `FormalTriggerReceipt` whose durable doorbell acknowledgement is missing,
//! requests one Coordinator wake per affected Team, then acknowledges that
//! same receipt. With no missing doorbell, a tick is an indexed read-only
//! no-op.

mod budget;
mod recover;
mod reservation;

#[cfg(test)]
mod tests;

pub use budget::clear_rewake_budget;
pub(crate) use budget::create_schema;
pub use budget::init_schema;
pub(crate) use budget::member_rewake_fingerprint;
pub(crate) use budget::task_failure_recovery_attempts_exhausted;
#[cfg(test)]
pub use budget::test_only_mark_failed_rewake_attempt;
pub(crate) use budget::{
    reserve_task_failure_recovery_with_connection, reserve_task_shutdown_release,
    task_failure_recovery_already_processed_with_connection, task_failure_recovery_fingerprint,
};
pub use recover::{repair_missing_doorbells, spawn, DoorbellRepairReport};
pub(crate) use reservation::{
    commit_member_rewake_reservation, refund_member_rewake_reservation,
    reserve_member_rewake_dispatch, MemberRewakeReservationOutcome,
};

use chrono::{DateTime, Duration as ChronoDuration, Utc};
use database::db::{get_connection, with_sessions_writer};
use rusqlite::{params, Connection, OptionalExtension};

use crate::coordination::agent_inbox::AgentInboxStore;
use crate::core::session::SessionStatus;

const WATCHDOG_INTERVAL_SECS: u64 = 60;
const WATCHDOG_MAX_RECEIPTS: usize = 100;
const WATCHDOG_TEAM_BUDGET: std::time::Duration = std::time::Duration::from_millis(250);

// These budgets belong to direct event owners (member wake, task failure,
// shutdown release), not to the periodic watchdog. They share the watchdog's
// durable recovery-attempt store, but the watchdog does not consume them.
const RECOVERY_DELAYS_SECS: [i64; 3] = [60, 5 * 60, 15 * 60];
const MEMBER_REWAKE: &str = "member_rewake";
