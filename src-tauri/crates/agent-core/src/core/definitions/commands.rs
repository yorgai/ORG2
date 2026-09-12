//! Tauri commands for agent definition and org CRUD operations.

use super::builtin::get_builtin_agents;
use super::orgs::{AgentOrgsStore, OrgDefinition};
use super::patch::AgentDefinitionPatch;
use super::schema::AgentDefinition;
use super::store::AgentDefinitionsStore;
use crate::foundation::security::CommandRiskRules;
use crate::integrations::patch::IntegrationsConfigPatch;
use crate::integrations::IntegrationsConfig;
use crate::session::prompt::cache::PromptCacheInvalidationReason;
use crate::state::AgentAppState;

// ── Agent Definition commands ──

/// Returns all agent definitions: builtin agents first, then user-created
/// agents. The user-only sibling `agent_definitions_list` was retired —
/// every consumer wants the unified builtin + user list, and a
/// "user-only" view can always be derived by filtering on
/// `AgentDefinition::built_in`.
#[tauri::command]
pub async fn agent_definitions_list_all(
    state: tauri::State<'_, std::sync::Arc<AgentDefinitionsStore>>,
) -> Result<Vec<AgentDefinition>, String> {
    let mut all = get_builtin_agents();
    let user_agents = state
        .agents
        .lock()
        .map_err(|err| format!("Lock error: {}", err))?;
    all.extend(user_agents.clone());
    Ok(all)
}

#[tauri::command]
pub async fn agent_definitions_add(
    state: tauri::State<'_, std::sync::Arc<AgentDefinitionsStore>>,
    agent_json: String,
) -> Result<String, String> {
    let agent: AgentDefinition =
        serde_json::from_str(&agent_json).map_err(|err| format!("Invalid agent JSON: {}", err))?;
    state.insert(agent)
}

#[tauri::command]
pub async fn agent_definitions_remove(
    state: tauri::State<'_, std::sync::Arc<AgentDefinitionsStore>>,
    app_state: tauri::State<'_, AgentAppState>,
    agent_id: String,
) -> Result<bool, String> {
    let removed = state.remove(&agent_id)?;
    if removed {
        let cancelled_memory_jobs =
            crate::memory::background::cancel_memory_jobs_for_agent(&agent_id);
        if cancelled_memory_jobs > 0 {
            tracing::info!(
                agent_id = %agent_id,
                cancelled_memory_jobs,
                "[agent_def_remove] cancelled background memory jobs for removed agent"
            );
        }
        app_state
            .invalidate_prompt_caches_for_agent_definition(
                &agent_id,
                PromptCacheInvalidationReason::AgentDefinitionChanged,
            )
            .await;
    }
    Ok(removed)
}

// ── Org commands ──

#[tauri::command]
pub async fn agent_orgs_list(
    state: tauri::State<'_, std::sync::Arc<AgentOrgsStore>>,
) -> Result<Vec<OrgDefinition>, String> {
    state.list()
}

pub(in crate::core::definitions) struct TrustedAgentOrgSettingsActor {
    _private: (),
}

#[tauri::command]
pub async fn agent_orgs_save_trusted_settings(
    state: tauri::State<'_, std::sync::Arc<AgentOrgsStore>>,
    org_json: String,
) -> Result<OrgDefinition, String> {
    let org: OrgDefinition =
        serde_json::from_str(&org_json).map_err(|err| format!("Invalid org JSON: {}", err))?;
    let store = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        store.save_trusted_settings(org, TrustedAgentOrgSettingsActor { _private: () })
    })
    .await
    .map_err(|err| format!("Agent Org settings save task failed: {}", err))?
}

#[tauri::command]
pub async fn agent_orgs_remove(
    state: tauri::State<'_, std::sync::Arc<AgentOrgsStore>>,
    org_id: String,
) -> Result<bool, String> {
    let store = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || store.remove(&org_id))
        .await
        .map_err(|err| format!("Agent Org remove task failed: {}", err))?
}

/// One row in the Inbox flat chat list — a persisted agent-org run that
/// has anchored a coordinator session. The Inbox renders one row per
/// run (so a single org may have multiple chats), ordered by recent
/// activity. `org_id` is included so the frontend can join with its
/// already-loaded `agent_orgs_list` data to render the org name as a
/// sub-label without an extra round-trip per row.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InboxRunSummary {
    pub run_id: String,
    pub org_id: String,
    pub root_session_id: String,
    pub status: String,
    pub summary: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// List every persisted run that has anchored a coordinator session,
