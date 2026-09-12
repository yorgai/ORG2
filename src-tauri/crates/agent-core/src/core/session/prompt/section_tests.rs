use super::section_builders::{
    build_agent_org_context_section, build_agent_org_context_section_with_task_snapshot,
    build_project_environment, build_rules_section, cap_rule_content, format_user_profile,
};
use crate::coordination::agent_org_run_completion::{
    RunCompletionCandidateAssessment, RunCompletionCandidateState, RunCompletionOutcome,
};
use crate::coordination::agent_org_runs::{
    AgentOrgContextMember, AgentOrgRunContext, AgentOrgRunEntryMode, AgentOrgRunStatus,
    AgentOrgRunStore, CreateAgentOrgRunParams, COORDINATOR_MEMBER_ID,
};
use crate::coordination::agent_org_tasks::{AgentOrgTaskStore, CreateTaskParams, TaskStatus};
use crate::definitions::orgs::{FlatOrgMember, OrgDefinition, PlanApprovalPolicy};
use serial_test::serial;
use test_helpers::test_env;

fn prompt_task_sandbox() -> test_env::SandboxGuard {
    let sandbox = test_env::sandbox();
    let conn = database::db::get_connection().expect("test sqlite connection");
    crate::coordination::agent_org_runs::init_schema(&conn).expect("agent org run schema");
    crate::coordination::agent_org_tasks::init_schema(&conn).expect("agent org task schema");
    sandbox
}

#[test]
fn format_user_profile_renders_non_empty_fields() {
    let profile = crate::session::UserProfile {
        name: Some("Frontend".to_string()),
        tech_savvy: Some("advanced".to_string()),
        job_roles: vec!["Frontend Engineer".to_string(), "Designer".to_string()],
        familiar_tech_stacks: vec!["TypeScript".to_string(), "React".to_string()],
        description: Some("Prefers concise implementation details.".to_string()),
    };

    let rendered = format_user_profile(&profile);

    assert!(rendered.contains("# User Profile"));
    assert!(rendered.contains("Active profile: Frontend"));
    assert!(rendered.contains("Technical familiarity: advanced"));
    assert!(rendered.contains("Job roles: Frontend Engineer, Designer"));
    assert!(rendered.contains("Familiar languages / tech stacks: TypeScript, React"));
    assert!(rendered.contains("About the user: Prefers concise implementation details."));
}

fn prompt_test_agent_org_context() -> AgentOrgRunContext {
    AgentOrgRunContext {
        run_id: "run-prompt-test".to_string(),
        org_id: "org-prompt-test".to_string(),
        org_name: "Prompt Test Org".to_string(),
        org_role: "delivery team".to_string(),
        coordinator_agent_id: "agent-coord".to_string(),
        coordinator_name: "Coordinator".to_string(),
        coordinator_role: "lead".to_string(),
        members: vec![AgentOrgContextMember {
            member_id: "member-worker".to_string(),
            name: "Worker".to_string(),
            role: "implementer".to_string(),
            agent_id: "agent-worker".to_string(),
        }],
        plan_approval_policy: PlanApprovalPolicy::Coordinator,
        capability_index: Default::default(),
        root_session_id: Some("root-prompt-test".to_string()),
    }
}

fn materialize_prompt_test_run(context: &AgentOrgRunContext) -> String {
    AgentOrgRunStore::create(CreateAgentOrgRunParams {
        org_id: context.org_id.clone(),
        coordinator_agent_id: context.coordinator_agent_id.clone(),
        root_session_id: context.root_session_id.clone(),
        org_snapshot: (&OrgDefinition {
            id: context.org_id.clone(),
            name: context.org_name.clone(),
            role: context.org_role.clone(),
            agent_id: context.coordinator_agent_id.clone(),
            description: None,
            plan_approval_policy: context.plan_approval_policy,
            members: context
                .members
                .iter()
                .map(|member| FlatOrgMember {
                    member_id: member.member_id.clone(),
                    name: member.name.clone(),
                    role: member.role.clone(),
                    agent_id: member.agent_id.clone(),
                    runtime_config: None,
                })
                .collect(),
            additional_task_graph_writer_member_ids: Vec::new(),
            member_communication_links: Vec::new(),
        })
            .into(),
        entry_mode: AgentOrgRunEntryMode::StandaloneSession,
        status: AgentOrgRunStatus::Running,
        work_item_id: None,
        project_slug: None,
        routine_fire_id: None,
    })
    .expect("materialize prompt test run")
    .id
}

