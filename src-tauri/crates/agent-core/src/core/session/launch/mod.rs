//! Canonical Rust-agent launch service.
//!
//! This module is the single runtime entry point for creating a Rust-native
//! agent session and optionally starting its first turn. Tauri commands,
//! WorkItem orchestration, Routine fires, and debug probes should adapt their
//! wire/domain DTOs into `AgentRunLaunchRequest` instead of duplicating session
//! creation or first-turn startup logic.

mod launch_helpers;
mod launch_org;
#[cfg(test)]
mod launch_tests;
mod launch_workspace;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

use tauri::Manager;

use crate::coordination::agent_org_runs::{
    AgentOrgRunEntryMode, AgentOrgRunStore, CreateAgentOrgInitialInput,
    CreateAgentOrgMaterializationIntent, CreateStartingAgentOrgRunParams, COORDINATOR_MEMBER_ID,
};
use crate::definitions::orgs::{AgentOrgsStore, OrgMemberLaunchOverride};
use crate::init::launch_spec::AgentLaunchSpec;
use crate::session::persistence;
use crate::session::IdeContext;
use crate::state::AgentAppState;
use project_management::projects::types as project_types;

use launch_helpers::{
    apply_member_launch_overrides_to_snapshot, derive_name, handle_background_launch_failure,
    provenance_fields, provenance_lock_reason, validate_launch_agent_definitions,
};
use launch_org::{
    cleanup_session_after_org_run_create_failure, materialize_org_member_sessions,
    send_initial_turn,
};
use launch_workspace::{
    acquire_work_item_execution_lock, prepare_rust_agent_workspace_for_launch,
    release_work_item_execution_lock_if_present,
};

pub(crate) const MAX_AUTO_NAME_LEN: usize = 80;

const AGENT_ORG_INITIAL_INPUT_PAYLOAD_VERSION: u8 = 1;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentOrgInitialInputPayload {
    version: u8,
    images: Option<Vec<String>>,
    ide_context: Option<IdeContext>,
    sub_agent_ids: Vec<String>,
}

fn decode_agent_org_initial_input_payload(
    input: &crate::coordination::agent_org_runs::AgentOrgInitialInput,
) -> Result<AgentOrgInitialInputPayload, String> {
    let payload: AgentOrgInitialInputPayload =
        serde_json::from_str(&input.payload_json).map_err(|error| {
            format!(
                "invalid Starting initial input payload for {}: {error}",
                input.org_run_id
            )
        })?;
    if payload.version != AGENT_ORG_INITIAL_INPUT_PAYLOAD_VERSION {
        return Err(format!(
            "unsupported Starting initial input payload version {} for {}",
            payload.version, input.org_run_id
        ));
    }
    Ok(payload)
}

#[derive(Debug, Clone)]
pub(crate) struct AgentRunLaunchRequest {
    /// Stable WorkItemRun id used for deterministic Session and turn ids.
    /// `None` preserves the ordinary user-launch behavior.
    pub durable_run_id: Option<String>,
    pub content: String,
    pub target: AgentRunTarget,
    pub resources: LaunchResourceSelection,
    pub workspace: WorkspaceLaunchTarget,
    pub org_context: LaunchOrgContext,
    pub provenance: LaunchProvenance,
    pub mode: Option<String>,
    /// Product mode (`orgtrack/v1` §5.2). Launch-from-work/routine
    /// resolves to `project` server-side regardless of this value.
    pub product_mode: Option<String>,
    pub name: Option<String>,
    pub images: Option<Vec<String>>,
    pub ide_context: Option<IdeContext>,
    pub parent_session_id: Option<String>,
    pub sub_agent_ids: Vec<String>,
}

