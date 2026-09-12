//! Agent Org run context: identity, task authority, roster, routing rules,
//! planning workflow, and the current task-board snapshot.

use crate::coordination::agent_org_runs::COORDINATOR_MEMBER_ID;
use crate::coordination::agent_org_tasks::{AgentOrgTaskStore, Task, TaskStatus};

const AGENT_ORG_TASK_CONTEXT_LIMIT: usize = 12;

fn format_agent_org_task_for_prompt(task: &Task) -> String {
    const OWNER_PREVIEW_CHARS: usize = 120;
    const BLOCKER_PREVIEW_CHARS: usize = 120;
    const BLOCKER_PREVIEW_COUNT: usize = 3;
    let owner = task
        .owner
        .as_deref()
        .map(|owner| crate::utils::safe_truncate_chars_to_string(owner, OWNER_PREVIEW_CHARS))
        .unwrap_or_else(|| "unclaimed".to_string());
    let blocked = if task.blocked_by.is_empty() {
        "unblocked".to_string()
    } else {
        let preview = task
            .blocked_by
            .iter()
            .take(BLOCKER_PREVIEW_COUNT)
            .map(|id| crate::utils::safe_truncate_chars_to_string(id, BLOCKER_PREVIEW_CHARS))
            .collect::<Vec<_>>()
            .join(",");
        let omitted = task.blocked_by.len().saturating_sub(BLOCKER_PREVIEW_COUNT);
        format!(
            "blocked_by=[{}{}]",
            preview,
            if omitted > 0 {
                format!(",+{omitted} more")
            } else {
                String::new()
            }
        )
    };
    format!(
        "- `{}` [{}] owner={} {} — {}",
        task.id,
        task.status.as_wire(),
        owner,
        blocked,
        task.subject
    )
}

fn build_agent_org_task_snapshot(tasks: Result<Vec<Task>, String>) -> Vec<String> {
    let tasks = match tasks {
        Ok(tasks) => tasks,
        Err(err) => {
            return vec![format!(
                "- Task board snapshot unavailable: {err}. Call `task_list` before changing task state."
            )]
        }
    };
    if tasks.is_empty() {
        return vec!["- No open tasks currently exist on this run. Use the completion-candidate snapshot below to decide whether delivery can be certified; do not re-read the Task board merely to confirm completion.".to_string()];
    }

    let mut open_tasks: Vec<&Task> = tasks.iter().filter(|task| task.status.is_open()).collect();
    open_tasks.sort_by_key(|task| match task.status {
        TaskStatus::InProgress => 0,
        TaskStatus::Pending => 1,
        TaskStatus::Completed | TaskStatus::Failed | TaskStatus::Cancelled => 2,
    });

    let mut lines = Vec::new();
    for task in open_tasks.iter().take(AGENT_ORG_TASK_CONTEXT_LIMIT) {
        lines.push(format_agent_org_task_for_prompt(task));
    }

    let omitted_open = open_tasks.len().saturating_sub(lines.len());
    let dependency_row_count = tasks.len().saturating_sub(open_tasks.len());
    if omitted_open > 0 || dependency_row_count > 0 {
        lines.push(format!(
            "- Operational snapshot: {omitted_open} additional open task(s) omitted from this prompt; {dependency_row_count} direct dependency row(s) were loaded only to evaluate readiness. Terminal history is not loaded here. Use `task_list` before creating duplicate work."
        ));
    } else {
        lines.push(
            "- Terminal history is not loaded into this prompt. Use `task_list` before creating duplicate work."
                .to_string(),
        );
    }
    lines
}