#[test]
fn agent_org_prompt_uses_only_runtime_member_id_for_identity() {
    // `build_agent_org_context_section` loads the Task board through
    // `AgentOrgTaskStore::list_operational` -> `database::db::get_connection()`,
    // which resolves `ORGII_HOME` at call time. Hold the sandbox so this test
    // never opens a sibling test's fresh sandbox database (a race SQLite
    // reports as an immediate "database is locked") or the real `~/.orgii`.
    let _sandbox = prompt_task_sandbox();
    let mut context = prompt_test_agent_org_context();
    context.coordinator_agent_id = "builtin:sde".to_string();
    context.members[0].agent_id = "builtin:sde".to_string();

    let section = build_agent_org_context_section(&context, "builtin:sde", Some("member-worker"));

    assert!(
            section.contains("Your identity in this org:** member_id `member-worker`"),
            "member prompt must use runtime member_id and not infer coordinator from shared agent id: {section}"
        );
    assert!(
        section.contains("- **Member IDs:**"),
        "prompt must list routable members by member_id: {section}"
    );
    assert!(
        !section.contains("using agent `builtin:sde`"),
        "prompt must not expose agent_id as identity/routing guidance: {section}"
    );
}

#[test]
fn agent_org_prompt_uses_task_board_for_roster_delegation() {
    // `build_agent_org_context_section` loads the Task board through
    // `AgentOrgTaskStore::list_operational` -> `database::db::get_connection()`,
    // which resolves `ORGII_HOME` at call time. Hold the sandbox so this test
    // never opens a sibling test's fresh sandbox database (a race SQLite
    // reports as an immediate "database is locked") or the real `~/.orgii`.
    let _sandbox = prompt_task_sandbox();
    let section = build_agent_org_context_section(
        &prompt_test_agent_org_context(),
        "agent-coord",
        Some(COORDINATOR_MEMBER_ID),
    );

    assert!(
        section.contains("Do NOT use the generic `agent` tool to delegate work to roster members"),
        "prompt must prevent the old generic-agent roster path: {section}"
    );
    assert!(
        section.contains("Use `task_create` and `task_update` only within the task authority"),
        "prompt must point at the authority-checked production task path: {section}"
    );
    assert!(
        section.contains("Task assignment wakes idle members"),
        "prompt must describe member-session reaction semantics: {section}"
    );
    assert!(
        section.contains("Routine progress is not a Coordinator message or assistant reply")
            && section.contains("call the next required tool directly")
            && section.contains("purpose` to `blocker`")
            && section.contains("TaskOutput for completion"),
        "prompt must keep routine progress out of the durable coordination channel: {section}"
    );
    assert!(
        !section.contains("status notes that are not task-state transitions"),
        "prompt must not encourage work-liveblog messages: {section}"
    );
    assert!(
        section.contains("set `eligible_member_ids` to the exact candidates"),
        "prompt must describe eligible_member_ids as a coordinator-validated candidate list: {section}"
    );
    assert!(
        section.contains("required_role` is only a human-readable hint"),
        "prompt must not treat required_role as authorization: {section}"
    );
    assert!(
        section.contains("Every `task_create` must also make a separate scheduling decision"),
        "prompt must require an explicit dispatch policy: {section}"
    );
    assert!(
        section.contains("after_dependencies"),
        "prompt must explain how consumer tasks wait for upstream output: {section}"
    );
    assert!(
        section.contains("requires_dependency_confirmation")
            && section.contains("allow_parallel_with_unlisted_open_tasks=true"),
        "prompt must explain the dependency confirmation handshake: {section}"
    );
    assert!(
        section.contains("role/name, not the member_id prefix alone"),
        "prompt must guide coordinator to use role/name, not member_id prefix: {section}"
    );
    assert!(
        section.contains("For example, planner members are for planning"),
        "prompt must include flexible role-selection examples: {section}"
    );
    assert!(
        !section.contains("Members may set their own unowned task to `in_progress`"),
        "prompt must not preserve old manual self-claim instruction: {section}"
    );
    assert!(
        !section.contains("delegate worker-sized subtasks with the `agent` tool"),
        "prompt must not preserve stale generic-agent delegation instruction: {section}"
    );
    assert!(
        section.contains("Your task authority:** coordinator")
            && section.contains("being allowed to message a peer never grants permission")
            && section.contains("may NOT reassign or rewrite the core goal")
            && section.contains("impersonate its Owner"),
        "prompt must separate communication reachability from task authority: {section}"
    );
    assert!(
        section.contains("first call `task_update` for that exact task id with `operation=start`")
            && section.contains("`output={summary, content?, artifact_ids?}`")
            && section.contains("`summary` is required"),
        "prompt must state the owner-authored task lifecycle contract: {section}"
    );
    assert!(
        section.contains("Messaging is not delegation")
            && section.contains("route the proposal to the coordinator"),
        "prompt must prevent plain-message task-authority bypass: {section}"
    );
    assert!(
        section.contains(
            "Only the coordinator may use `allow_parallel_with_unlisted_open_tasks=true`"
        ),
        "prompt must reserve global dependency override for the coordinator: {section}"
    );
}