#[derive(Debug, Clone)]
pub(crate) enum AgentRunTarget {
    AgentDefinition {
        agent_definition_id: Option<String>,
    },
    AgentOrg {
        agent_org_id: String,
        agent_definition_id: Option<String>,
        member_overrides: HashMap<String, OrgMemberLaunchOverride>,
        apply_member_overrides_for_future: bool,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct LaunchOrgContext {
    pub org_id: String,
    pub project_id: Option<String>,
    pub project_name: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct LaunchResourceSelection {
    pub key_source: Option<String>,
    pub account_id: Option<String>,
    pub model: Option<String>,
    pub native_harness_type: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) enum WorkspaceLaunchTarget {
    LocalWorkspace {
        workspace_path: String,
        additional_directories: Vec<String>,
    },
    Worktree {
        workspace_path: String,
        worktree_path: Option<String>,
        branch: Option<String>,
        create_isolated: bool,
        additional_directories: Vec<String>,
    },
}

#[derive(Debug, Clone)]
pub(crate) enum LaunchProvenance {
    UserSession,
    WorkItem {
        /// Project scope for the Work Item. `None` is a first-class
        /// standalone Work Item, not a reason to discard the linkage.
        project_slug: Option<String>,
        work_item_id: String,
        agent_role: Option<String>,
        lock_reason: project_types::WorkItemExecutionLockReason,
    },
    RoutineFire {
        routine_fire_id: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LaunchReturnStatus {
    Idle,
    FirstTurnStarted,
}

impl LaunchReturnStatus {
    pub(crate) fn session_status(self) -> crate::session::SessionStatus {
        match self {
            Self::Idle => crate::session::SessionStatus::Idle,
            Self::FirstTurnStarted => crate::session::SessionStatus::Running,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct AgentRunLaunchResult {
    pub session_id: String,
    pub status: LaunchReturnStatus,
    pub created_at: String,
    pub workspace_path: Option<String>,
    pub worktree_path: Option<String>,
    pub worktree_branch: Option<String>,
    pub base_ref: Option<String>,
    pub agent_org_id: Option<String>,
    pub agent_org_run_id: Option<String>,
    pub org_id: String,
    pub project_id: Option<String>,
    pub project_name: Option<String>,
    pub project_slug: Option<String>,
    pub work_item_id: Option<String>,
    pub agent_role: Option<String>,
    pub product_mode: Option<String>,
}

/// Write the fail-closed session-context marker (design M6) into the
/// session workspace: `org2-pm` locks its identity to this file when
/// present, so an agent inside the workspace can never act as a human
/// or as another session.
pub fn write_agent_session_marker(
    workspace_path: &str,
    session_id: &str,
    agent_definition_id: Option<&str>,
    product_mode: Option<&str>,
    project_slug: Option<&str>,
    org_id: Option<&str>,
) {
    if workspace_path.trim().is_empty() {
        return;
    }
    let agent = agent_definition_id
        .unwrap_or("os")
        .trim_start_matches("builtin:");
    // Historical rows may store NULL for ordinary Build, but the marker is a
    // fail-closed authority boundary and must always carry an explicit mode.
    // Otherwise `org2-pm --mode project` could elevate a Build session.
    let product_mode = product_mode.unwrap_or("build");
    let capabilities: Vec<&str> = if product_mode == "project" {
        vec![
            "work.read",
            "work.mutate",
            "routine.invoke",
            "project.mutate",
        ]
    } else {
        vec!["work.read"]
    };
    let marker = serde_json::json!({
        "apiVersion": "orgtrack/v1",
        "sessionRef": format!("org2:{session_id}"),
        "actor": format!("agent:{agent}"),
        "productMode": product_mode,
        "scope": project_slug,
        "org": project_management::projects::io::resolve_local_org_scope(org_id),
        "capabilities": capabilities,
        "issuedAt": chrono::Utc::now().to_rfc3339(),
    });
    let dir = std::path::Path::new(workspace_path).join(".orgii");
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let path = dir.join("agent_session_context.json");
    if let Ok(raw) = serde_json::to_string_pretty(&marker) {
        let _ = std::fs::write(path, raw);
    }
}

async fn generate_title_before_first_turn(
    state: &AgentAppState,
    session_id: &str,
    workspace_path: std::path::PathBuf,
    account_id: Option<String>,
    model: Option<String>,
    native_harness_type: Option<core_types::providers::NativeHarnessType>,
    content: &str,
) {
    if content.trim().is_empty() {
        return;
    }

    let launch_spec = match AgentLaunchSpec::from_session_sources(
        state,
        session_id,
        workspace_path,
        account_id,
        model,
        native_harness_type,
    )
    .await
    {
        Ok(spec) => spec,
        Err(err) => {
            tracing::warn!(
                session_id = %session_id,
                error = %err,
                "[session_title] failed to resolve launch spec before first turn"
            );
            return;
        }
    };

    let runtime = match crate::init::init_session(state, launch_spec).await {
        Ok(runtime) => runtime,
        Err(err) => {
            tracing::warn!(
                session_id = %session_id,
                error = %err,
                "[session_title] failed to initialize runtime before first turn"
            );
            return;
        }
    };

    let title = crate::session::title::generate_and_persist_session_title(
        session_id,
        runtime.provider.as_ref(),
        &runtime.model,
        runtime.account_id.as_deref(),
        content,
    )
    .await;

    crate::lifecycle::emit_session_renamed(state.app_handle.as_ref(), session_id, &title);
}

/// Spawn session-title generation as an independent fire-and-forget task.
///
/// Title generation is a `side_query` that runs a full LLM round-trip. It
/// MUST NOT block `send_initial_turn`: doing so makes any slowness/failure in
/// the auxiliary call (provider 400s, retry backoff, network) directly delay
/// the user's first turn — the very "a side query stalls the main turn"
/// coupling this decoupling removes. The title task persists the name and
/// emits `session:renamed` when it finishes; the first turn proceeds
/// concurrently regardless of when (or whether) the title resolves.
fn spawn_session_title_generation(
    state: AgentAppState,
    session_id: String,
    workspace_path: std::path::PathBuf,
    account_id: Option<String>,
    model: Option<String>,
    native_harness_type: Option<core_types::providers::NativeHarnessType>,
    content: String,
) {
    tokio::spawn(async move {
        generate_title_before_first_turn(
            &state,
            &session_id,
            workspace_path,
            account_id,
            model,
            native_harness_type,
            &content,
        )
        .await;
    });
}

/// Create and start an agent session linked to a work item.
///
/// This remains as the public WorkItem-facing adapter for existing callers;
/// all actual launch behavior is delegated to `launch_rust_agent_run`.
pub struct WorkItemLaunchRequest<'a> {
    pub durable_run_id: Option<&'a str>,
    pub workspace_path: &'a str,
    pub prompt: &'a str,
    pub model: &'a str,
    pub account_id: &'a str,
    pub work_item_id: &'a str,
    pub project_slug: &'a str,
    pub worktree_path: Option<&'a str>,
    pub agent_definition_id: Option<&'a str>,
    pub agent_role: &'a str,
    pub sub_agent_ids: &'a [String],
    pub lock_reason: project_types::WorkItemExecutionLockReason,
}

pub async fn launch_agent_session(
    app: &tauri::AppHandle,
    request: WorkItemLaunchRequest<'_>,
) -> Result<String, String> {
    let state: tauri::State<'_, AgentAppState> = app.state();
    let WorkItemLaunchRequest {
        durable_run_id,
        workspace_path,
        prompt,
        model,
        account_id,
        work_item_id,
        project_slug,
        worktree_path,
        agent_definition_id,
        agent_role,
        sub_agent_ids,
        lock_reason,
    } = request;

    let workspace = match worktree_path.filter(|path| !path.is_empty()) {
        Some(path) => WorkspaceLaunchTarget::Worktree {
            workspace_path: workspace_path.to_string(),
            worktree_path: Some(path.to_string()),
            branch: None,
            create_isolated: false,
            additional_directories: Vec::new(),
        },
        None => WorkspaceLaunchTarget::LocalWorkspace {
            workspace_path: workspace_path.to_string(),
            additional_directories: Vec::new(),
        },
    };

    let result = launch_rust_agent_run(
        &state,
        None,
        AgentRunLaunchRequest {
            durable_run_id: durable_run_id.map(str::to_string),
            content: prompt.to_string(),
            target: AgentRunTarget::AgentDefinition {
                agent_definition_id: agent_definition_id.map(str::to_string),
            },
            resources: LaunchResourceSelection {
                key_source: Some(
                    core_types::key_source::KeySource::OwnKey
                        .as_ref()
                        .to_string(),
                ),
                account_id: Some(account_id.to_string()),
                model: Some(model.to_string()),
                native_harness_type: None,
            },
            workspace,
            org_context: LaunchOrgContext {
                org_id: project_management::projects::types::PERSONAL_ORG_ID.to_string(),
                project_id: None,
                project_name: None,
            },
            provenance: LaunchProvenance::WorkItem {
                project_slug: Some(project_slug.to_string()),
                work_item_id: work_item_id.to_string(),
                agent_role: Some(agent_role.to_string()),
                lock_reason,
            },
            mode: Some(crate::session::AgentExecMode::Build.as_str().to_string()),
            // Launch-from-WorkItem is Project mode by the frozen resolver
            // (orgtrack/v1 decisions §1, precedence rule 1).
            product_mode: Some("project".to_string()),
            name: Some(format!("{}: {}", agent_role, work_item_id)),
            images: None,
            ide_context: None,
            parent_session_id: None,
            sub_agent_ids: sub_agent_ids.to_vec(),
        },
    )
    .await?;

    Ok(result.session_id)
}

pub(crate) async fn launch_rust_agent_run(
    state: &AgentAppState,
    org_store: Option<&AgentOrgsStore>,
    request: AgentRunLaunchRequest,
) -> Result<AgentRunLaunchResult, String> {
    if matches!(&request.target, AgentRunTarget::AgentOrg { .. }) {
        crate::coordination::agent_org_runs::require_agent_org_enabled()?;
    }
    let (workspace_path, branch, isolate, existing_worktree_path, additional_directories) =
        match &request.workspace {
            WorkspaceLaunchTarget::LocalWorkspace {
                workspace_path,
                additional_directories,
            } => (
                workspace_path.clone(),
                None,
                false,
                None,
                additional_directories.clone(),
            ),
            WorkspaceLaunchTarget::Worktree {
                workspace_path,
                worktree_path,
                branch,
                create_isolated,
                additional_directories,
            } => (
                workspace_path.clone(),
                branch.clone(),
                *create_isolated,
                worktree_path.clone(),
                additional_directories.clone(),
            ),
        };
    let (
        agent_org_id,
        coordinator_agent_id,
        agent_definition_id,
        org_definition,
        member_overrides,
        apply_member_overrides_for_future,
    ) = match &request.target {
        AgentRunTarget::AgentOrg {
            agent_org_id,
            agent_definition_id,
            member_overrides,
            apply_member_overrides_for_future,
        } => {
            let store = org_store.ok_or_else(|| {
                "Agent Org launch requires AgentOrgsStore, but none was provided".to_string()
            })?;
            let org = store.get(agent_org_id)?;
            let resolved = org.agent_id.trim();
            if resolved.is_empty() {
                return Err(format!(
                    "Agent Org '{}' has no coordinator agent configured",
                    org.name
                ));
            }
            if let Some(requested_agent_id) = agent_definition_id.as_deref() {
                if requested_agent_id != resolved {
                    return Err(format!(
                        "Agent Org '{}' coordinator '{}' conflicts with requested agent definition '{}'",
                        agent_org_id, resolved, requested_agent_id
                    ));
                }
            }
            (
                Some(agent_org_id.clone()),
                Some(resolved.to_string()),
                Some(resolved.to_string()),
                Some(org),
                member_overrides.clone(),
                *apply_member_overrides_for_future,
            )
        }
        AgentRunTarget::AgentDefinition {
            agent_definition_id,
        } => (
            None,
            None,
            agent_definition_id.clone(),
            None,
            HashMap::new(),
            false,
        ),
    };
    let effective_org_definition = org_definition
        .as_ref()
        .map(|org| {
            let mut effective_org = org.clone();
            apply_member_launch_overrides_to_snapshot(&mut effective_org.members, &member_overrides)
                .map(|()| effective_org)
        })
        .transpose()?;
    validate_launch_agent_definitions(
        agent_definition_id.as_deref(),
        effective_org_definition.as_ref(),
    )?;
    // A requested template mutation is durable before the immutable launch
    // snapshot or any Team lifecycle row is created. The effective snapshot
    // above contains the same overrides, while a disk failure leaves no run
    // that could appear to have accepted an unpersisted future policy.
    if apply_member_overrides_for_future {
        if let (Some(store), Some(org_id)) = (org_store, agent_org_id.as_ref()) {
            store.apply_member_launch_overrides(org_id, &member_overrides)?;
        }
    }

    let (project_slug, work_item_id, agent_role, routine_fire_id) =
        provenance_fields(&request.provenance);
    let name = request
        .name
        .clone()
        .unwrap_or_else(|| derive_name(None, &request.content));

    // Existing worktrees can be validated before a session id exists. Do it
    // before creating any DB/org records so an invalid or cross-repo path can
    // never surface as a briefly successful background launch.
    let registered_existing_worktree = if let Some(path) = existing_worktree_path.as_deref() {
        let repo = std::path::PathBuf::from(&workspace_path);
        let path = std::path::PathBuf::from(path);
        Some(
            tokio::task::spawn_blocking(move || {
                git::worktree::validate_existing_worktree(&repo, &path)
            })
            .await
            .map_err(|err| format!("worktree validation task failed: {err}"))??,
        )
    } else {
        None
    };
    let existing_worktree_path = registered_existing_worktree
        .as_ref()
        .map(|entry| entry.path.clone());
    let existing_worktree_branch = registered_existing_worktree
        .as_ref()
        .map(|entry| entry.branch.clone());

    let has_initial_content = !request.content.trim().is_empty();
    let org_session_key = agent_org_id.as_ref().map(|_| {
        request
            .durable_run_id
            .clone()
            .unwrap_or_else(|| format!("agent-org-{}", uuid::Uuid::new_v4()))
    });
    let expected_root_session_id = org_session_key.as_ref().map(|key| {
        let prefix = crate::definitions::prefix_lookup::session_prefix_for_launch(
            agent_definition_id.as_deref(),
            !workspace_path.is_empty(),
        );
        format!("{prefix}{key}")
    });
    let initial_turn_intent_id =
        has_initial_content.then(|| format!("agent-org-initial-turn-{}", uuid::Uuid::new_v4()));
    let initial_message_id =
        has_initial_content.then(|| format!("agent-org-initial-message-{}", uuid::Uuid::new_v4()));
    let initial_input_payload_json = has_initial_content
        .then(|| {
            serde_json::to_string(&AgentOrgInitialInputPayload {
                version: AGENT_ORG_INITIAL_INPUT_PAYLOAD_VERSION,
                images: request.images.clone(),
                ide_context: request.ide_context.clone(),
                sub_agent_ids: request.sub_agent_ids.clone(),
            })
            .map_err(|error| format!("serialize Agent Org initial input: {error}"))
        })
        .transpose()?;

    // Build the construction envelope before any Team lifecycle row exists.
    // The already-persisted coordinator is certified in the same transaction
    // as Starting and the remaining stable member identities.
    let starting_params = match (
        agent_org_id.as_ref(),
        coordinator_agent_id.as_ref(),
        effective_org_definition.as_ref(),
        expected_root_session_id.as_ref(),
    ) {
        (Some(org_id), Some(coordinator_id), Some(org_snapshot), Some(root_session_id)) => {
            let mut materialization_intents = vec![CreateAgentOrgMaterializationIntent {
                member_id: COORDINATOR_MEMBER_ID.to_string(),
                agent_id: coordinator_id.clone(),
                session_id: root_session_id.clone(),
                succeeded: true,
            }];
            for member in &org_snapshot.members {
                let prefix = crate::definitions::prefix_lookup::session_prefix_for_launch(
                    Some(&member.agent_id),
                    !workspace_path.is_empty(),
                );
                materialization_intents.push(CreateAgentOrgMaterializationIntent {
                    member_id: member.member_id.clone(),
                    agent_id: member.agent_id.clone(),
                    session_id: format!("{prefix}{}", uuid::Uuid::new_v4()),
                    succeeded: false,
                });
            }
            Some(CreateStartingAgentOrgRunParams {
                org_id: org_id.clone(),
                coordinator_agent_id: coordinator_id.clone(),
                root_session_id: root_session_id.clone(),
                org_snapshot: crate::definitions::orgs::AgentOrgLaunchSnapshot::from(org_snapshot),
                entry_mode: AgentOrgRunEntryMode::StandaloneSession,
                work_item_id: work_item_id.clone(),
                project_slug: project_slug.clone(),
                routine_fire_id: routine_fire_id.clone(),
                materialization_intents,
                initial_input: initial_turn_intent_id.as_ref().map(|turn_intent_id| {
                    CreateAgentOrgInitialInput {
                        turn_intent_id: turn_intent_id.clone(),
                        message_id: initial_message_id
                            .clone()
                            .expect("initial message id accompanies initial turn"),
                        content: request.content.clone(),
                        payload_json: initial_input_payload_json
                            .clone()
                            .expect("initial payload accompanies initial turn"),
                    }
                }),
            })
        }
        _ => None,
    };

    let create_result = crate::state::commands::session::create::create_session_impl(
        None,
        workspace_path.clone(),
        request.resources.model.clone(),
        request.resources.account_id.clone(),
        Some(name.clone()),
        Some(request.org_context.org_id.clone()),
        request.org_context.project_id.clone(),
        request.org_context.project_name.clone(),
        work_item_id.clone(),
        agent_role.clone(),
        existing_worktree_path.clone(),
        project_slug.clone(),
        agent_definition_id.clone(),
        request.resources.key_source.clone(),
        request.mode.clone(),
        request.product_mode.clone(),
        request.resources.native_harness_type.clone(),
        request.parent_session_id.clone(),
        org_session_key
            .clone()
            .or_else(|| request.durable_run_id.clone()),
    )
    .await?;

    let session_id = create_result
        .get("sessionId")
        .and_then(|value| value.as_str())
        .ok_or("create_session_impl did not return sessionId")?
        .to_string();
    if let Some(expected_session_id) = expected_root_session_id.as_deref() {
        if session_id != expected_session_id {
            return Err(format!(
                "coordinator identity mismatch: expected {expected_session_id}, got {session_id}"
            ));
        }
    }
    let resolved_product_mode = create_result
        .get("productMode")
        .and_then(|value| value.as_str())
        .map(str::to_string);

    if starting_params.is_some() {
        persistence::update_org_member_id(&session_id, COORDINATOR_MEMBER_ID)
            .map_err(|err| format!("failed to persist coordinator member_id: {err}"))?;
    }

    let starting_run = match starting_params {
        Some(params) => match AgentOrgRunStore::create_starting(params) {
            Ok(run) => Some(run),
            Err(error) => {
                cleanup_session_after_org_run_create_failure(session_id.clone()).await;
                return Err(error);
            }
        },
        None => None,
    };

    if let (Some(project_slug_value), Some(work_item_id_value)) =
        (project_slug.as_deref(), work_item_id.as_deref())
    {
        if let Err(err) = acquire_work_item_execution_lock(
            project_slug_value,
            work_item_id_value,
            &session_id,
            agent_role.as_deref(),
            provenance_lock_reason(&request.provenance),
        )
        .await
        {
            cleanup_session_after_org_run_create_failure(session_id.clone()).await;
            return Err(err);
        }
    }

    let agent_org_run_id = starting_run.as_ref().map(|run| run.id.clone());
    let created_at = chrono::Utc::now().to_rfc3339();
    let native_harness_type_for_send = request
        .resources
        .native_harness_type
        .as_deref()
        .filter(|value| !value.is_empty())
        .map(|value| {
            core_types::providers::NativeHarnessType::parse(value)
                .ok_or_else(|| format!("Unknown native_harness_type: {value:?}"))
        })
        .transpose()?;

    if agent_org_run_id.is_some() {
        let state_for_background = state.clone();
        let session_id_for_background = session_id.clone();
        let workspace_path_for_background = workspace_path.clone();
        let branch_for_background = branch.clone();
        let existing_worktree_path_for_background = existing_worktree_path.clone();
        let additional_directories_for_background = additional_directories.clone();
        let content_for_send = request.content.clone();
        let model_for_send = request.resources.model.clone();
        let account_id_for_send = request.resources.account_id.clone();
        let mode_for_send = request.mode.clone();
        let images_for_send = request.images.clone();
        let ide_context_for_send = request.ide_context.clone();
        let sub_agent_ids_for_send = request.sub_agent_ids.clone();
        let agent_definition_id_for_send = agent_definition_id.clone();
        let agent_org_run_id_for_background = agent_org_run_id.clone();
        let project_slug_for_background = project_slug.clone();
        let work_item_id_for_background = work_item_id.clone();
        let app_handle_for_background = state.app_handle.clone();
        let request_key_source_for_background = request.resources.key_source.clone();
        let request_native_harness_for_background = request.resources.native_harness_type.clone();
        let org_for_background = crate::definitions::orgs::AgentOrgLaunchSnapshot::from(
            effective_org_definition
                .as_ref()
                .expect("Agent Org launch has a validated snapshot"),
        );
        let starting_generation = starting_run
            .as_ref()
            .map(|run| run.activation_generation)
            .expect("Agent Org launch has a Starting generation");
        let initial_turn_intent_id_for_background = initial_turn_intent_id.clone();
        let initial_message_id_for_background = initial_message_id.clone();

        tokio::spawn(async move {
            let prepared_workspace = match prepare_rust_agent_workspace_for_launch(
                &session_id_for_background,
                &workspace_path_for_background,
                branch_for_background.as_deref(),
                isolate,
                existing_worktree_path_for_background.as_deref(),
                &additional_directories_for_background,
            )
            .await
            {
                Ok(path) => path,
                Err(err) => {
                    let message = format!(
                        "[session_launch] workspace preparation failed for {}: {}",
                        session_id_for_background, err
                    );
                    handle_background_launch_failure(
                        &session_id_for_background,
                        agent_org_run_id_for_background.as_deref(),
                        project_slug_for_background.as_deref(),
                        work_item_id_for_background.as_deref(),
                        app_handle_for_background.as_ref(),
                        &message,
                        "[session_launch] failed to mark Agent Org run failed after workspace preparation error",
                        "[session_launch] failed to mark session failed after workspace preparation error",
                    )
                    .await;
                    return;
                }
            };

            let workspace_path_for_send = prepared_workspace
                .worktree_path
                .clone()
                .unwrap_or_else(|| workspace_path_for_background.clone());
            write_agent_session_marker(
                &workspace_path_for_send,
                &session_id_for_background,
                agent_definition_id_for_send.as_deref(),
                None,
                project_slug_for_background.as_deref(),
                Some(project_management::projects::types::PERSONAL_ORG_ID),
            );

            let run_id = agent_org_run_id_for_background
                .as_deref()
                .expect("Agent Org background launch has a run id");
            if let Err(err) = materialize_org_member_sessions(
                run_id,
                &org_for_background,
                &session_id_for_background,
                &name,
                &workspace_path_for_send,
                model_for_send.clone(),
                account_id_for_send.clone(),
                request_key_source_for_background.clone(),
                mode_for_send.clone(),
                request_native_harness_for_background.clone(),
                work_item_id_for_background.clone(),
                project_slug_for_background.clone(),
            )
            .await
            {
                if err.is_retryable() {
                    tracing::warn!(
                        run_id = %run_id,
                        error = %err,
                        "[session_launch] retryable Starting materialization deferred"
                    );
                    return;
                }
                let message = format!(
                    "[session_launch] member materialization failed for {}: {}",
                    session_id_for_background, err
                );
                handle_background_launch_failure(
                    &session_id_for_background,
                    Some(run_id),
                    project_slug_for_background.as_deref(),
                    work_item_id_for_background.as_deref(),
                    app_handle_for_background.as_ref(),
                    &message,
                    "[session_launch] failed to mark Starting materialization failure",
                    "[session_launch] failed to mark coordinator session failed",
                )
                .await;
                return;
            }

            if let Some(turn_intent_id) = initial_turn_intent_id_for_background.as_deref() {
                let input = match AgentOrgRunStore::initial_input(run_id) {
                    Ok(Some(input)) => input,
                    Ok(None) => {
                        handle_background_launch_failure(
                            &session_id_for_background,
                            Some(run_id),
                            project_slug_for_background.as_deref(),
                            work_item_id_for_background.as_deref(),
                            app_handle_for_background.as_ref(),
                            "Starting initial input receipt is missing",
                            "[session_launch] failed to mark missing Starting input",
                            "[session_launch] failed to mark coordinator session failed",
                        )
                        .await;
                        return;
                    }
                    Err(error) => {
                        handle_background_launch_failure(
                            &session_id_for_background,
                            Some(run_id),
                            project_slug_for_background.as_deref(),
                            work_item_id_for_background.as_deref(),
                            app_handle_for_background.as_ref(),
                            &error,
                            "[session_launch] failed to mark Starting input lookup failure",
                            "[session_launch] failed to mark coordinator session failed",
                        )
                        .await;
                        return;
                    }
                };
                let session_id_for_persistence = session_id_for_background.clone();
                let input_for_persistence = input.clone();
                let transcript_result = tokio::task::spawn_blocking(move || {
                    persistence::save_user_msg_with_id(
                        &input_for_persistence.message_id,
                        &session_id_for_persistence,
                        &input_for_persistence.content,
                    )
                    .map(|_| ())
                    .map_err(|error| error.to_string())
                })
                .await
                .map_err(|error| error.to_string())
                .and_then(|result| result);
                let event_result = app_handle_for_background
                    .as_ref()
                    .ok_or_else(|| "App handle is unavailable during Starting".to_string())
                    .and_then(|handle| {
                        crate::bus::event_pipeline_bridge::persist_user_message_event(
                            handle,
                            &session_id_for_background,
                            &input.message_id,
                            &input.content,
                            None,
                            images_for_send.as_deref(),
                            crate::bus::event_pipeline_bridge::PersistedUserMessageSource::User,
                            turn_intent_id,
                        )
                    });
                if let Err(error) = transcript_result.and(event_result) {
                    tracing::warn!(
                        run_id = %run_id,
                        error = %error,
                        "[session_launch] retryable Starting input persistence deferred"
                    );
                    return;
                }
            }

            if let Err(error) = AgentOrgRunStore::finish_starting(run_id, starting_generation) {
                if error.starts_with("materialization_identity_mismatch:")
                    || error.contains("initial input certificate missing")
                    || error.contains("unexpected initial input certificate")
                {
                    handle_background_launch_failure(
                        &session_id_for_background,
                        Some(run_id),
                        project_slug_for_background.as_deref(),
                        work_item_id_for_background.as_deref(),
                        app_handle_for_background.as_ref(),
                        &format!("Starting convergence failed: {error}"),
                        "[session_launch] failed to mark Starting convergence failure",
                        "[session_launch] failed to mark coordinator session failed",
                    )
                    .await;
                } else {
                    tracing::warn!(
                        run_id = %run_id,
                        error = %error,
                        "[session_launch] retryable Starting convergence deferred"
                    );
                }
                return;
            }

            if !has_initial_content {
                return;
            }

            // Title generation runs concurrently — it must not delay the
            // first turn. See `spawn_session_title_generation`.
            spawn_session_title_generation(
                state_for_background.clone(),
                session_id_for_background.clone(),
                std::path::PathBuf::from(&workspace_path_for_send),
                account_id_for_send.clone(),
                model_for_send.clone(),
                native_harness_type_for_send,
                content_for_send.clone(),
            );
            let send_result = send_initial_turn(
                &state_for_background,
                &session_id_for_background,
                content_for_send,
                model_for_send,
                account_id_for_send,
                workspace_path_for_send,
                native_harness_type_for_send,
                mode_for_send,
                images_for_send,
                ide_context_for_send,
                agent_definition_id_for_send,
                sub_agent_ids_for_send,
                agent_org_run_id_for_background.clone(),
                initial_message_id_for_background.clone(),
                initial_turn_intent_id_for_background.clone(),
                crate::foundation::session_bridge::TurnIntentBridgeSource::AgentOrg,
            )
            .await;

            if let Err(err) = send_result {
                let message = format!(
                    "[session_launch] send_message failed for {}: {}",
                    session_id_for_background, err
                );
                handle_background_launch_failure(
                    &session_id_for_background,
                    agent_org_run_id_for_background.as_deref(),
                    project_slug_for_background.as_deref(),
                    work_item_id_for_background.as_deref(),
                    app_handle_for_background.as_ref(),
                    &message,
                    "[session_launch] failed to mark Agent Org run failed",
                    "[session_launch] failed to mark session failed after first-message error",
                )
                .await;
            } else if let Some(turn_intent_id) = initial_turn_intent_id_for_background.as_deref() {
                if let Err(error) =
                    AgentOrgRunStore::mark_initial_input_dispatched(run_id, turn_intent_id)
                {
                    tracing::warn!(
                        run_id,
                        error = %error,
                        "[session_launch] initial input was accepted but dispatch receipt update failed"
                    );
                }
            }
        });

        return Ok(AgentRunLaunchResult {
            session_id,
            status: if has_initial_content {
                LaunchReturnStatus::FirstTurnStarted
            } else {
                LaunchReturnStatus::Idle
            },
            created_at,
            workspace_path: Some(workspace_path).filter(|path| !path.is_empty()),
            worktree_path: existing_worktree_path,
            worktree_branch: existing_worktree_branch,
            base_ref: None,
            agent_org_id,
            agent_org_run_id,
            org_id: request.org_context.org_id.clone(),
            project_id: request.org_context.project_id.clone(),
            project_name: request.org_context.project_name.clone(),
            project_slug,
            work_item_id,
            agent_role,
            product_mode: resolved_product_mode.clone(),
        });
    }

    let prepared_workspace = match prepare_rust_agent_workspace_for_launch(
        &session_id,
        &workspace_path,
        branch.as_deref(),
        isolate,
        existing_worktree_path.as_deref(),
        &additional_directories,
    )
    .await
    {
        Ok(path) => path,
        Err(err) => {
            release_work_item_execution_lock_if_present(
                project_slug.as_deref(),
                work_item_id.as_deref(),
                &session_id,
                state.app_handle.as_ref(),
            )
            .await;
            // The session row was already created by `create_session_impl`
            // above. Workspace preparation failing (e.g. empty workspace_path
            // for a non personal-workspace agent) means this session can never
            // run — delete it so we don't leave a workspace-less orphan that
            // would fail again on its first message.
            cleanup_session_after_org_run_create_failure(session_id.clone()).await;
            return Err(err);
        }
    };

    write_agent_session_marker(
        &prepared_workspace
            .worktree_path
            .clone()
            .unwrap_or_else(|| workspace_path.clone()),
        &session_id,
        agent_definition_id.as_deref(),
        resolved_product_mode.as_deref(),
        project_slug.as_deref(),
        Some(request.org_context.org_id.as_str()),
    );

    let durable_turn_already_accepted = match request.durable_run_id.as_deref() {
        // Any persisted status proves this exact durable intent was accepted
        // previously. Never enqueue it twice. The dispatcher reconciles
        // terminal/pre-durable statuses into Run finality.
        Some(run_id) => {
            crate::foundation::session_bridge::get_turn_intent_status(&session_id, run_id).is_some()
        }
        None => false,
    };

    if has_initial_content && !durable_turn_already_accepted {
        let state_for_send = state.clone();
        let session_id_for_send = session_id.clone();
        let workspace_path_for_send = prepared_workspace
            .worktree_path
            .clone()
            .unwrap_or_else(|| workspace_path.clone());
        let content_for_send = request.content.clone();
        let model_for_send = request.resources.model.clone();
        let account_id_for_send = request.resources.account_id.clone();
        let mode_for_send = request.mode.clone();
        let images_for_send = request.images.clone();
        let ide_context_for_send = request.ide_context.clone();
        let sub_agent_ids_for_send = request.sub_agent_ids.clone();
        let agent_definition_id_for_send = agent_definition_id.clone();
        let agent_org_run_id_for_send = agent_org_run_id.clone();
        let durable_run_id_for_send = request.durable_run_id.clone();
        let project_slug_for_send = project_slug.clone();
        let work_item_id_for_send = work_item_id.clone();
        let app_handle_for_send = state.app_handle.clone();

        let send_task = async move {
            // Title generation runs concurrently — it must not delay the
            // first turn. See `spawn_session_title_generation`.
            spawn_session_title_generation(
                state_for_send.clone(),
                session_id_for_send.clone(),
                std::path::PathBuf::from(&workspace_path_for_send),
                account_id_for_send.clone(),
                model_for_send.clone(),
                native_harness_type_for_send,
                content_for_send.clone(),
            );

            // A plain (non-org) launch's first message IS the user's real
            // request — UserSubmit so downstream consumers (goal loop,
            // org-task resume) treat it as user intent. Org-run launches
            // keep the AgentOrg source.
            let send_result = send_initial_turn(
                &state_for_send,
                &session_id_for_send,
                content_for_send,
                model_for_send,
                account_id_for_send,
                workspace_path_for_send,
                native_harness_type_for_send,
                mode_for_send,
                images_for_send,
                ide_context_for_send,
                agent_definition_id_for_send,
                sub_agent_ids_for_send,
                agent_org_run_id_for_send.clone(),
                durable_run_id_for_send.clone(),
                durable_run_id_for_send,
                crate::foundation::session_bridge::TurnIntentBridgeSource::UserSubmit,
            )
            .await;

            if let Err(err) = send_result {
                let message = format!(
                    "[session_launch] send_message failed for {}: {}",
                    session_id_for_send, err
                );
                handle_background_launch_failure(
                    &session_id_for_send,
                    agent_org_run_id_for_send.as_deref(),
                    project_slug_for_send.as_deref(),
                    work_item_id_for_send.as_deref(),
                    app_handle_for_send.as_ref(),
                    &message,
                    "[session_launch] failed to mark Agent Org run failed",
                    "[session_launch] failed to mark session failed after first-message error",
                )
                .await;
                return Err(message);
            }
            Ok::<(), String>(())
        };

        if request.durable_run_id.is_some() {
            // Durable delivery is acknowledged only after the turn has been
            // accepted by the scheduler. Returning before this await would
            // leave a crash window where the outbox says delivered but no
            // executable turn exists.
            send_task.await?;
        } else {
            tokio::spawn(async move {
                let _ = send_task.await;
            });
        }
    }

    Ok(AgentRunLaunchResult {
        session_id,
        status: if has_initial_content {
            LaunchReturnStatus::FirstTurnStarted
        } else {
            LaunchReturnStatus::Idle
        },
        created_at,
        workspace_path: Some(workspace_path).filter(|path| !path.is_empty()),
        worktree_path: prepared_workspace.worktree_path,
        worktree_branch: prepared_workspace.worktree_branch,
        base_ref: prepared_workspace.base_ref,
        agent_org_id,
        agent_org_run_id,
        org_id: request.org_context.org_id,
        project_id: request.org_context.project_id,
        project_name: request.org_context.project_name,
        project_slug,
        work_item_id,
        agent_role,
        product_mode: resolved_product_mode,
    })
}

const AGENT_ORG_STARTUP_RECOVERY_LIMIT: usize = 100;
static AGENT_ORG_UDW_RECOVERY_RUNNING: AtomicBool = AtomicBool::new(false);

struct UserDirectedRecoveryGuard;

impl Drop for UserDirectedRecoveryGuard {
    fn drop(&mut self) {
        AGENT_ORG_UDW_RECOVERY_RUNNING.store(false, Ordering::Release);
    }
}

/// Run the one-shot Starting/initial-input recovery owner after app state and
/// the EventStore bridge are ready. This is intentionally separate from the
/// periodic Working watchdog.
pub fn spawn_agent_org_startup_recovery(state: AgentAppState) {
    if !crate::coordination::agent_org_runs::agent_org_enabled() {
        return;
    }
    tauri::async_runtime::spawn(async move {
        if let Err(error) = recover_agent_org_starting_runs(&state).await {
            tracing::warn!(error = %error, "[agent-org-startup] Starting recovery failed");
        }
        if let Err(error) = recover_agent_org_initial_dispatches(&state).await {
            tracing::warn!(error = %error, "[agent-org-startup] initial dispatch recovery failed");
        }
        if let Err(error) = recover_agent_org_user_directed_dispatches(&state).await {
            tracing::warn!(error = %error, "[agent-org-startup] direct-work recovery failed");
        }
        if let Err(error) = recover_agent_org_return_continuations(&state).await {
            tracing::warn!(error = %error, "[agent-org-startup] Return continuation recovery failed");
        }
        crate::state::commands::session::org_tasks::schedule_ready_continuations(state);
    });
}

async fn recover_agent_org_return_continuations(state: &AgentAppState) -> Result<(), String> {
    let receipts = tokio::task::spawn_blocking(|| {
        crate::coordination::agent_member_interventions::AgentMemberInterventionStore::dispatchable_return_continuations(
            AGENT_ORG_STARTUP_RECOVERY_LIMIT,
        )
    })
    .await
    .map_err(|error| error.to_string())??;
    for receipt in receipts {
        let Some(turn_intent_id) = receipt.continuation_turn_intent_id.clone() else {
            continue;
        };
        if let Err(error) =
            crate::state::commands::session::org_tasks::dispatch_return_continuation(
                state,
                &receipt,
                &turn_intent_id,
            )
            .await
        {
            tracing::warn!(
                intervention_receipt_id = %receipt.intervention_receipt_id,
                error = %error,
                "[agent-org-startup] Return continuation recovery deferred"
            );
        }
    }
    Ok(())
}

async fn recover_agent_org_user_directed_dispatches(state: &AgentAppState) -> Result<(), String> {
    if AGENT_ORG_UDW_RECOVERY_RUNNING
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Ok(());
    }
    let _single_flight = UserDirectedRecoveryGuard;
    let mut interrupted_after_key: Option<String> = None;
    loop {
        let cursor = interrupted_after_key.clone();
        let changed = tokio::task::spawn_blocking(move || {
            crate::coordination::agent_org_user_directed_work::mark_started_unknown_after_restart_after(
                cursor.as_deref(),
                AGENT_ORG_STARTUP_RECOVERY_LIMIT,
            )
        })
        .await
        .map_err(|error| error.to_string())??;
        if changed.is_empty() {
            break;
        }
        let page_len = changed.len();
        interrupted_after_key = changed.last().cloned();
        if page_len < AGENT_ORG_STARTUP_RECOVERY_LIMIT {
            break;
        }
        tokio::task::yield_now().await;
    }

    let mut direct_after_key = None;
    let mut direct_yield_deadline =
        std::time::Instant::now() + std::time::Duration::from_millis(50);
    loop {
        let cursor = direct_after_key;
        let turns = tokio::task::spawn_blocking(move || {
            crate::coordination::agent_member_interventions::AgentMemberInterventionStore::recoverable_queued_turns_after(
                cursor,
                AGENT_ORG_STARTUP_RECOVERY_LIMIT,
            )
        })
        .await
        .map_err(|error| error.to_string())??;
        if turns.is_empty() {
            break;
        }
        let page_len = turns.len();
        for turn in turns {
            direct_after_key = Some(turn.recovery_key);
            if let Err(error) =
                crate::state::commands::session::message::send_message_impl_for_direct_recovery(
                    state, turn,
                )
                .await
            {
                tracing::warn!(error = %error, "[agent-org-startup] queued direct Turn recovery deferred");
            }
            if std::time::Instant::now() >= direct_yield_deadline {
                tokio::task::yield_now().await;
                direct_yield_deadline =
                    std::time::Instant::now() + std::time::Duration::from_millis(50);
            }
        }
        if page_len < AGENT_ORG_STARTUP_RECOVERY_LIMIT {
            break;
        }
        tokio::task::yield_now().await;
        direct_yield_deadline = std::time::Instant::now() + std::time::Duration::from_millis(50);
    }

    let mut after_key: Option<String> = None;
    let mut yield_deadline = std::time::Instant::now() + std::time::Duration::from_millis(50);
    loop {
        let cursor = after_key.clone();
        let dispatches = tokio::task::spawn_blocking(move || {
            crate::coordination::agent_org_user_directed_work::recoverable_pending_after(
                cursor.as_deref(),
                AGENT_ORG_STARTUP_RECOVERY_LIMIT,
            )
        })
        .await
        .map_err(|error| error.to_string())??;
        if dispatches.is_empty() {
            break;
        }
        let page_len = dispatches.len();
        for dispatch in dispatches {
            after_key = Some(dispatch.recovery_key.clone());
            let wake = crate::tools::impls::orchestration::org_send_message::UserDirectedWake {
                org_run_id: dispatch.org_run_id,
                recipient_member_id: dispatch.recipient_member_id,
                recipient_session_id: dispatch.recipient_session_id,
                turn_intent_id: dispatch.turn_intent_id,
                content: dispatch.content,
                display_text: dispatch.display_text,
                images: dispatch.images,
            };
            if let Err(error) =
                crate::state::commands::session::message::send_message_impl_for_user_directed_wake(
                    state, wake,
                )
                .await
            {
                tracing::warn!(
                    error = %error,
                    "[agent-org-startup] pending linked/group UDW recovery deferred"
                );
            }
            if std::time::Instant::now() >= yield_deadline {
                tokio::task::yield_now().await;
                yield_deadline = std::time::Instant::now() + std::time::Duration::from_millis(50);
            }
        }
        if page_len < AGENT_ORG_STARTUP_RECOVERY_LIMIT {
            break;
        }
        tokio::task::yield_now().await;
        yield_deadline = std::time::Instant::now() + std::time::Duration::from_millis(50);
    }
    Ok(())
}

async fn recover_agent_org_starting_runs(state: &AgentAppState) -> Result<(), String> {
    let runs = tokio::task::spawn_blocking(|| {
        AgentOrgRunStore::list_starting_runs(AGENT_ORG_STARTUP_RECOVERY_LIMIT)
    })
    .await
    .map_err(|error| error.to_string())??;

    for run in runs {
        let Some(root_session_id) = run.root_session_id.as_deref() else {
            AgentOrgRunStore::fail_starting(
                &run.id,
                run.activation_generation,
                &crate::coordination::agent_org_runs::AgentOrgStartingFailure::new(
                    "missing_coordinator_identity",
                    "Starting run has no coordinator Session identity",
                ),
            )?;
            continue;
        };
        let root = persistence::get_session(root_session_id)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| format!("Starting coordinator Session is missing: {root_session_id}"));
        let root = match root {
            Ok(root) => root,
            Err(message) => {
                AgentOrgRunStore::fail_starting(
                    &run.id,
                    run.activation_generation,
                    &crate::coordination::agent_org_runs::AgentOrgStartingFailure::new(
                        "missing_coordinator_identity",
                        message,
                    ),
                )?;
                continue;
            }
        };
        let Some(snapshot_raw) = run.org_snapshot_json.as_deref() else {
            AgentOrgRunStore::fail_starting(
                &run.id,
                run.activation_generation,
                &crate::coordination::agent_org_runs::AgentOrgStartingFailure::new(
                    "missing_launch_snapshot",
                    format!("Starting run {} has no launch snapshot", run.id),
                ),
            )?;
            continue;
        };
        let snapshot: crate::definitions::orgs::AgentOrgLaunchSnapshot =
            match serde_json::from_str(snapshot_raw) {
                Ok(snapshot) => snapshot,
                Err(error) => {
                    AgentOrgRunStore::fail_starting(
                        &run.id,
                        run.activation_generation,
                        &crate::coordination::agent_org_runs::AgentOrgStartingFailure::new(
                            "invalid_launch_snapshot",
                            error.to_string(),
                        ),
                    )?;
                    continue;
                }
            };
        let workspace_path = root
            .worktree_path
            .clone()
            .or_else(|| root.workspace_path.clone())
            .unwrap_or_default();
        if let Err(error) = materialize_org_member_sessions(
            &run.id,
            &snapshot,
            root_session_id,
            &root.name,
            &workspace_path,
            root.model.clone(),
            root.account_id.clone(),
            Some(root.key_source.as_ref().to_string()),
            root.agent_exec_mode.clone(),
            root.native_harness_type.clone(),
            root.work_item_id.clone(),
            root.project_slug.clone(),
        )
        .await
        {
            if !error.is_retryable() {
                AgentOrgRunStore::fail_starting(
                    &run.id,
                    run.activation_generation,
                    error.failure(),
                )?;
                continue;
            }
            tracing::warn!(run_id = %run.id, error = %error, "[agent-org-startup] materialization retry deferred");
            continue;
        }

        let initial_input = match AgentOrgRunStore::initial_input(&run.id) {
            Ok(input) => input,
            Err(error) => {
                tracing::warn!(run_id = %run.id, error = %error, "[agent-org-startup] initial input lookup retry deferred");
                continue;
            }
        };
        if let Some(input) = initial_input {
            if let Err(error) = persist_starting_initial_input(state, root_session_id, &input).await
            {
                if error.starts_with("invalid Starting initial input payload")
                    || error.starts_with("unsupported Starting initial input payload version")
                {
                    AgentOrgRunStore::fail_starting(
                        &run.id,
                        run.activation_generation,
                        &crate::coordination::agent_org_runs::AgentOrgStartingFailure::new(
                            "invalid_initial_input_payload",
                            error,
                        ),
                    )?;
                } else {
                    tracing::warn!(run_id = %run.id, error = %error, "[agent-org-startup] initial input persistence retry deferred");
                }
                continue;
            }
        }
        if let Err(error) = AgentOrgRunStore::finish_starting(&run.id, run.activation_generation) {
            if error.starts_with("materialization_identity_mismatch:")
                || error.contains("initial input certificate missing")
                || error.contains("unexpected initial input certificate")
            {
                AgentOrgRunStore::fail_starting(
                    &run.id,
                    run.activation_generation,
                    &crate::coordination::agent_org_runs::AgentOrgStartingFailure::new(
                        "starting_certificate_invalid",
                        error,
                    ),
                )?;
            } else {
                tracing::warn!(run_id = %run.id, error = %error, "[agent-org-startup] Starting transition retry deferred");
            }
        }
    }
    Ok(())
}

async fn recover_agent_org_initial_dispatches(state: &AgentAppState) -> Result<(), String> {
    let inputs = tokio::task::spawn_blocking(|| {
        AgentOrgRunStore::recoverable_initial_inputs(AGENT_ORG_STARTUP_RECOVERY_LIMIT)
    })
    .await
    .map_err(|error| error.to_string())??;

    for input in inputs {
        let Some(run) = AgentOrgRunStore::load(&input.org_run_id)? else {
            continue;
        };
        let Some(root_session_id) = run.root_session_id.as_deref() else {
            continue;
        };
        let Some(root) =
            persistence::get_session(root_session_id).map_err(|error| error.to_string())?
        else {
            continue;
        };
        persistence::update_status(root_session_id, crate::session::SessionStatus::Idle)
            .map_err(|error| error.to_string())?;
        let workspace_path = root
            .worktree_path
            .clone()
            .or(root.workspace_path.clone())
            .unwrap_or_default();
        let native_harness_type = root
            .native_harness_type
            .as_deref()
            .map(|raw| {
                core_types::providers::NativeHarnessType::parse(raw)
                    .ok_or_else(|| format!("Unknown native_harness_type: {raw:?}"))
            })
            .transpose()?;
        let payload = decode_agent_org_initial_input_payload(&input)?;
        send_initial_turn(
            state,
            root_session_id,
            input.content.clone(),
            root.model,
            root.account_id,
            workspace_path,
            native_harness_type,
            root.agent_exec_mode,
            payload.images,
            payload.ide_context,
            Some(run.coordinator_agent_id),
            payload.sub_agent_ids,
            Some(run.id.clone()),
            Some(input.message_id.clone()),
            Some(input.turn_intent_id.clone()),
            crate::foundation::session_bridge::TurnIntentBridgeSource::AgentOrg,
        )
        .await?;
        AgentOrgRunStore::mark_initial_input_dispatched(&run.id, &input.turn_intent_id)?;
    }
    Ok(())
}

async fn persist_starting_initial_input(
    state: &AgentAppState,
    session_id: &str,
    input: &crate::coordination::agent_org_runs::AgentOrgInitialInput,
) -> Result<(), String> {
    let payload = decode_agent_org_initial_input_payload(input)?;
    let session_id_owned = session_id.to_string();
    let input_owned = input.clone();
    tokio::task::spawn_blocking(move || {
        persistence::save_user_msg_with_id(
            &input_owned.message_id,
            &session_id_owned,
            &input_owned.content,
        )
        .map(|_| ())
        .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())??;
    let handle = state
        .app_handle
        .as_ref()
        .ok_or_else(|| "App handle is unavailable during Starting recovery".to_string())?;
    crate::bus::event_pipeline_bridge::persist_user_message_event(
        handle,
        session_id,
        &input.message_id,
        &input.content,
        None,
        payload.images.as_deref(),
        crate::bus::event_pipeline_bridge::PersistedUserMessageSource::User,
        &input.turn_intent_id,
    )
}