fn build_agent_org_completion_candidate_snapshot(
    assessment: Option<
        &crate::coordination::agent_org_run_completion::RunCompletionCandidateAssessment,
    >,
) -> Vec<String> {
    use crate::coordination::agent_org_run_completion::RunCompletionCandidateState;

    let Some(assessment) = assessment else {
        return Vec::new();
    };
    let blockers = serde_json::to_string(&assessment.blockers)
        .unwrap_or_else(|_| "[{\"kind\":\"validation_error\"}]".to_string());
    let activation_generation = assessment
        .activation_generation
        .map_or_else(|| "null".to_string(), |value| value.to_string());
    let work_revision = assessment
        .work_revision
        .map_or_else(|| "null".to_string(), |value| value.to_string());
    let mut lines = vec![
        "### Completion candidate snapshot".to_string(),
        format!(
            "- state=`{}`; checked_outcome=`delivered`; activation_generation={activation_generation}; work_revision={work_revision}; blockers={blockers}",
            assessment.state.as_wire(),
        ),
    ];
    lines.push(match assessment.state {
        RunCompletionCandidateState::Ready => "- The backend's read-only certificate assessment proves only that the formal Tasks already present in this work episode have durable terminal closure. It does NOT prove that you created Tasks for every deliverable in the user's request or approved plan. Compare the user request and approved plan with the current task-board snapshot now. A routine `member_idle` event only reports availability; it is never new work and never authorizes recreating completed Tasks. If any requested implementation, testing, review, report, or other deliverable truly has no formal Task and TaskOutput, create the missing dependency graph instead of completing the run. Otherwise call `org_run_complete` exactly once with `candidate_outcome=delivered` and your bounded summary. Do not refresh with `task_list` or `task_get` merely to reconfirm closure: the snapshot above is current, and `org_run_complete` will atomically revalidate its revision.".to_string(),
        RunCompletionCandidateState::Blocked => "- Delivery is not yet certifiable. Act only on the explicit blocker types above when you have an authorized action. If no new authorized action is available, end this Turn and wait for a committed Team event; do not poll `task_list`, `task_get`, processes, or timers.".to_string(),
        RunCompletionCandidateState::NotApplicable => "- This Turn has no active formal Task episode. A Delivered, Cancelled, or Failed certificate belongs only to its already-closed episode; it never makes the long-lived Team unavailable. Answer ordinary questions normally and leave an Idle Team Idle. When the user authorizes new formal Team work, create its Task graph: the first committed Task atomically reactivates an Idle Team and opens the next work episode. Do not ask the user to reopen or restore the Team in the UI. Do not call `org_run_complete` until the new episode has formal Tasks to close.".to_string(),
        RunCompletionCandidateState::Certified => "- A current completion certificate already exists. Do not call `org_run_complete` again or create a second delivery claim.".to_string(),
    });
    lines
}

pub fn build_agent_org_context_section(
    context: &crate::coordination::agent_org_runs::AgentOrgRunContext,
    current_agent_id: &str,
    current_member_id: Option<&str>,
) -> String {
    let tasks = AgentOrgTaskStore::list_operational(&context.run_id);
    build_agent_org_context_section_with_task_snapshot(
        context,
        current_agent_id,
        current_member_id,
        tasks,
        None,
    )
}