#[test]
fn agent_org_prompt_worker_cannot_confuse_peer_chat_with_delegation() {
    // `build_agent_org_context_section` loads the Task board through
    // `AgentOrgTaskStore::list_operational` -> `database::db::get_connection()`,
    // which resolves `ORGII_HOME` at call time. Hold the sandbox so this test
    // never opens a sibling test's fresh sandbox database (a race SQLite
    // reports as an immediate "database is locked") or the real `~/.orgii`.
    let _sandbox = prompt_task_sandbox();
    let context = prompt_test_agent_org_context();
    let section = build_agent_org_context_section(&context, "agent-worker", Some("member-worker"));
    assert!(
        section.contains("Your task authority:** worker")
            && section.contains(
                "the frozen Team snapshot does not grant Writer authority to this member"
            )
            && section.contains("cannot create, assign, or rewrite the Task graph")
            && section.contains("exact Task bound to your persisted TaskExecution turn")
            && section.contains("only you may start it"),
        "worker prompt must explain self-only task authority: {section}"
    );
    assert!(
        section.contains(
            "During UserDirectedWork, a Member may message the Coordinator or a peer linked in the frozen launch snapshot"
        ) && section.contains(
            "During TaskExecution, a Member may message only the Coordinator"
        ),
        "prompt must describe the persisted Turn-specific routing rules: {section}"
    );
}

#[test]
#[serial]
fn agent_org_prompt_includes_bounded_task_snapshot() {
    let _sandbox = prompt_task_sandbox();
    let mut context = prompt_test_agent_org_context();
    context.run_id = materialize_prompt_test_run(&context);
    AgentOrgTaskStore::create(CreateTaskParams {
        id: "prompt-open".to_string(),
        org_run_id: context.run_id.clone(),
        subject: "Open prompt task".to_string(),
        description: String::new(),
        active_form: None,
        owner: Some("member-worker".to_string()),
        status: TaskStatus::InProgress,
        blocks: vec![],
        blocked_by: vec!["prompt-blocker".to_string()],
        metadata: None,
    })
    .unwrap();
    AgentOrgTaskStore::create(CreateTaskParams {
        id: "prompt-done".to_string(),
        org_run_id: context.run_id.clone(),
        subject: "Done prompt task".to_string(),
        description: String::new(),
        active_form: None,
        owner: Some("member-worker".to_string()),
        status: TaskStatus::Completed,
        blocks: vec![],
        blocked_by: vec![],
        metadata: Some(serde_json::json!({
            "output": {
                "summary": "Done prompt result",
                "content": null,
                "artifactIds": [],
                "producedByMemberId": "member-worker",
                "producedAt": "2026-08-20T00:00:00Z"
            }
        })),
    })
    .unwrap();

    let section = build_agent_org_context_section(&context, "agent-coord", None);
    assert!(section.contains("### Current task board snapshot"));
    assert!(section.contains("`prompt-open` [in_progress] owner=member-worker blocked_by=[prompt-blocker] — Open prompt task"));
    assert!(!section.contains("Done prompt task"));
    assert!(section.contains("Terminal history is not loaded into this prompt"));
}