/// across all orgs, ordered by `updated_at DESC`. The Inbox renders
/// the result as its flat chat list (one row per run).
///
/// `limit` caps the response at 200 rows. The current Inbox UI does
/// not paginate — orgs that build up thousands of historical runs
/// should re-introduce pagination here before lifting the cap.
#[tauri::command]
pub async fn agent_org_run_list(limit: Option<usize>) -> Result<Vec<InboxRunSummary>, String> {
    use crate::core::coordination::agent_org_runs::AgentOrgRunStore;
    crate::core::coordination::agent_org_runs::require_agent_org_enabled()?;
    const MAX_LIMIT: usize = 200;
    let effective_limit = limit.map(|n| n.min(MAX_LIMIT)).unwrap_or(MAX_LIMIT);
    // This command backs a read-only Inbox list. Quiescence reconciliation is a
    // lifecycle/watchdog responsibility: doing it here used to turn one UI
    // refresh into as many as 200 global writer-lock + IMMEDIATE
    // transactions. Keep the read off the async command executor as well.
    let runs = tokio::task::spawn_blocking(move || AgentOrgRunStore::list_runs(effective_limit))
        .await
        .map_err(|err| format!("Agent Org run list worker failed: {err}"))??;
    Ok(runs
        .into_iter()
        .filter_map(|run| {
            let root_session_id = run.root_session_id?;
            Some(InboxRunSummary {
                run_id: run.id,
                org_id: run.org_id,
                root_session_id,
                status: run.status.as_str().to_string(),
                summary: run.summary,
                created_at: run.created_at,
                updated_at: run.updated_at,
            })
        })
        .collect())
}
// ─────────────────────────────────────────────────────────────────────────────
// RPC contract §13 — Typed patch RPCs.
//
// The four commands below replace the retired blob-based
// `agent_get_config` / `agent_update_config` surface. Frontend consumers
// that need to read / mutate a single agent's configuration call
// `agent_def_get` / `agent_def_update_patch`; consumers that need
// app-level integrations (channels, plugins, web search, exec)
// call `integrations_get` / `integrations_update_patch`.
//
// Patch semantics: every field on both patch types is `Option<T>`. `None`
// leaves the existing value unchanged; `Some(value)` replaces the whole
// sub-struct. See `core::definitions::patch` and
// `integrations::patch` for the full rationale.
// ─────────────────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn agent_def_get(
    state: tauri::State<'_, std::sync::Arc<AgentDefinitionsStore>>,
    agent_id: String,
) -> Result<AgentDefinition, String> {
    state
        .get(&agent_id)
        .ok_or_else(|| format!("agent '{}' not found", agent_id))
}

/// Backend-resolved per-tool availability state for the Settings tool
/// editor. One row per builtin tool; the frontend renders this verbatim
/// instead of re-implementing `CapabilitySet::satisfies` and the
/// excluded/user-allowed precedence in TypeScript (the two copies had
/// already diverged: Rust lets `user_allowed_tools` win over
/// `excluded_tools` when the capability allows, the TS copy made
/// excluded win unconditionally).
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentToolStateRow {
    pub name: String,
    /// Effective availability after the full resolve (capability +
    /// restrict_to + excluded/user_allowed) — what the session will see.
    pub enabled: bool,
    /// Tool is in the system-pinned allowlist (`system_restrict_to_tools`).
    pub system_pinned: bool,
    /// Tool is in the user's `user_allowed_tools` delta.
    pub user_allowed: bool,
    /// Tool is in the user's `excluded_tools` delta.
    pub user_excluded: bool,
    /// The agent's capability set does not satisfy the tool's
    /// `required_capability` — cannot be enabled regardless of deltas.
    pub capability_blocked: bool,
}