pub(crate) fn build_agent_org_context_section_with_task_snapshot(
    context: &crate::coordination::agent_org_runs::AgentOrgRunContext,
    _current_agent_id: &str,
    current_member_id: Option<&str>,
    task_snapshot: Result<Vec<Task>, String>,
    completion_candidate: Option<
        crate::coordination::agent_org_run_completion::RunCompletionCandidateAssessment,
    >,
) -> String {
    use crate::definitions::orgs::PlanApprovalPolicy;
    let identity_line = match current_member_id {
        Some(member_id) if context.participant_by_member_id(member_id).is_some() => format!(
            "- **Your identity in this org:** member_id `{member_id}`."
        ),
        Some(member_id) => format!(
            "- **Your identity in this org:** unknown member_id `{member_id}`. You are not a canonical Agent Org participant."
        ),
        None => "- **Your identity in this org:** delegate/shadow worker. You are not a canonical Agent Org participant and you do not have an org member_id.".to_string(),
    };
    let task_authority_line = match current_member_id {
        Some(COORDINATOR_MEMBER_ID) => {
            "- **Your task authority:** coordinator — you may create tasks, edit or assign pending tasks, cancel pending/in-progress tasks, atomically cancel-and-replace changed in-progress work, append terminal audit notes, and approve cross-workflow parallel overrides. You may NOT reassign or rewrite the core goal of an in-progress task in place, or impersonate its Owner: only the bound Owner may start, complete with output, fail, or append progress/evidence. Assignment and dependency unblocking already wake the Owner; do not perform its lifecycle operation on that member's behalf.".to_string()
        }
        Some(member_id)
            if context.participant_by_member_id(member_id).is_some()
                && context.capability_index.is_additional_writer(member_id) =>
        {
            "- **Your task authority:** writer — the immutable Team launch snapshot grants Task graph tools in Direct Member, Group Mention, Member Inbox, and TaskExecution turns. During TaskExecution you may also start, annotate, complete, or fail only the exact Task bound to this persisted turn; Writer authority never permits impersonating another Task owner.".to_string()
        }
        Some(member_id) if context.participant_by_member_id(member_id).is_some() => {
            "- **Your task authority:** worker — the frozen Team snapshot does not grant Writer authority to this member, so you cannot create, assign, or rewrite the Task graph. For the exact Task bound to your persisted TaskExecution turn, only you may start it, append progress/evidence, complete it with output, or fail it with a reason.".to_string()
        }
        _ => "- **Your task authority:** none — non-roster sessions cannot mutate the Agent Org task board.".to_string(),
    };
    let mut lines = vec![
        "## Agent Org Run".to_string(),
        String::new(),
        identity_line,
        format!("- **Run ID:** {}", context.run_id),
        format!("- **Org:** {} (`{}`)", context.org_name, context.org_id),
        format!("- **Org role:** {}", context.org_role),
        "- **Coordinator member_id:** `coordinator`".to_string(),
        "- **Team structure:** flat Coordinator + peer Members".to_string(),
    ];

    if context.members.is_empty() {
        lines.push("- **Members:** none configured".to_string());
    } else {
        lines.push("- **Member IDs:**".to_string());
        for member in &context.members {
            lines.push(format!("  - `{}`", member.member_id));
        }
    }

    lines.push(String::new());
    lines.push("## Team task board".to_string());
    lines.push(String::new());
    lines.push(task_authority_line);
    lines.push(String::new());
    lines.push(
        "Do NOT use the generic `agent` tool to delegate work to roster members in this Agent Org. Roster members are already materialized as persistent sessions for this run. Use `task_create` and `task_update` only within the task authority stated above. Communication reachability and task authority are separate: being allowed to message a peer never grants permission to assign, rewrite, cancel, or complete that peer's work. Use `task_list` / `task_get` to inspect current state before an authorized change."
            .to_string(),
    );
    lines.push(String::new());
    lines.push(
        "For normal worker tasks, set `owner_member_id` for direct assignment to one specific member. An ownerless task is only a parked `awaiting coordinator assignment` state: set `eligible_member_ids` to the exact candidates, but no worker will self-claim or be woken. The coordinator must later choose the owner explicitly. `required_role` is only a human-readable hint and never authorizes a member by itself. Never create worker tasks with neither `owner_member_id` nor `eligible_member_ids`."
            .to_string(),
    );
    lines.push(String::new());
    lines.push(
        "For a new multi-stage request, the coordinator should prefer one `task_graph_create` call: give each node a local key and express the complete dependency graph with `depends_on`. The graph is validated and inserted atomically, so review/test/synthesis work cannot disappear between separate create calls. Use single `task_create` only for a genuinely incremental follow-up or repair. Every `task_create` must also make a separate scheduling decision with `dispatch_policy`. Use `dispatch_policy=immediate` only when the task can start now without another task's result. For review, testing, synthesis, or any consumer work, use `dispatch_policy=after_dependencies` plus `dependency_task_ids=[...]` with all upstream task ids. If a request omits currently-open work, `task_create` returns `requires_dependency_confirmation` or `requires_parallel_confirmation` guidance without creating anything. Add omitted ids when their outputs are needed. Only the coordinator may use `allow_parallel_with_unlisted_open_tasks=true`; members must send the proposed parallel work to the coordinator for approval. Dependent tasks remain pending and receive `TaskAssigned` only after their dependencies complete."
            .to_string(),
    );
    lines.push(
        "When the user names multiple stages or role-owned deliverables (for example plan, then implement, then test, with separate reports), a lone Plan task is never the complete task graph. Create the Plan task and every requested downstream Build task in the same graph before waiting for approval, with the Build tasks depending on the Plan task. Plan approval closes only the planning deliverable; it never proves that implementation, testing, or report deliverables happened."
            .to_string(),
    );
    lines.push(String::new());
    lines.push(
        "Every `task_create` must also set `execution_mode`. Use `execution_mode=plan` only when the task's deliverable is a plan submitted with `create_plan`; use `execution_mode=build` for implementation, writing, review, testing, research, and all other work. The task assignment selects the member's next mode automatically. Inside an active Agent Org, never switch the Group chat or coordinator session into Plan mode in response to phrases such as 'plan then implement'; create a member Plan task instead. Do not send a separate mode-switch message. A Build task that bypasses an open Plan task is rejected for dependency confirmation unless the coordinator explicitly confirms that the work is independent."
            .to_string(),
    );
    lines.push(String::new());
    lines.push(
        "When choosing `eligible_member_ids`, use the roster member's role/name, not the member_id prefix alone. For example, planner members are for planning, decomposition, coordination, checklists, and synthesis. Implementer members are for implementation, writing deliverables, and production artifacts. Reviewer members are for review and quality gates. Tester members are for test execution, verification, and reproduction."
            .to_string(),
    );
    lines.push(String::new());
    lines.push(
        "Task assignment wakes idle members through their normal member-session runtime and queues work for running members without starting a second concurrent turn. Keep ordinary progress in Task state and record completion once through TaskOutput. Do not send routine work narration such as starting work, completed modules, next steps, or problems you already resolved yourself."
            .to_string(),
    );
    lines.push(String::new());
    lines.push(
        "When a member receives `TaskAssigned`, it must first call `task_update` for that exact task id with `operation=start` before doing the work. When done, the same owning member must call `task_update` with `operation=complete` and `output={summary, content?, artifact_ids?}`; `summary` is required. If execution fails, use `operation=fail` with a bounded reason. Coordinators and managers must never perform these lifecycle/output calls for another owner. At turn end, the runtime gives a worker at most one bounded correction if a Build task is still `in_progress`; if it remains unresolved, `MemberIdle.unfinished_task_ids` tells the coordinator to retry or replace instead of waiting silently. Plan tasks awaiting approval are excluded."
            .to_string(),
    );
    lines.push(String::new());
    if current_member_id == Some(COORDINATOR_MEMBER_ID) {
        lines.push(
            "The atomic completion-candidate snapshot below is the only prompt-level readiness guidance. It is not a certificate: only `org_run_complete` can revalidate and create delivery authority. Never announce Delivered from open-task counts, free text, or Quiescence alone."
                .to_string(),
        );
        lines.push(String::new());
    }
    lines.push(
        "When you receive `MemberIdle` with non-empty `unfinished_task_ids`, do not wait silently: ask that owner to finish its lifecycle or use `operation=cancel_and_replace` for changed in-progress work. When `reason=failed`, the failed member's in-progress tasks become ownerless Pending rows; inspect eligibility and choose a new owner explicitly with `task_update operation=patch_pending owner_member_id=...`. Workers never self-claim ownerless work. Never assign outside `eligible_member_ids`, and do not ask one member to inspect another member's private failed context. If no recovery is possible, pause and report to the user."
            .to_string(),
    );
    lines.push(String::new());
    lines.push(
        "Before creating a task, compare against the snapshot below and call `task_list` when uncertain. If a task already exists, update it instead of creating a duplicate. Ownerless means waiting for explicit coordinator assignment, never an automatic claim pool. Workers must not set themselves as owner or set an ownerless task to `in_progress`; the coordinator first chooses `owner_member_id`, then normal TaskAssigned delivery wakes only that owner."
            .to_string(),
    );
    lines.push(String::new());
    lines.push(
        "Choose skills and tools from the user's actual request. For non-code work such as summaries, research, or writing, do not invoke GitHub issue-fix, repository, or code-audit workflows merely because those tools are available."
            .to_string(),
    );
    lines.push(String::new());
    lines.push("### Current task board snapshot".to_string());
    lines.extend(build_agent_org_task_snapshot(task_snapshot));
    if current_member_id == Some(COORDINATOR_MEMBER_ID) {
        lines.push(String::new());
        lines.extend(build_agent_org_completion_candidate_snapshot(
            completion_candidate.as_ref(),
        ));
    }
    lines.push(String::new());
    lines.push("## Org messaging".to_string());
    lines.push(String::new());
    lines.push(
        "Use the `org_send_message` tool to send a typed org message to exactly one coordinator/member participant in this org. The only routing field is `recipient_member_id`; never route by display name or agent id. Messages are persisted and surfaced to the recipient on its next turn — they do not interrupt the recipient's current turn. Every plain message to a non-coordinator worker must include `related_task_id` for unresolved, dependency-ready work already owned by that worker. Eligibility alone is not assignment; the coordinator must set `owner_member_id` before sending formal work instructions. A TaskExecution member may send plain text to the Coordinator only when the Coordinator must act: bind the exact current `related_task_id` and set `purpose` to `blocker`, `decision_required`, `material_change`, `risk`, or `requested_reply`. `requested_reply` is only for an explicit mid-task reply requested by the Coordinator. Chat cannot create invisible work or bypass dependencies.".to_string(),
    );

    lines.push(String::new());
    lines.push(
        "**Routing:** the Coordinator and every Member are always mutually reachable. During UserDirectedWork, a Member may message the Coordinator or a peer linked in the frozen launch snapshot. During TaskExecution, a Member may message only the Coordinator."
            .to_string(),
    );
    lines.push(String::new());
    lines.push(
        "**Messaging is not delegation.** Do not use a `plain` message to bypass task authority by telling a peer or another branch to start formal work. Use messages for questions, discussion, handoff context, and proposals. Formal work must already exist as an authority-checked task; if an unauthorized peer asks you to start new work, route the proposal to the coordinator instead of silently creating or executing a second task chain."
            .to_string(),
    );
    lines.push(String::new());
    lines.push(
        "**Routine progress is not a Coordinator message or assistant reply.** The activity UI already shows tool calls. During TaskExecution, call the next required tool directly: do not preface or accompany tools with assistant prose about starting, retries, finished modules, next steps, or self-resolved problems, and never prefix such prose with `@Coordinator`. Continue working when you can resolve an issue yourself without changing scope, dependencies, approach, or risk. Reserve normal text for the compact final result after the terminal Task update; use Task state for progress and `task_update operation=complete` with TaskOutput for completion."
            .to_string(),
    );
    lines.push(String::new());
    lines.push(
        "**Your normal text output is NOT visible to other agents in this org.** To communicate with another org participant you MUST call `org_send_message` with a listed `recipient_member_id`. Writing the message in your reply alone reaches the user, not the agent.".to_string(),
    );
    lines.push(String::new());
    lines.push(
        "Available message kinds: `plain` (free-form text — the common case), `shutdown_request` / `shutdown_response` (coordinator-driven graceful stop RPC — pair them with a sender-generated `request_id` the responder must echo), and, when this run uses coordinator plan approval, `plan_approval_response` (echo the plan request_id and set accepted/feedback). orgii's user permission and user mode-switch systems are separate; do NOT encode user-facing permission prompts as org messages.".to_string(),
    );
    lines.push(String::new());
    lines.push("### Planning workflow".to_string());
    lines.push(String::new());
    lines.push(
        "If a member must draft an implementation plan, risk review, migration plan, architecture proposal, or phased design, create its task with `execution_mode=plan`. The member enters Plan mode automatically, submits through `create_plan`, and stops. Approval completes that planning task and unlocks tasks that depend on it; the Planner is not woken into a fake Build turn.".to_string(),
    );
    lines.push(String::new());
    lines.push(match context.plan_approval_policy {
        PlanApprovalPolicy::Coordinator => "This run uses coordinator plan approval. When a member submits `create_plan`, review the durable inbox request, then send `kind=\"plan_approval_response\"` with the same `request_id`. `accepted=true` completes the source planning task and unlocks its dependants. `accepted=false` requires concrete `feedback` and wakes the Planner once in Plan mode for revision.".to_string(),
        PlanApprovalPolicy::User => "This run uses user plan approval. A submitted member plan appears in Group chat. Do not manufacture approval messages or bypass the gate; wait quietly until the user approves, edits and approves, or requests changes.".to_string(),
        PlanApprovalPolicy::Automatic => "This run uses automatic plan approval. A valid `create_plan` submission completes the source planning task immediately and unlocks its dependants; no coordinator approval message is needed.".to_string(),
    });
    lines.push(String::new());
    lines.push(
        "A root session explicitly launched by the user in Plan mode remains a separate, user-selected workflow and may use the coordinator's own `create_plan` Build approval surface. Once an Agent Org run has launched in Build mode, keep the coordinator in Build mode and use member Plan tasks. Only non-coordinator member plans use the internal coordinator approval path.".to_string(),
    );
    lines.join("\n")
}