#[test]
#[serial]
fn agent_org_prompt_snapshot_warns_before_duplicate_task_creation() {
    let _sandbox = prompt_task_sandbox();
    let context = prompt_test_agent_org_context();
    let section = build_agent_org_context_section(&context, "agent-coord", None);
    assert!(section.contains("No open tasks currently exist on this run"));
    assert!(section.contains("update it instead of creating a duplicate"));
    assert!(section.contains(
        "Ownerless means waiting for explicit coordinator assignment, never an automatic claim pool"
    ));
    assert!(section.contains("Workers must not set themselves as owner"));
}

#[test]
fn coordinator_ready_snapshot_requires_scope_coverage_before_completion() {
    let context = prompt_test_agent_org_context();
    let section = build_agent_org_context_section_with_task_snapshot(
        &context,
        "agent-coord",
        Some(COORDINATOR_MEMBER_ID),
        Ok(Vec::new()),
        Some(RunCompletionCandidateAssessment {
            state: RunCompletionCandidateState::Ready,
            checked_outcome: RunCompletionOutcome::Delivered,
            activation_generation: Some(1),
            work_revision: Some(9),
            blockers: Vec::new(),
        }),
    );

    assert!(section.contains("state=`ready`"), "{section}");
    assert!(
        section.contains("proves only that the formal Tasks already present"),
        "{section}"
    );
    assert!(
        section.contains("does NOT prove that you created Tasks for every deliverable"),
        "{section}"
    );
    assert!(
        section.contains("`member_idle` event only reports availability"),
        "{section}"
    );
    assert!(
        section.contains("never authorizes recreating completed Tasks"),
        "{section}"
    );
    assert!(
        section.contains("create the missing dependency graph instead of completing the run"),
        "{section}"
    );
    assert!(
        section.contains("Do not refresh with `task_list` or `task_get` merely"),
        "{section}"
    );
    assert!(
        !section.contains("run_summary.completion_ready"),
        "{section}"
    );
    assert!(!section.contains("terminal history is available through `task_list`"));
}

#[test]
fn coordinator_without_active_episode_is_told_idle_team_accepts_new_missions() {
    let context = prompt_test_agent_org_context();
    let section = build_agent_org_context_section_with_task_snapshot(
        &context,
        "agent-coord",
        Some(COORDINATOR_MEMBER_ID),
        Ok(Vec::new()),
        Some(RunCompletionCandidateAssessment {
            state: RunCompletionCandidateState::NotApplicable,
            checked_outcome: RunCompletionOutcome::Delivered,
            activation_generation: Some(5),
            work_revision: Some(29),
            blockers: Vec::new(),
        }),
    );

    assert!(section.contains("state=`not_applicable`"), "{section}");
    assert!(
        section.contains("certificate belongs only to its already-closed episode"),
        "{section}"
    );
    assert!(
        section.contains("it never makes the long-lived Team unavailable"),
        "{section}"
    );
    assert!(section.contains("leave an Idle Team Idle"), "{section}");
    assert!(
        section.contains("atomically reactivates an Idle Team and opens the next work episode"),
        "{section}"
    );
    assert!(
        section.contains("Do not ask the user to reopen or restore the Team in the UI"),
        "{section}"
    );
    assert!(!section.contains("run_unavailable"), "{section}");
}

#[test]
fn agent_org_prompt_lists_llm_callable_message_kinds() {
    // `build_agent_org_context_section` loads the Task board through
    // `AgentOrgTaskStore::list_operational` -> `database::db::get_connection()`,
    // which resolves `ORGII_HOME` at call time. Hold the sandbox so this test
    // never opens a sibling test's fresh sandbox database (a race SQLite
    // reports as an immediate "database is locked") or the real `~/.orgii`.
    let _sandbox = prompt_task_sandbox();
    let section =
        build_agent_org_context_section(&prompt_test_agent_org_context(), "agent-coord", None);

    assert!(section.contains("`plain`"), "plain kind missing: {section}");
    assert!(
        section.contains("`shutdown_request` / `shutdown_response`"),
        "shutdown RPC kinds missing: {section}"
    );
    assert!(
        section.contains("`plan_approval_response`"),
        "plan approval response kind missing: {section}"
    );
    assert!(
        !section.contains("`exec_mode_set_request`"),
        "obsolete remote mode switch must not be advertised: {section}"
    );
}