#[tauri::command]
pub async fn agent_def_tool_states(
    state: tauri::State<'_, std::sync::Arc<AgentDefinitionsStore>>,
    agent_id: String,
) -> Result<Vec<AgentToolStateRow>, String> {
    use crate::tools::builtin_tools::BUILTIN_TOOLS;

    let def = state
        .get(&agent_id)
        .ok_or_else(|| format!("agent '{}' not found", agent_id))?;
    let merged =
        super::resolver::resolve_definition(&def, Some(&state)).map_err(|err| err.to_string())?;
    let capabilities = merged.capabilities.clone().unwrap_or_default();
    let resolved =
        super::resolved::ResolvedToolSelection::from_schema(&merged.tools, &capabilities);
    let disabled = crate::tools::derive_disabled_tools(&resolved.restrict_to, &resolved.excluded);

    let system_set: std::collections::HashSet<&str> = merged
        .tools
        .system_restrict_to_tools
        .as_deref()
        .unwrap_or_default()
        .iter()
        .map(String::as_str)
        .collect();
    let user_allowed: std::collections::HashSet<&str> = merged
        .tools
        .user_allowed_tools
        .iter()
        .map(String::as_str)
        .collect();
    let user_excluded: std::collections::HashSet<&str> = merged
        .tools
        .excluded_tools
        .iter()
        .map(String::as_str)
        .collect();
    let capability_blocked: std::collections::HashSet<String> =
        crate::tools::defaults::default_excluded_tools_for_capabilities(&capabilities)
            .into_iter()
            .collect();

    Ok(BUILTIN_TOOLS
        .iter()
        .map(|entry| AgentToolStateRow {
            name: entry.name.to_string(),
            enabled: !disabled.contains(entry.name),
            system_pinned: system_set.contains(entry.name),
            user_allowed: user_allowed.contains(entry.name),
            user_excluded: user_excluded.contains(entry.name),
            capability_blocked: capability_blocked.contains(entry.name),
        })
        .collect())
}

#[tauri::command]
pub async fn agent_command_risk_rules_default() -> CommandRiskRules {
    CommandRiskRules::default()
}

#[tauri::command]
pub async fn agent_def_update_patch(
    state: tauri::State<'_, std::sync::Arc<AgentDefinitionsStore>>,
    app_state: tauri::State<'_, AgentAppState>,
    agent_id: String,
    patch: AgentDefinitionPatch,
) -> Result<AgentDefinition, String> {
    let updated = if super::builtin::is_builtin_agent(&agent_id) {
        let gated = patch.gate_for_builtin();
        state.update_with_overlay(&agent_id, |def| gated.apply(def))
    } else {
        state.update(&agent_id, |def| patch.apply(def))
    }?;
    let cancelled_memory_jobs =
        crate::memory::background::cancel_disabled_memory_jobs_for_agent(&agent_id);
    if cancelled_memory_jobs > 0 {
        tracing::info!(
            agent_id = %agent_id,
            cancelled_memory_jobs,
            "[agent_def_update] cancelled background memory jobs disabled by hot policy update"
        );
    }
    app_state
        .invalidate_prompt_caches_for_agent_definition(
            &agent_id,
            PromptCacheInvalidationReason::AgentDefinitionChanged,
        )
        .await;
    Ok(updated)
}

/// Drop any user overlay on a `builtin:*` agent definition, reverting it
/// to the compiled-in default. Returns the resulting definition (the
/// freshly-seeded compiled-in builtin). No-op when no overlay exists;
/// returns an error for non-builtin ids.
#[tauri::command]
pub async fn agent_def_reset_builtin(
    state: tauri::State<'_, std::sync::Arc<AgentDefinitionsStore>>,
    app_state: tauri::State<'_, AgentAppState>,
    agent_id: String,
) -> Result<AgentDefinition, String> {
    state.reset_builtin(&agent_id)?;
    let cancelled_memory_jobs =
        crate::memory::background::cancel_disabled_memory_jobs_for_agent(&agent_id);
    if cancelled_memory_jobs > 0 {
        tracing::info!(
            agent_id = %agent_id,
            cancelled_memory_jobs,
            "[agent_def_reset] cancelled background memory jobs disabled by reset policy"
        );
    }
    let definition = state
        .get(&agent_id)
        .ok_or_else(|| format!("builtin '{}' missing after reset", agent_id))?;
    app_state
        .invalidate_prompt_caches_for_agent_definition(
            &agent_id,
            PromptCacheInvalidationReason::AgentDefinitionChanged,
        )
        .await;
    Ok(definition)
}

#[tauri::command]
pub async fn integrations_get(
    state: tauri::State<'_, AgentAppState>,
) -> Result<IntegrationsConfig, String> {
    Ok(state.integrations.snapshot())
}

#[tauri::command]
pub async fn integrations_update_patch(
    state: tauri::State<'_, AgentAppState>,
    patch: IntegrationsConfigPatch,
) -> Result<IntegrationsConfig, String> {
    state
        .integrations
        .update(|cfg| -> Result<(), std::convert::Infallible> {
            patch.apply(cfg);
            Ok(())
        })
        .map_err(|err| format!("{}", err))
}
