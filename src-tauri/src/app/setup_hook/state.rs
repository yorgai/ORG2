//! Setup stages 3 and 5: the managed application state Tauri hands back to
//! commands (event store, PTY, LSP, browser, unified agent
//! state, MCP, agent definitions, settings, power) plus the app-level hooks
//! and listeners installed alongside it.

// Carried over verbatim from the crate root, where this attribute already sat
// on this exact `use` item. It moved with the import to the module that needs
// both traits; on desktop targets `cfg_attr(mobile, …)` expands to nothing.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
use tauri::{Listener, Manager};

use crate::agent_sessions;
use crate::app::bootstrap::dev_startup_debug_enabled;

pub(crate) fn init_core_state(app: &tauri::App) {
    // Initialize Rust EventStore state
    app.manage(agent_sessions::event_pipeline::commands::EventStoreState::new());
    tracing::info!("[EventStore] Rust event store initialized");

    let persistence_recovery = match crate::app::startup_recovery::run_persistence_startup_recovery(
    ) {
        Ok(report) => {
            tracing::info!(
                ordinary_turn_intents_reconciled = report.ordinary_turn_intents_reconciled,
                agent_org_turn_intents_reconciled = report.agent_org_turn_intents_reconciled,
                terminal_sessions_reconciled = report.terminal_sessions_reconciled,
                sessions_abandoned = report.sessions_abandoned,
                inspected_agent_org_runs = report.agent_org.inspected_runs,
                recovered_agent_org_tasks = report.agent_org.recovered_task_count(),
                recovery_failures = report.agent_org.failures.len(),
                "[StartupRecovery] persistence reconciliation completed"
            );
            Some(report)
        }
        Err(error) => {
            tracing::warn!(error = %error, "[StartupRecovery] persistence reconciliation failed");
            None
        }
    };

    // Initialize PTY state for terminal sessions
    let pty_state = ::terminal::pty_commands::pty::PtyState::new();
    let pty_sessions_arc = pty_state.sessions_arc();
    app.manage(pty_state);
    tracing::info!("[PTY] Terminal PTY state initialized");

    // Initialize LSP Manager
    let lsp_manager = std::sync::Arc::new(tokio::sync::Mutex::new(lsp::LspManager::new()));
    app.manage(lsp_manager);
    tracing::info!("[LSP] LSP manager initialized");

    let agent_browser_config = match settings::file_io::read_settings() {
        Ok(settings_value) => shared_state::AgentBrowserConfig::from_settings(&settings_value),
        Err(err) => {
            tracing::warn!(
                "[Browser] Failed to read Agent Browser settings; using defaults: {}",
                err
            );
            shared_state::AgentBrowserConfig::default()
        }
    };

    // Initialize independent browser and screenshot state (to avoid circular dependencies)
    let agent_browser = std::sync::Arc::new(tokio::sync::Mutex::new(
        shared_state::AgentBrowserController::with_config(agent_browser_config),
    ));
    let screenshot_store = std::sync::Arc::new(shared_state::ScreenshotStore::new());

    // Manage browser and screenshot state independently for dependency injection
    app.manage(agent_browser.clone());
    app.manage(screenshot_store.clone());
    tracing::info!("[Browser] Agent browser controller and screenshot store initialized");

    // Initialize Unified Agent State (replaces separate OS/SDE states)
    let mut unified_state = agent_core::state::AgentAppState::with_browser(
        agent_browser.clone(),
        screenshot_store.clone(),
    );
    unified_state.set_pty_sessions(pty_sessions_arc.clone());
    unified_state.set_app_handle(app.handle().clone());

    // Plan-approval lifecycle: process-wide AppHandle for terminal
    // transcript events pushed outside a live session manager, then
    // a one-shot GC pass that archives orphaned pending-plan rows
    // (missing plan file / deleted session), a repair scan that
    // restores half-committed create_plan submissions, then a scan
    // that finalizes historically stranded awaiting_user create_plan
    // events (pre-backend-finalize archives whose FE patch never
    // landed).
    agent_core::interaction::plan_approval::install_app_handle(app.handle().clone());
    tauri::async_runtime::spawn(async {
        agent_core::interaction::plan_approval::gc_orphaned_pending_plans().await;
        agent_core::interaction::plan_approval::repair_orphaned_create_plan_submissions().await;
        tokio::task::spawn_blocking(
            crate::agent_sessions::event_pipeline::agent_core_bridge::repair_stranded_plan_events,
        );
    });

    // Install the production `MemberShutdownHook` for the
    // inbox-drain side effect that fires when the coordinator
    // accepts a member's `ShutdownResponse{accepted=true}`.
    // The hook resolves `(member_agent_id, run_id) →
    // session_id` via the org store and dispatches an
    // `AgentState::cancel_session`.
    agent_core::core::session::turn::inbox_drain::install_member_shutdown_hook(
        agent_core::tools::impls::orchestration::member_shutdown::AppHandleMemberShutdownHook::new(
            app.handle().clone(),
        ),
    );
    tracing::info!("[InboxDrain] Member shutdown hook installed");

    // Install the production `MemberIdleHook` so every worker
    // turn-end (success or cancel) posts a `MemberIdle`
    // envelope into the coordinator's inbox and wakes the
    // coordinator to keep draining open org work.
    agent_core::core::session::turn::member_idle::install_member_idle_hook(
        agent_core::tools::impls::orchestration::member_idle::InboxStoreMemberIdleHook::new(
            agent_core::tools::impls::orchestration::inbox_wake::AppHandleInboxWakeHook::new(
                app.handle().clone(),
            ),
        ),
    );
    tracing::info!("[MemberIdle] Member idle hook installed");

    // Plan artifacts have their own one-shot startup owner. Keep this repair
    // independent from the bounded Working-only watchdog so a global
    // filesystem scan cannot consume its Team scan budget or run repeatedly.
    tauri::async_runtime::spawn(async move {
        match tokio::task::spawn_blocking(|| {
            agent_core::coordination::agent_org_plan_approvals::AgentOrgPlanRevisionStore::repair_latest_plan_artifacts()
        })
        .await
        {
            Ok(Ok(report)) if report.repaired > 0 || report.failed > 0 => tracing::info!(
                inspected = report.inspected,
                repaired = report.repaired,
                failed = report.failed,
                "[AgentOrgPlanArtifacts] one-shot startup reconciliation finished"
            ),
            Ok(Ok(_)) => {}
            Ok(Err(error)) => tracing::warn!(
                error = %error,
                "[AgentOrgPlanArtifacts] startup reconciliation failed"
            ),
            Err(error) => tracing::warn!(
                error = %error,
                "[AgentOrgPlanArtifacts] startup worker failed"
            ),
        }
    });

    // Install the production `JobCompletionWakeHook` so a background
    // job — subagent worker or backgrounded shell — that finishes
    // while its owning session is idle resumes that session's turn
    // loop (which then consumes the result via the Background Jobs
    // reminder). Without this, an idle owner never learns the job
    // completed. Mirrors Claude Code's task-notification →
    // idle-queue-processor wake.
    agent_core::tools::impls::orchestration::job_wake::install_job_completion_wake_hook(
        agent_core::tools::impls::orchestration::job_wake::AppHandleJobCompletionWakeHook::new(
            app.handle().clone(),
        ),
    );
    tracing::info!("[JobWake] Job completion wake hook installed");

    let agent_org_startup_state = unified_state.clone();
    let agent_org_archive_reconcile_state = unified_state.clone();
    let agent_org_handoff_reconcile_state = unified_state.clone();
    let housekeeper_compaction_state = unified_state.clone();
    app.manage(unified_state);
    tracing::info!("[UnifiedAgent] Unified agent state initialized");

    if let Some(report) = persistence_recovery.as_ref() {
        crate::app::startup_recovery::dispatch_agent_org_recovery_receipts(
            app.handle().clone(),
            &report.agent_org,
        );
        tracing::info!(
            receipts = report
                .agent_org
                .recovered_tasks
                .iter()
                .filter(|recovery| recovery.receipt_id.is_some())
                .count(),
            "[AgentOrgStartup] exact recovery doorbells scheduled"
        );
    }

    if agent_core::coordination::agent_org_runs::agent_org_enabled() {
        agent_core::coordination::agent_org_watchdog::spawn(app.handle().clone());
        tracing::info!("[AgentOrgWatchdog] Agent Org watchdog started");
    } else {
        tracing::info!("[AgentOrgWatchdog] Agent Org is disabled");
    }

    agent_core::core::session::launch::spawn_agent_org_startup_recovery(agent_org_startup_state);
    tracing::info!("[AgentOrgStartup] one-shot lifecycle recovery scheduled");

    // One event-driven outbound relay supervisor. It sleeps while the
    // feature is disabled and reacts to settings-file changes without polling.
    crate::api::mobile_bridge::relay::start();

    if agent_core::coordination::agent_org_runs::agent_org_enabled() {
        agent_core::state::commands::session::org_tasks::reconcile_pending_archive_teardowns(
            agent_org_archive_reconcile_state,
        );
        agent_core::state::commands::session::org_tasks::reconcile_pending_task_handoff_resolutions(
            agent_org_handoff_reconcile_state,
        );
        tracing::info!(
            "[AgentOrgLifecycle] one-shot Archive and Task handoff reconciliation scheduled"
        );
    }

    agent_core::session::housekeeper_compaction::spawn(
        housekeeper_compaction_state,
    );
    tracing::info!(
        "[HousekeeperCompaction] opt-in MiniCPM context worker initialized"
    );
}