#[test]
fn agent_org_prompt_explains_member_plan_protocol() {
    // `build_agent_org_context_section` loads the Task board through
    // `AgentOrgTaskStore::list_operational` -> `database::db::get_connection()`,
    // which resolves `ORGII_HOME` at call time. Hold the sandbox so this test
    // never opens a sibling test's fresh sandbox database (a race SQLite
    // reports as an immediate "database is locked") or the real `~/.orgii`.
    let _sandbox = prompt_task_sandbox();
    let section =
        build_agent_org_context_section(&prompt_test_agent_org_context(), "agent-coord", None);

    assert!(
        section.contains("### Planning workflow"),
        "planning workflow section missing: {section}"
    );
    assert!(
        section.contains("execution_mode=plan")
            && section.contains("enters Plan mode automatically"),
        "coordinator prompt must explain assignment-driven Plan mode: {section}"
    );
    assert!(
        section.contains("kind=\"plan_approval_response\"")
            && section.contains("`accepted=true`")
            && section.contains("`accepted=false`"),
        "coordinator prompt must explain approving and rejecting member plans: {section}"
    );
    assert!(
        section.contains("explicitly launched by the user in Plan mode"),
        "prompt must preserve user-facing coordinator/top-level plan semantics: {section}"
    );
    assert!(
        section.contains("never switch the Group chat or coordinator session into Plan mode"),
        "active org planning must use member Plan tasks instead of a root mode switch: {section}"
    );
    assert!(
        section.contains("a lone Plan task is never the complete task graph")
            && section.contains("every requested downstream Build task in the same graph")
            && section.contains("Plan approval closes only the planning deliverable"),
        "multi-stage requests must keep downstream implementation and test work in the formal graph: {section}"
    );
}

#[test]
fn rules_budget_keeps_later_rules_visible() {
    let huge = "a".repeat(60_000);
    let rules = vec![
        ("huge".to_string(), huge),
        ("small".to_string(), "small marker".to_string()),
    ];
    let section = build_rules_section(&rules);
    assert!(section.contains("### huge"));
    assert!(section.contains("### small"));
    assert!(section.contains("small marker"));
    assert!(section.contains("rules budget applied"));
}

#[test]
fn rule_content_truncation_is_utf8_safe() {
    let content = "规则".repeat(30_000);
    let capped = cap_rule_content(&content, 5);
    assert!(capped.is_char_boundary(capped.len()));
    assert!(capped.contains("rule truncated"));
}

#[test]
fn project_env_omits_additional_dirs_when_empty() {
    // negative-half half: sessions that never called
    // `add_workspace_directory` must not emit the block at all,
    // otherwise the prompt cache ping-pongs and every turn
    // pays cold-cache cost.
    let tmp = std::env::temp_dir();
    let out = build_project_environment(&tmp, &[]);
    assert!(
        !out.contains("Additional working directories"),
        "empty additional_dirs must not emit the block: {out}"
    );
}

#[test]
fn project_env_lists_each_additional_dir() {
    // positive-half half: every seeded path must appear in
    // the rendered bullet list. Path literals chosen to be
    // unambiguously test-only so a stray prod match is obvious.
    let tmp = std::env::temp_dir();
    let a = std::path::PathBuf::from("/tmp/pr-f-alpha");
    let b = std::path::PathBuf::from("/tmp/pr-f-beta");
    let dirs: Vec<&std::path::Path> = vec![a.as_path(), b.as_path()];
    let out = build_project_environment(&tmp, &dirs);
    assert!(
        out.contains("- Additional working directories:"),
        "header must be present: {out}"
    );
    assert!(out.contains("/tmp/pr-f-alpha"), "first path missing: {out}");
    assert!(out.contains("/tmp/pr-f-beta"), "second path missing: {out}");
}