pub(crate) fn init_settings_and_stores(app: &tauri::App) {
    // Load skill env vars from ~/.orgii/skill-env.json into the process
    agent_core::skills::loader::load_and_apply_skill_env();

    // Initialize MCP state
    app.manage(agent_core::mcp::commands::McpState::new());
    tracing::info!("[MCP] MCP server manager initialized");

    // Agent Definitions and Orgs

    // Manage the process-wide store singletons. Library code that
    // has no AppHandle reaches the SAME instances via
    // `definitions_store()` / `orgs_store()` — one in-memory state,
    // no per-call disk re-reads.
    app.manage(agent_core::definitions::definitions_store());
    tracing::info!("[AgentDefinitions] Custom agent definitions loaded");

    // Every store mutation (RPC commands, skills_toggle, the
    // manage_agent_def LLM tool) flows through the store
    // chokepoints, which fire this hook — frontend atoms refresh
    // on the event instead of manual post-mutation polling.
    {
        let handle = app.handle().clone();
        agent_core::definitions::set_definitions_changed_hook(move |agent_id| {
            use tauri::Emitter;
            let _ = handle.emit(
                "orgii-agent-defs-changed",
                serde_json::json!({ "agentId": agent_id }),
            );
        });
    }

    app.manage(agent_core::definitions::orgs::orgs_store());
    tracing::info!("[AgentOrgs] Agent organizations loaded");

    // Initialize Settings state and file watcher
    let settings_state = settings::SettingsState::new();
    match settings::watcher::start_watching(app.handle().clone()) {
        Ok(handle) => match settings_state.watcher_handle.lock() {
            Ok(mut watcher_handle) => {
                *watcher_handle = Some(handle);
                tracing::info!("[Settings] File watcher started for ~/.orgii/settings.jsonc");
            }
            Err(err) => {
                tracing::error!(error = %err, "[Settings] Failed to lock watcher handle");
            }
        },
        Err(err) => {
            tracing::warn!(error = %err, "[Settings] Failed to start file watcher");
        }
    }
    app.manage(settings_state);

    // System power state — holds the platform sleep-inhibitor handle
    // while at least one agent session is actively working AND the
    // `general.preventSleepWhileRunning` setting is enabled.
    app.manage(system_services::power::PowerState::new());

    // Apply HTTP version preference from settings.jsonc so the
    // provider HTTP clients (created lazily per-session) honor it.
    if let Ok(settings) = settings::file_io::read_settings() {
        if let Some(val) = settings.get("network.httpVersion").and_then(|v| v.as_str()) {
            let pref = agent_core::utils::HttpVersionPref::from_setting(val);
            agent_core::utils::set_global_http_version_pref(pref);
            tracing::info!(
                http_version = val,
                "[Network] HTTP version preference applied"
            );
        }
    }

    if dev_startup_debug_enabled() {
        app.listen("orgii-startup-first-paint", |event| {
            println!(
                "[TauriStartup] frontend first paint ready {}",
                event.payload()
            );
            tracing::info!(
                payload = event.payload(),
                "[TauriStartup] frontend first paint ready"
            );
        });
    }
}
