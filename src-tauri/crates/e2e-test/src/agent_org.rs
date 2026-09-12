//! Inter-agent communication scenarios.
//!
//! These pin the `OrgSendMessageTool` contract exercised by the
//! `/agent/test/agent-org/send-message-direct` helper-isolation probe
//! and cross-checked against the bounded
//! `/agent/test/agent-org/inbox/list-by-run` helper-isolation probe. All scenarios are
//! deterministic — they bypass the coordinator-launch / LLM cycle
//! (~30s/turn) and exercise only the synchronous Rust path:
//! `recipient` resolution, payload validation, and `agent_inbox` row
//! tagging.
//!
//! Pairing strategy: positive AND negative pins per behavior. Every
//! successful send is followed by a `list-by-run` read so a future
//! refactor of either side surfaces here, not just in unit tests.
//! User-visible Direct work and Return to Work coverage lives in packaged,
//! rendered UI E2E. This deterministic suite does not expose an HTTP action
//! bridge that could substitute for the product controls.

use super::config::Config;
use super::harness;

const SEND_MESSAGE_PATH: &str = "/agent/test/agent-org/send-message-direct";
const LIST_INBOX_PATH: &str = "/agent/test/agent-org/inbox/list-by-run";
const DRAIN_INBOX_PATH: &str = "/agent/test/agent-org/drain-inbox";
const CHECK_MEMBER_SPAWN_GATE_PATH: &str = "/agent/test/agent-org/check-member-spawn-gate";
const POST_MEMBER_IDLE_PATH: &str = "/agent/test/agent-org/post-member-idle";
const SEED_ORG_PATH: &str = "/agent/test/agent-org/seed";
const LAUNCH_COORDINATOR_PATH: &str = "/agent/test/agent-org/launch-coordinator";
const RUN_SEED_PATH: &str = "/agent/test/agent-org/run/seed";
const E2E_RUN_FIXTURE_ORG_PREFIX: &str = "e2e-agent-org-fixture:";
const RUN_VIEW_PATH: &str = "/agent/test/agent-org/run-view";
const DURABLE_INVARIANTS_PATH: &str = "/agent/test/agent-org/durable-invariants";
const FIND_WORKER_SESSION_PATH: &str = "/agent/test/agent-org/find-worker-session";
const SEED_CLI_MEMBER_RUN_PATH: &str = "/agent/test/agent-org/stale-workers/seed-cli-member";
const SEED_CRASHED_TASK_EXECUTION_PATH: &str =
    "/agent/test/agent-org/startup-recovery/seed-crashed-task";
const TASKS_SEED_PATH: &str = "/agent/test/agent-org/tasks/seed";
const PAUSE_RUN_PATH: &str = "/agent/test/agent-org/run/pause";
const RESUME_RUN_PATH: &str = "/agent/test/agent-org/run/resume";
const SIMULATE_APP_RESTART_PATH: &str = "/agent/test/agent-org/simulate-app-restart";
const SESSION_UPDATE_STATUS_PATH: &str = "/agent/test/session/update-status-via-cmd";
const SESSION_STATUS_FAILED: &str = "failed";
const TASK_STATUS_PENDING: &str = "pending";
const TASK_STATUS_IN_PROGRESS: &str = "in_progress";
const TASK_STATUS_COMPLETED: &str = "completed";

pub(super) fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .expect("client")
}

/// Default 1-coordinator + 2-worker org context body. `org_run_id` and
/// `sender_agent_id` are required at the call site so each scenario
/// gets a fresh, isolated inbox slice and self-routing semantics are
/// explicit.
fn default_context(run_id: &str, sender: &str) -> serde_json::Value {
    let mut body = serde_json::json!({
        "org_run_id": run_id,
        "org_id": "test-org-2b3a",
        "org_name": "Searcher Org",
        "org_role": "team",
        "coordinator_agent_id": "coord",
        "coordinator_name": "Searcher Org",
        "coordinator_role": "lead",
        "members": [
            {
                "member_id": "m1",
                "name": "Alice",
                "role": "worker",
                "agent_id": "alice-agent",
            },
            {
                "member_id": "m2",
                "name": "Bob",
                "role": "worker",
                "agent_id": "bob-agent",
            },
        ],
        "sender_agent_id": sender,
    });
    if let Some(member_id) = default_member_id_for_agent(sender) {
        body.as_object_mut()
            .expect("default_context returns object")
            .insert(
                "sender_member_id".to_string(),
                serde_json::Value::String(member_id.to_string()),
            );
    }
    body
}

fn default_member_id_for_agent(agent_id: &str) -> Option<&'static str> {
    match agent_id {
        "alice-agent" => Some("m1"),
        "bob-agent" => Some("m2"),
        "coord" => Some("coordinator"),
        _ => None,
    }
}

pub(super) async fn post_send(
    cfg: &Config,
    body: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let url = format!("{}{}", cfg.base_url, SEND_MESSAGE_PATH);
    let resp = http_client()
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|err| format!("HTTP error: {err}"))?;
    resp.json::<serde_json::Value>()
        .await
        .map_err(|err| format!("JSON parse error: {err}"))
}

pub(super) async fn list_inbox(cfg: &Config, run_id: &str) -> Result<serde_json::Value, String> {
    let url = format!("{}{}", cfg.base_url, LIST_INBOX_PATH);
    let resp = http_client()
        .post(&url)
        .json(&serde_json::json!({ "org_run_id": run_id }))
        .send()
        .await
        .map_err(|err| format!("HTTP error: {err}"))?;
    resp.json::<serde_json::Value>()
        .await
        .map_err(|err| format!("JSON parse error: {err}"))
}

async fn drain_inbox(
    cfg: &Config,
    run_id: &str,
    recipient_agent_id: &str,
) -> Result<serde_json::Value, String> {
    drain_inbox_with_body(cfg, default_drain_body(run_id, recipient_agent_id)).await
}

fn default_drain_body(run_id: &str, recipient_agent_id: &str) -> serde_json::Value {
    let mut body = default_context(run_id, "");
    let obj = body
        .as_object_mut()
        .expect("default_context returns object");
    obj.remove("sender_agent_id");
    obj.insert(
        "recipient_agent_id".to_string(),
        serde_json::Value::String(recipient_agent_id.to_string()),
    );
    if let Some(member_id) = default_member_id_for_agent(recipient_agent_id) {
        obj.insert(
            "recipient_member_id".to_string(),
            serde_json::Value::String(member_id.to_string()),
        );
    }
    body
}

async fn drain_inbox_with_body(
    cfg: &Config,
    body: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let url = format!("{}{}", cfg.base_url, DRAIN_INBOX_PATH);
    let resp = http_client()
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|err| format!("HTTP error: {err}"))?;
    resp.json::<serde_json::Value>()
        .await
        .map_err(|err| format!("JSON parse error: {err}"))
}

pub(super) fn unique_run_id(label: &str) -> String {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{E2E_RUN_FIXTURE_ORG_PREFIX}{label}-{ts}")
}

fn member_id_for_legacy_recipient_name(name: &str) -> &str {
    match name {
        "Alice" => "m1",
        "Bob" => "m2",
        "Searcher Org" => "coordinator",
        other => other,
    }
}

fn member_id_for_legacy_recipient_agent_id(agent_id: &str) -> &str {
    match agent_id {
        "alice-agent" => "m1",
        "bob-agent" => "m2",
        "coord" => "coordinator",
        other => other,
    }
}

fn normalize_send_params_for_member_routing(mut params: serde_json::Value) -> serde_json::Value {
    let Some(obj) = params.as_object_mut() else {
        return params;
    };
    if obj.get("recipient_member_id").is_some() {
        return params;
    }
    let member_id = obj
        .get("recipient_name")
        .and_then(|value| value.as_str())
        .map(member_id_for_legacy_recipient_name)
        .or_else(|| {
            obj.get("recipient_agent_id")
                .and_then(|value| value.as_str())
                .map(member_id_for_legacy_recipient_agent_id)
        });
    if let Some(member_id) = member_id {
        obj.insert(
            "recipient_member_id".to_string(),
            serde_json::Value::String(member_id.to_string()),
        );
    }
    params
}

fn build_send_body(run_id: &str, sender: &str, params: serde_json::Value) -> serde_json::Value {
    let mut body = default_context(run_id, sender);
    body.as_object_mut()
        .expect("default_context returns an object")
        .insert(
            "params".to_string(),
            normalize_send_params_for_member_routing(params),
        );
    body
}

/// Helper: extract the messages array from a `list-by-run` response, or
/// emit a parse-error string the scenario can surface via `print_error`.
pub(super) fn messages_array(resp: &serde_json::Value) -> Result<&Vec<serde_json::Value>, String> {
    let ok = resp.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
    if !ok {
        let err = resp
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown error");
        return Err(format!("list-by-run returned ok=false: {err}"));
    }
    resp.get("messages")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "list-by-run response missing messages array".to_string())
}

async fn post_agent_org_json(
    cfg: &Config,
    path: &str,
    body: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let url = format!("{}{}", cfg.base_url, path);
    let resp = http_client()
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|err| format!("HTTP error ({path}): {err}"))?;
    resp.json::<serde_json::Value>()
        .await
        .map_err(|err| format!("JSON parse error ({path}): {err}"))
}

/// Agent Org member sessions are materialized in a background task after the
/// coordinator launch commits. Runtime scenarios must therefore wait for the
/// durable member row instead of treating the first `found: false` response as
/// a product failure.
async fn wait_for_worker_session(
    cfg: &Config,
    org_run_id: &str,
    member_id: &str,
) -> Result<serde_json::Value, String> {
    const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
    const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(25);

    let started_at = std::time::Instant::now();
    loop {
        let response = post_agent_org_json(
            cfg,
            FIND_WORKER_SESSION_PATH,
            serde_json::json!({
                "org_run_id": org_run_id,
                "member_id": member_id,
            }),
        )
        .await?;
        if response.get("ok").and_then(serde_json::Value::as_bool) != Some(true) {
            return Err(format!(
                "worker-session lookup failed for member {member_id}: {response}"
            ));
        }
        if response.get("found").and_then(serde_json::Value::as_bool) == Some(true) {
            return Ok(response);
        }
        if started_at.elapsed() >= TIMEOUT {
            return Err(format!(
                "timed out waiting for member session materialization: run={org_run_id} member={member_id} last_response={response}"
            ));
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

async fn seed_task(
    cfg: &Config,
    run_id: &str,
    id: &str,
    subject: &str,
    owner: &str,
    status: &str,
) -> Result<serde_json::Value, String> {
    seed_task_with_owner(cfg, run_id, id, subject, Some(owner), status).await
}

async fn seed_unowned_task(
    cfg: &Config,
    run_id: &str,
    id: &str,
    subject: &str,
    status: &str,
) -> Result<serde_json::Value, String> {
    seed_task_with_owner(cfg, run_id, id, subject, None, status).await
}

async fn seed_task_with_owner(
    cfg: &Config,
    run_id: &str,
    id: &str,
    subject: &str,
    owner: Option<&str>,
    status: &str,
) -> Result<serde_json::Value, String> {
    let mut body = serde_json::json!({
        "id": id,
        "org_run_id": run_id,
        "subject": subject,
        "description": "",
        "status": status,
        "blocks": [],
        "blocked_by": []
    });
    if let Some(owner) = owner {
        body.as_object_mut()
            .expect("task seed body is object")
            .insert(
                "owner".to_string(),
                serde_json::Value::String(owner.to_string()),
            );
    }
    let resp = post_agent_org_json(cfg, TASKS_SEED_PATH, body).await?;
    if resp.get("ok").and_then(|value| value.as_bool()) != Some(true) {
        return Err(format!("tasks/seed rejected payload: {resp}"));
    }
    Ok(resp)
}

/// Create the durable Running run required by production message guards, then
/// pin it open with one real pending task so the background finality pass
/// cannot legitimately complete an otherwise empty synthetic fixture.
async fn seed_default_running_run(
    cfg: &Config,
    label: &str,
    pending_plan_request_id: Option<&str>,
) -> Result<String, String> {
    let fixture_id = unique_run_id(label);
    let mut body = default_context(&fixture_id, "");
    let obj = body
        .as_object_mut()
        .expect("default_context returns object");
    obj.remove("sender_agent_id");
    obj.insert(
        "org_id".to_string(),
        serde_json::Value::String(fixture_id.clone()),
    );
    let response = post_agent_org_json(cfg, RUN_SEED_PATH, body).await?;
    if response.get("ok").and_then(serde_json::Value::as_bool) != Some(true) {
        return Err(format!("run/seed rejected payload: {response}"));
    }
    let run_id = response
        .get("org_run_id")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| format!("run/seed omitted org_run_id: {response}"))?;

    let keep_open_task_id = format!("keep-open-{fixture_id}");
    if let Some(request_id) = pending_plan_request_id {
        let approval_seed = post_agent_org_json(
            cfg,
            TASKS_SEED_PATH,
            serde_json::json!({
                "id": keep_open_task_id,
                "org_run_id": run_id,
                "subject": "Plan deterministic typed-message fixture",
                "description": "Create one durable pending plan approval",
                "owner": "m1",
                "status": TASK_STATUS_IN_PROGRESS,
                "blocks": [],
                "blocked_by": [],
                "execution_mode": "plan",
                "pending_plan_approval": {
                    "request_id": request_id,
                    "source_member_id": "m1"
                }
            }),
        )
        .await?;
        if approval_seed.get("ok").and_then(serde_json::Value::as_bool) != Some(true) {
            return Err(format!(
                "tasks/seed could not create pending plan approval: {approval_seed}"
            ));
        }
    } else {
        seed_task(
            cfg,
            &run_id,
            &keep_open_task_id,
            "Keep deterministic message fixture open",
            "m2",
            TASK_STATUS_PENDING,
        )
        .await?;
    }
    Ok(run_id)
}

async fn set_session_status(
    cfg: &Config,
    session_id: &str,
    status: &str,
) -> Result<serde_json::Value, String> {
    let resp = post_agent_org_json(
        cfg,
        SESSION_UPDATE_STATUS_PATH,
        serde_json::json!({
            "session_id": session_id,
            "status": status,
        }),
    )
    .await?;
    if resp.get("ok").and_then(|value| value.as_bool()) != Some(true) {
        return Err(format!("session status update rejected payload: {resp}"));
    }
    Ok(resp)
}

async fn durable_invariants(cfg: &Config, org_run_id: &str) -> Result<serde_json::Value, String> {
    let resp = post_agent_org_json(
        cfg,
        DURABLE_INVARIANTS_PATH,
        serde_json::json!({ "org_run_id": org_run_id }),
    )
    .await?;
    if resp.get("ok").and_then(|value| value.as_bool()) != Some(true) {
        return Err(format!("durable invariant probe rejected payload: {resp}"));
    }
    Ok(resp)
}

fn tmp_agent_org_workspace(label: &str) -> String {
    let suffix = unique_run_id(label).replace(':', "-");
    let path = std::env::temp_dir().join(format!("orgii-{suffix}"));
    std::fs::create_dir_all(&path).expect("create e2e agent org workspace");
    path.to_string_lossy().to_string()
}

// ── Success cases ────────────────────────────────────────────────────

/// Caller-path launch/read-model pin for the member-first Agent Org model.
/// Seeds a real org definition, launches through `session_launch_impl`, and
/// asserts the production run-view helper sees materialized member sessions.
pub async fn launch_materializes_member_sessions_in_run_view(cfg: &Config) -> bool {
    let label = "Agent-Org: launch materializes member sessions in run view";
    let org_id = unique_run_id("materialized-org");
    let alice_agent = "builtin:explore";
    let bob_agent = "builtin:sde";

    let seed_resp = match post_agent_org_json(
        cfg,
        SEED_ORG_PATH,
        serde_json::json!({
            "id": org_id,
            "name": "Materialized Org",
            "coordinator_agent_id": "builtin:sde",
            "members": [
                { "id": "m-alice", "name": "Alice", "role": "worker", "agent_id": alice_agent },
                { "id": "m-bob", "name": "Bob", "role": "worker", "agent_id": bob_agent }
            ]
        }),
    )
    .await
    {
        Err(err) => return harness::print_error(label, &err),
        Ok(json) => json,
    };
    if seed_resp.get("ok").and_then(|value| value.as_bool()) != Some(true) {
        return harness::print_error(label, &seed_resp.to_string());
    }

    let workspace_path = tmp_agent_org_workspace("materialized-members");
    let launch_resp = match post_agent_org_json(
        cfg,
        LAUNCH_COORDINATOR_PATH,
        serde_json::json!({
            "agent_org_id": org_id,
            "workspace_path": workspace_path,
            "content": "",
            "model": cfg.model,
            "account_id": cfg.account_id,
            "name": "Materialized Org E2E",
            "sync_turn": false
        }),
    )
    .await
    {
        Err(err) => return harness::print_error(label, &err),
        Ok(json) => json,
    };
    let launch_ok = launch_resp.get("ok").and_then(|value| value.as_bool()) == Some(true);
    let session_id = match launch_resp
        .get("session_id")
        .and_then(|value| value.as_str())
    {
        Some(value) if !value.is_empty() => value.to_string(),
        _ => return harness::print_error(label, &launch_resp.to_string()),
    };
    let org_run_id = match launch_resp
        .get("agent_org_run_id")
        .and_then(|value| value.as_str())
    {
        Some(value) if !value.is_empty() => value.to_string(),
        _ => return harness::print_error(label, &launch_resp.to_string()),
    };

    let alice_lookup = match wait_for_worker_session(cfg, &org_run_id, "m-alice").await {
        Err(err) => return harness::print_error(label, &err),
        Ok(json) => json,
    };
    let bob_lookup = match wait_for_worker_session(cfg, &org_run_id, "m-bob").await {
        Err(err) => return harness::print_error(label, &err),
        Ok(json) => json,
    };

    let run_view_resp = match post_agent_org_json(
        cfg,
        RUN_VIEW_PATH,
        serde_json::json!({ "session_id": session_id }),
    )
    .await
    {
        Err(err) => return harness::print_error(label, &err),
        Ok(json) => json,
    };

    let view = run_view_resp.get("view");
    let members = view
        .and_then(|value| value.get("members"))
        .and_then(|value| value.as_array());
    let member_count_ok = members.map(|items| items.len() == 3).unwrap_or(false);

    let member_runtime_ok = |member_id: &str| -> bool {
        members
            .and_then(|items| {
                items.iter().find(|member| {
                    member.get("memberId").and_then(|value| value.as_str()) == Some(member_id)
                })
            })
            .and_then(|member| member.get("sessionRuntime"))
            .and_then(|runtime| runtime.get("sessionId"))
            .and_then(|value| value.as_str())
            .map(|value| !value.is_empty())
            .unwrap_or(false)
    };

    let alice_found = alice_lookup.get("found").and_then(|value| value.as_bool()) == Some(true);
    let bob_found = bob_lookup.get("found").and_then(|value| value.as_bool()) == Some(true);
    let run_view_ok = run_view_resp.get("ok").and_then(|value| value.as_bool()) == Some(true)
        && run_view_resp.get("found").and_then(|value| value.as_bool()) == Some(true);

    harness::print_result(
        label,
        &run_view_resp.to_string(),
        &[
            ("launch endpoint returned ok", launch_ok),
            ("Alice materialized in agent_sessions", alice_found),
            ("Bob materialized in agent_sessions", bob_found),
            ("run view endpoint returned a view", run_view_ok),
            ("run view includes coordinator + 2 members", member_count_ok),
            (
                "Alice member row has sessionRuntime.sessionId",
                member_runtime_ok("m-alice"),
            ),
            (
                "Bob member row has sessionRuntime.sessionId",
                member_runtime_ok("m-bob"),
            ),
        ],
    )
}

/// Run-view task counts distinguish queued owned work from the currently active
/// task. This pins the read model needed for running members: assigning more
/// work to a member that already has an in-progress task must be visible as a
/// pending queue, not as a second active turn.
pub async fn run_view_distinguishes_pending_and_in_progress_tasks(cfg: &Config) -> bool {
    let label = "Agent-Org: run view separates pending and in-progress tasks";
    let org_id = unique_run_id("task-count-org");
    let alice_agent = "builtin:explore";

    let seed_resp = match post_agent_org_json(
        cfg,
        SEED_ORG_PATH,
        serde_json::json!({
            "id": org_id,
            "name": "Task Count Org",
            "coordinator_agent_id": "builtin:sde",
            "members": [
                { "id": "m-alice", "name": "Alice", "role": "worker", "agent_id": alice_agent }
            ]
        }),
    )
    .await
    {
        Err(err) => return harness::print_error(label, &err),
        Ok(json) => json,
    };
    if seed_resp.get("ok").and_then(|value| value.as_bool()) != Some(true) {
        return harness::print_error(label, &seed_resp.to_string());
    }

    let workspace_path = tmp_agent_org_workspace("task-counts");
    let launch_resp = match post_agent_org_json(
        cfg,
        LAUNCH_COORDINATOR_PATH,
        serde_json::json!({
            "agent_org_id": org_id,
            "workspace_path": workspace_path,
            "content": "",
            "model": cfg.model,
            "account_id": cfg.account_id,
            "name": "Task Count Org E2E",
            "sync_turn": false
        }),
    )
    .await
    {
        Err(err) => return harness::print_error(label, &err),
        Ok(json) => json,
    };
    let session_id = match launch_resp
        .get("session_id")
        .and_then(|value| value.as_str())
    {
        Some(value) if !value.is_empty() => value.to_string(),
        _ => return harness::print_error(label, &launch_resp.to_string()),
    };
    let org_run_id = match launch_resp
        .get("agent_org_run_id")
        .and_then(|value| value.as_str())
    {
        Some(value) if !value.is_empty() => value.to_string(),
        _ => return harness::print_error(label, &launch_resp.to_string()),
    };

    if let Err(err) = seed_task(
        cfg,
        &org_run_id,
        "task-active",
        "Current active work",
        "m-alice",
        TASK_STATUS_IN_PROGRESS,
    )
    .await
    {
        return harness::print_error(label, &err);
    }
    if let Err(err) = seed_task(
        cfg,
        &org_run_id,
        "task-queued",
        "Queued follow-up work",
        "m-alice",
        TASK_STATUS_PENDING,
    )
    .await
    {
        return harness::print_error(label, &err);
    }

    let run_view_resp = match post_agent_org_json(
        cfg,
        RUN_VIEW_PATH,
        serde_json::json!({ "session_id": session_id }),
    )
    .await
    {
        Err(err) => return harness::print_error(label, &err),
        Ok(json) => json,
    };

    let alice_member = run_view_resp
        .get("view")
        .and_then(|value| value.get("members"))
        .and_then(|value| value.as_array())
        .and_then(|members| {
            members.iter().find(|member| {
                member.get("memberId").and_then(|value| value.as_str()) == Some("m-alice")
            })
        });
    let pending_count = alice_member
        .and_then(|member| member.get("pendingTaskCount"))
        .and_then(|value| value.as_u64());
    let in_progress_count = alice_member
        .and_then(|member| member.get("inProgressTaskCount"))
        .and_then(|value| value.as_u64());
    let active_count = alice_member
        .and_then(|member| member.get("activeTaskCount"))
        .and_then(|value| value.as_u64());

    harness::print_result(
        label,
        &run_view_resp.to_string(),
        &[
            (
                "run view endpoint returned a view",
                run_view_resp.get("ok").and_then(|value| value.as_bool()) == Some(true)
                    && run_view_resp.get("found").and_then(|value| value.as_bool()) == Some(true),
            ),
            ("Alice member row exists", alice_member.is_some()),
            ("pendingTaskCount == 1", pending_count == Some(1)),
            ("inProgressTaskCount == 1", in_progress_count == Some(1)),
            (
                "activeTaskCount remains aggregate == 2",
                active_count == Some(2),
            ),
        ],
    )
}

/// Caller-path read-model pin for recovery-visible UI state.
///
/// The production overview panel renders one `run-view` payload: member
/// `sessionRuntime.status`, task ownership, and per-member task counts.
/// This scenario pins the recovery-visible Run View invariant:
/// a failed worker can coexist with a released/unowned pending task and a peer
/// that owns reclaimed in-progress work.
pub async fn run_view_shows_failed_member_and_released_task_state(cfg: &Config) -> bool {
    let label = "Agent-Org: run view shows failed member and released task state";
    let org_id = unique_run_id("failed-member-run-view-org");
    let alice_agent = "builtin:explore";
    let bob_agent = "builtin:sde";

    let seed_resp = match post_agent_org_json(
        cfg,
        SEED_ORG_PATH,
        serde_json::json!({
            "id": org_id,
            "name": "Failed Member Read Model Org",
            "coordinator_agent_id": "builtin:sde",
            "members": [
                { "id": "m-alice", "name": "Alice", "role": "worker", "agent_id": alice_agent },
                { "id": "m-bob", "name": "Bob", "role": "worker", "agent_id": bob_agent }
            ]
        }),
    )
    .await
    {
        Err(err) => return harness::print_error(label, &err),
        Ok(json) => json,
    };
    if seed_resp.get("ok").and_then(|value| value.as_bool()) != Some(true) {
        return harness::print_error(label, &seed_resp.to_string());
    }

    let workspace_path = tmp_agent_org_workspace("failed-member-run-view");
    let launch_resp = match post_agent_org_json(
        cfg,
        LAUNCH_COORDINATOR_PATH,
        serde_json::json!({
            "agent_org_id": org_id,
            "workspace_path": workspace_path,
            "content": "",
            "model": cfg.model,
            "account_id": cfg.account_id,
            "name": "Failed Member Read Model E2E",
            "sync_turn": false
        }),
    )
    .await
    {
        Err(err) => return harness::print_error(label, &err),
        Ok(json) => json,
    };
    let session_id = match launch_resp
        .get("session_id")
        .and_then(|value| value.as_str())
    {
        Some(value) if !value.is_empty() => value.to_string(),
        _ => return harness::print_error(label, &launch_resp.to_string()),
    };
    let org_run_id = match launch_resp
        .get("agent_org_run_id")
        .and_then(|value| value.as_str())
    {
        Some(value) if !value.is_empty() => value.to_string(),
        _ => return harness::print_error(label, &launch_resp.to_string()),
    };

    let alice_lookup = match wait_for_worker_session(cfg, &org_run_id, "m-alice").await {
        Err(err) => return harness::print_error(label, &err),
        Ok(json) => json,
    };
    let Some(alice_session_id) = alice_lookup
        .get("session_id")
        .and_then(|value| value.as_str())
    else {
        return harness::print_error(label, &alice_lookup.to_string());
    };

    let status_resp = match set_session_status(cfg, alice_session_id, SESSION_STATUS_FAILED).await {
        Err(err) => return harness::print_error(label, &err),
        Ok(json) => json,
    };

    if let Err(err) = seed_unowned_task(
        cfg,
        &org_run_id,
        "task-released-pending",
        "Released recovery work",
        TASK_STATUS_PENDING,
    )
    .await
    {
        return harness::print_error(label, &err);
    }
    if let Err(err) = seed_task(
        cfg,
        &org_run_id,
        "task-peer-active",
        "Peer reclaimed work",
        "m-bob",
        TASK_STATUS_IN_PROGRESS,
    )
    .await
    {
        return harness::print_error(label, &err);
    }
    if let Err(err) = seed_task(
        cfg,
        &org_run_id,
        "task-failed-completed",
        "Completed audit trail",
        "m-alice",
        TASK_STATUS_COMPLETED,
    )
    .await
    {
        return harness::print_error(label, &err);
    }

    let run_view_resp = match post_agent_org_json(
        cfg,
        RUN_VIEW_PATH,
        serde_json::json!({ "session_id": session_id }),
    )
    .await
    {
        Err(err) => return harness::print_error(label, &err),
        Ok(json) => json,
    };

    let view = run_view_resp.get("view");
    let members = view
        .and_then(|value| value.get("members"))
        .and_then(|value| value.as_array());
    let alice_member = members.and_then(|items| {
        items.iter().find(|member| {
            member.get("memberId").and_then(|value| value.as_str()) == Some("m-alice")
        })
    });
    let bob_member = members.and_then(|items| {
        items
            .iter()
            .find(|member| member.get("memberId").and_then(|value| value.as_str()) == Some("m-bob"))
    });
    let tasks = view
        .and_then(|value| value.get("tasks"))
        .and_then(|value| value.as_array());
    let released_task = tasks.and_then(|items| {
        items.iter().find(|task| {
            task.get("id").and_then(|value| value.as_str()) == Some("task-released-pending")
        })
    });
    let peer_task = tasks.and_then(|items| {
        items.iter().find(|task| {
            task.get("id").and_then(|value| value.as_str()) == Some("task-peer-active")
        })
    });
    let completed_task = tasks.and_then(|items| {
        items.iter().find(|task| {
            task.get("id").and_then(|value| value.as_str()) == Some("task-failed-completed")
        })
    });

    let alice_status_failed = alice_member
        .and_then(|member| member.get("sessionRuntime"))
        .and_then(|runtime| runtime.get("status"))
        .and_then(|value| value.as_str())
        == Some(SESSION_STATUS_FAILED);
    let alice_has_no_open_work = alice_member
        .and_then(|member| member.get("pendingTaskCount"))
        .and_then(|value| value.as_u64())
        == Some(0)
        && alice_member
            .and_then(|member| member.get("inProgressTaskCount"))
            .and_then(|value| value.as_u64())
            == Some(0);
    let bob_has_peer_work = bob_member
        .and_then(|member| member.get("inProgressTaskCount"))
        .and_then(|value| value.as_u64())
        == Some(1);
    let released_task_visible = released_task
        .and_then(|task| task.get("owner"))
        .is_none_or(serde_json::Value::is_null)
        && released_task.and_then(|task| task.get("status").and_then(|value| value.as_str()))
            == Some(TASK_STATUS_PENDING);
    let peer_task_visible = peer_task
        .and_then(|task| task.get("owner").and_then(|value| value.as_str()))
        == Some("m-bob")
        && peer_task.and_then(|task| task.get("status").and_then(|value| value.as_str()))
            == Some(TASK_STATUS_IN_PROGRESS);
    let completed_task_preserved = completed_task
        .and_then(|task| task.get("owner").and_then(|value| value.as_str()))
        == Some("m-alice")
        && completed_task.and_then(|task| task.get("status").and_then(|value| value.as_str()))
            == Some(TASK_STATUS_COMPLETED);

    let output = serde_json::json!({
        "status_update": status_resp,
        "run_view": run_view_resp,
    });
    harness::print_result(
        label,
        &output.to_string(),
        &[
            (
                "run view endpoint returned a view",
                run_view_resp.get("ok").and_then(|value| value.as_bool()) == Some(true)
                    && run_view_resp.get("found").and_then(|value| value.as_bool()) == Some(true),
            ),
            ("Alice member row status is failed", alice_status_failed),
            (
                "Alice has no pending or in-progress task badges",
                alice_has_no_open_work,
            ),
            (
                "Bob has one in-progress reclaimed task badge",
                bob_has_peer_work,
            ),
            (
                "released task is visible as unowned pending",
                released_task_visible,
            ),
            (
                "peer task is visible as Bob-owned in_progress",
                peer_task_visible,
            ),
            (
                "failed member completed task remains owned for audit",
                completed_task_preserved,
            ),
        ],
    )
}

/// Guardrail for the Run View's pure-read contract.
///
/// A failed coordinator and failed workers with unread recovery input may
/// legitimately remain `running`: the watchdog still has a path to wake or
/// escalate them. Opening a UI view must not acquire the writer lock or turn
/// that recoverable state into a terminal run. This scenario reads the raw
/// durable state before and after Run View, then proves the unread row remains
/// independently drainable.
pub async fn control_after_state_run_view_is_pure_read(cfg: &Config) -> bool {
    let label = "Agent-Org: control-after run view is pure read";
    let org_id = unique_run_id("control-after-finality-org");
    let alice_agent = "builtin:explore";
    let bob_agent = "builtin:sde";

    let seed_resp = match post_agent_org_json(
        cfg,
        SEED_ORG_PATH,
        serde_json::json!({
            "id": org_id,
            "name": "Control After Finality Org",
            "coordinator_agent_id": "builtin:sde",
            "members": [
                { "id": "m-alice", "name": "Alice", "role": "worker", "agent_id": alice_agent },
                { "id": "m-bob", "name": "Bob", "role": "worker", "agent_id": bob_agent }
            ]
        }),
    )
    .await
    {
        Err(err) => return harness::print_error(label, &err),
        Ok(json) => json,
    };
    if seed_resp.get("ok").and_then(|value| value.as_bool()) != Some(true) {
        return harness::print_error(label, &seed_resp.to_string());
    }

    let workspace_path = tmp_agent_org_workspace("control-after-finality");
    let launch_resp = match post_agent_org_json(
        cfg,
        LAUNCH_COORDINATOR_PATH,
        serde_json::json!({
            "agent_org_id": org_id,
            "workspace_path": workspace_path,
            "content": "",
            "model": cfg.model,
            "account_id": cfg.account_id,
            "name": "Control After Finality E2E",
            "sync_turn": false
        }),
    )
    .await
    {
        Err(err) => return harness::print_error(label, &err),
        Ok(json) => json,
    };
    let session_id = match launch_resp
        .get("session_id")
        .and_then(|value| value.as_str())
    {
        Some(value) if !value.is_empty() => value.to_string(),
        _ => return harness::print_error(label, &launch_resp.to_string()),
    };
    let org_run_id = match launch_resp
        .get("agent_org_run_id")
        .and_then(|value| value.as_str())
    {
        Some(value) if !value.is_empty() => value.to_string(),
        _ => return harness::print_error(label, &launch_resp.to_string()),
    };

    let alice_lookup = match wait_for_worker_session(cfg, &org_run_id, "m-alice").await {
        Err(err) => return harness::print_error(label, &err),
        Ok(json) => json,
    };
    let bob_lookup = match wait_for_worker_session(cfg, &org_run_id, "m-bob").await {
        Err(err) => return harness::print_error(label, &err),
        Ok(json) => json,
    };
    let Some(alice_session_id) = alice_lookup
        .get("session_id")
        .and_then(|value| value.as_str())
    else {
        return harness::print_error(label, &alice_lookup.to_string());
    };
    let Some(bob_session_id) = bob_lookup
        .get("session_id")
        .and_then(|value| value.as_str())
    else {
        return harness::print_error(label, &bob_lookup.to_string());
    };

    if let Err(err) = seed_task(
        cfg,
        &org_run_id,
        "task-pending-after-control",
        "Pending work after control action",
        "m-alice",
        TASK_STATUS_PENDING,
    )
    .await
    {
        return harness::print_error(label, &err);
    }
    if let Err(err) = seed_task(
        cfg,
        &org_run_id,
        "task-active-after-control",
        "Owned in-progress work after control action",
        "m-bob",
        TASK_STATUS_IN_PROGRESS,
    )
    .await
    {
        return harness::print_error(label, &err);
    }
    let inbox_seed_resp = match post_send(
        cfg,
        serde_json::json!({
            "org_run_id": org_run_id,
            "org_id": org_id,
            "org_name": "Control After Finality Org",
            "org_role": "team",
            "coordinator_agent_id": "builtin:sde",
            "coordinator_name": "Control After Finality Org",
            "coordinator_role": "lead",
            "sender_agent_id": "builtin:sde",
            "sender_member_id": "coordinator",
            "members": [
                { "member_id": "m-alice", "name": "Alice", "role": "worker", "agent_id": alice_agent },
                { "member_id": "m-bob", "name": "Bob", "role": "worker", "agent_id": bob_agent }
            ],
            "params": {
                "recipient_member_id": "m-alice",
                "kind": "plain",
                "summary": "resume after control",
                "text": "Please resume after rewind/truncate control state.",
                "related_task_id": "task-pending-after-control"
            }
        }),
    )
    .await
    {
        Err(err) => return harness::print_error(label, &err),
        Ok(json) => json,
    };
    if inbox_seed_resp.get("ok").and_then(|value| value.as_bool()) != Some(true) {
        return harness::print_error(label, &inbox_seed_resp.to_string());
    }

    if let Err(err) = set_session_status(cfg, &session_id, SESSION_STATUS_FAILED).await {
        return harness::print_error(label, &err);
    }
    if let Err(err) = set_session_status(cfg, alice_session_id, SESSION_STATUS_FAILED).await {
        return harness::print_error(label, &err);
    }
    if let Err(err) = set_session_status(cfg, bob_session_id, SESSION_STATUS_FAILED).await {
        return harness::print_error(label, &err);
    }

    let before_raw = match durable_invariants(cfg, &org_run_id).await {
        Err(err) => return harness::print_error(label, &err),
        Ok(json) => json,
    };
    let before_is_recoverable_control_state = before_raw
        .get("runningOpenWorkWithoutLiveWorker")
        .and_then(|value| value.as_bool())
        == Some(true)
        && before_raw
            .get("ownerlessInProgressCount")
            .and_then(|value| value.as_u64())
            == Some(0)
        && before_raw
            .get("unreadInboxCount")
            .and_then(|value| value.as_u64())
            .unwrap_or(0)
            > 0;

    let run_view_resp = match post_agent_org_json(
        cfg,
        RUN_VIEW_PATH,
        serde_json::json!({ "session_id": session_id }),
    )
    .await
    {
        Err(err) => return harness::print_error(label, &err),
        Ok(json) => json,
    };
    let after_raw = match durable_invariants(cfg, &org_run_id).await {
        Err(err) => return harness::print_error(label, &err),
        Ok(json) => json,
    };
    let run_view_left_durable_state_unchanged = after_raw.get("runStatus")
        == before_raw.get("runStatus")
        && after_raw.get("openTaskCount") == before_raw.get("openTaskCount")
        && after_raw.get("unreadInboxCount") == before_raw.get("unreadInboxCount")
        && after_raw.get("liveWorkerCount") == before_raw.get("liveWorkerCount");

    let drain_resp = match drain_inbox_with_body(
        cfg,
        serde_json::json!({
            "org_run_id": org_run_id,
            "org_id": org_id,
            "org_name": "Control After Finality Org",
            "org_role": "team",
            "coordinator_agent_id": "builtin:sde",
            "coordinator_name": "Control After Finality Org",
            "coordinator_role": "lead",
            "members": [
                { "member_id": "m-alice", "name": "Alice", "role": "worker", "agent_id": alice_agent },
                { "member_id": "m-bob", "name": "Bob", "role": "worker", "agent_id": bob_agent }
            ],
            "recipient_agent_id": alice_agent,
            "recipient_member_id": "m-alice"
        }),
    )
    .await
    {
        Err(err) => return harness::print_error(label, &err),
        Ok(json) => json,
    };
    let final_raw = match durable_invariants(cfg, &org_run_id).await {
        Err(err) => return harness::print_error(label, &err),
        Ok(json) => json,
    };
    let inbox_drainable = drain_resp.get("ok").and_then(|value| value.as_bool()) == Some(true)
        && final_raw
            .get("unreadInboxCount")
            .and_then(|value| value.as_u64())
            == Some(0);

    let output = serde_json::json!({
        "beforeRaw": before_raw,
        "runView": run_view_resp,
        "afterRaw": after_raw,
        "drain": drain_resp,
        "finalRaw": final_raw,
    });
    harness::print_result(
        label,
        &output.to_string(),
        &[
            (
                "raw post-control DB state is recoverable running + no live worker + open work",
                before_is_recoverable_control_state,
            ),
            (
                "run view endpoint returned a view",
                run_view_resp.get("ok").and_then(|value| value.as_bool()) == Some(true)
                    && run_view_resp.get("found").and_then(|value| value.as_bool()) == Some(true),
            ),
            (
                "opening run view leaves durable state unchanged",
                run_view_left_durable_state_unchanged,
            ),
            (
                "pending/in_progress tasks remain legal after control state",
                final_raw
                    .get("ownerlessInProgressCount")
                    .and_then(|value| value.as_u64())
                    == Some(0),
            ),
            ("unread inbox row remains drainable", inbox_drainable),
            (
                "draining recovery input does not falsely abandon the run",
                final_raw.get("runStatus").and_then(|value| value.as_str()) == Some("running"),
            ),
        ],
    )
}

/// Coordinator sends a `plain` message addressed by `recipient_name`.
/// Asserts the inbox row landed under the named worker's `agent_id`,
/// `payload_kind = "plain"`, and the decoded payload preserves text/summary.
pub async fn send_plain_by_name(cfg: &Config) -> bool {
    let run_id = match seed_default_running_run(cfg, "send-by-name", None).await {
        Ok(run_id) => run_id,
        Err(err) => return harness::print_error("Agent-Org: send by name", &err),
    };
    let related_task_id = format!("send-by-name-task-{run_id}");
    if let Err(err) = seed_task(
        cfg,
        &run_id,
        &related_task_id,
        "Alice message fixture",
        "m1",
        TASK_STATUS_PENDING,
    )
    .await
    {
        return harness::print_error("Agent-Org: send by name", &err);
    }
    let body = build_send_body(
        &run_id,
        "coord",
        serde_json::json!({
            "recipient_name": "Alice",
            "kind": "plain",
            "summary": "ping",
            "text": "please look up X",
            "related_task_id": related_task_id,
        }),
    );

    let send_resp = match post_send(cfg, body).await {
        Err(err) => return harness::print_error("Agent-Org: send by name", &err),
        Ok(json) => json,
    };
    let send_ok = send_resp
        .get("ok")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let list_resp = match list_inbox(cfg, &run_id).await {
        Err(err) => return harness::print_error("Agent-Org: send by name", &err),
        Ok(json) => json,
    };
    let messages = match messages_array(&list_resp) {
        Err(err) => return harness::print_error("Agent-Org: send by name", &err),
        Ok(arr) => arr,
    };

    let one_message = messages.len() == 1;
    let row = messages.first();
    let recipient_ok = row.and_then(|r| r.get("recipient_agent_id").and_then(|v| v.as_str()))
        == Some("alice-agent");
    let sender_ok =
        row.and_then(|r| r.get("sender_agent_id").and_then(|v| v.as_str())) == Some("coord");
    let kind_ok = row.and_then(|r| r.get("payload_kind").and_then(|v| v.as_str())) == Some("plain");
    let text_ok = row.and_then(|r| {
        r.get("payload_decoded")
            .and_then(|p| p.get("text"))
            .and_then(|v| v.as_str())
    }) == Some("please look up X");

    harness::print_result(
        "Agent-Org: send by name",
        &send_resp.to_string(),
        &[
            ("send ok=true", send_ok),
            ("inbox has exactly 1 row", one_message),
            ("recipient_agent_id == alice-agent", recipient_ok),
            ("sender_agent_id == coord", sender_ok),
            ("payload_kind == plain", kind_ok),
            ("decoded text round-trips", text_ok),
        ],
    )
}

/// Coordinator sends by `recipient_agent_id` (bypasses name lookup).
/// Pins the agent-id resolution path independently from the name path.
pub async fn send_plain_by_agent_id(cfg: &Config) -> bool {
    let run_id = match seed_default_running_run(cfg, "send-by-id", None).await {
        Ok(run_id) => run_id,
        Err(err) => return harness::print_error("Agent-Org: send by agent_id", &err),
    };
    let related_task_id = format!("send-by-id-task-{run_id}");
    if let Err(err) = seed_task(
        cfg,
        &run_id,
        &related_task_id,
        "Bob message fixture",
        "m2",
        TASK_STATUS_PENDING,
    )
    .await
    {
        return harness::print_error("Agent-Org: send by agent_id", &err);
    }
    let body = build_send_body(
        &run_id,
        "coord",
        serde_json::json!({
            "recipient_agent_id": "bob-agent",
            "kind": "plain",
            "summary": "hello bob",
            "text": "status update?",
            "related_task_id": related_task_id,
        }),
    );

    let send_resp = match post_send(cfg, body).await {
        Err(err) => return harness::print_error("Agent-Org: send by agent_id", &err),
        Ok(json) => json,
    };
    let send_ok = send_resp
        .get("ok")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let list_resp = match list_inbox(cfg, &run_id).await {
        Err(err) => return harness::print_error("Agent-Org: send by agent_id", &err),
        Ok(json) => json,
    };
    let messages = match messages_array(&list_resp) {
        Err(err) => return harness::print_error("Agent-Org: send by agent_id", &err),
        Ok(arr) => arr,
    };

    let exactly_one = messages.len() == 1;
    let landed_on_bob = messages
        .first()
        .and_then(|r| r.get("recipient_agent_id").and_then(|v| v.as_str()))
        == Some("bob-agent");

    harness::print_result(
        "Agent-Org: send by agent_id",
        &send_resp.to_string(),
        &[
            ("send ok=true", send_ok),
            ("inbox has exactly 1 row", exactly_one),
            ("recipient_agent_id == bob-agent", landed_on_bob),
        ],
    )
}

/// Worker → coordinator round-trip: a worker sends `plain` to the
/// coordinator's display name. Pins the worker-as-sender code path
/// (`addressable_agents` includes the coordinator).
pub async fn worker_addresses_coordinator(cfg: &Config) -> bool {
    let run_id = match seed_default_running_run(cfg, "worker-to-coord", None).await {
        Ok(run_id) => run_id,
        Err(err) => return harness::print_error("Agent-Org: worker → coord", &err),
    };
    let body = build_send_body(
        &run_id,
        "alice-agent",
        serde_json::json!({
            "recipient_name": "Searcher Org",
            "kind": "plain",
            "summary": "report",
            "text": "task done",
        }),
    );

    let send_resp = match post_send(cfg, body).await {
        Err(err) => return harness::print_error("Agent-Org: worker → coord", &err),
        Ok(json) => json,
    };
    let send_ok = send_resp
        .get("ok")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let list_resp = match list_inbox(cfg, &run_id).await {
        Err(err) => return harness::print_error("Agent-Org: worker → coord", &err),
        Ok(json) => json,
    };
    let messages = match messages_array(&list_resp) {
        Err(err) => return harness::print_error("Agent-Org: worker → coord", &err),
        Ok(arr) => arr,
    };

    let one_row = messages.len() == 1;
    let recipient_is_coord = messages
        .first()
        .and_then(|r| r.get("recipient_agent_id").and_then(|v| v.as_str()))
        == Some("coord");
    let sender_is_alice = messages
        .first()
        .and_then(|r| r.get("sender_agent_id").and_then(|v| v.as_str()))
        == Some("alice-agent");

    harness::print_result(
        "Agent-Org: worker → coord",
        &send_resp.to_string(),
        &[
            ("send ok=true", send_ok),
            ("inbox has exactly 1 row", one_row),
            ("recipient_agent_id == coord", recipient_is_coord),
            ("sender_agent_id == alice-agent", sender_is_alice),
        ],
    )
}

/// Each typed RPC variant must persist with a `payload_kind` that matches
/// its serde tag — pins the `kind_tag_matches_serde_tag` invariant from
/// the unit tests at the wire boundary.
pub async fn typed_kinds_round_trip(cfg: &Config) -> bool {
    let run_id = match seed_default_running_run(cfg, "typed-kinds", Some("plan-req-1")).await {
        Ok(run_id) => run_id,
        Err(err) => return harness::print_error("Agent-Org: typed kinds round-trip", &err),
    };

    // There are 5 typed kinds with in-process listeners:
    // `plain`, `shutdown_request`, `shutdown_response`,
    // `plan_approval_request` (member → coordinator inbox, written by
    // `create_plan` when a member submits a plan) and
    // `plan_approval_response` (coordinator → member inbox, drained by
    // the member's turn boundary which then exits plan mode on accept).
    // Permission / mode-switch flows are intentionally absent because
    // the real systems talk to the user, not sibling agents.
    // Each tuple is (label, sender, params). Sender is explicit because
    // the second member of each request/response RPC pair is the
    // recipient of the request (i.e. roles swap mid-pair). Using
    // explicit senders also lets the plan-approval response carry a
    // distinct `request_id` from the shutdown pair without relying on
    // ordering implications.
    let cases: Vec<(&str, &str, serde_json::Value)> = vec![
        (
            "shutdown_request",
            "coord",
            serde_json::json!({
                "recipient_agent_id": "alice-agent",
                "kind": "shutdown_request",
                "request_id": "req-1",
                "reason": "wrap up",
            }),
        ),
        (
            "shutdown_response",
            "alice-agent",
            serde_json::json!({
                "recipient_agent_id": "coord",
                "kind": "shutdown_response",
                "request_id": "req-1",
                "accepted": true,
                "note": "winding down cleanly",
            }),
        ),
        (
            "plan_approval_response",
            "coord",
            serde_json::json!({
                "recipient_agent_id": "alice-agent",
                "kind": "plan_approval_response",
                "request_id": "plan-req-1",
                "accepted": false,
                "feedback": "please revise the plan",
            }),
        ),
    ];

    for (label, sender, params) in &cases {
        let body = build_send_body(&run_id, sender, params.clone());
        let send_resp = match post_send(cfg, body).await {
            Err(err) => {
                return harness::print_error(&format!("Agent-Org typed kinds ({label})"), &err)
            }
            Ok(json) => json,
        };
        if !send_resp
            .get("ok")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            return harness::print_error(
                &format!("Agent-Org typed kinds ({label})"),
                &send_resp.to_string(),
            );
        }
    }

    let list_resp = match list_inbox(cfg, &run_id).await {
        Err(err) => return harness::print_error("Agent-Org: typed kinds round-trip", &err),
        Ok(json) => json,
    };
    let messages = match messages_array(&list_resp) {
        Err(err) => return harness::print_error("Agent-Org: typed kinds round-trip", &err),
        Ok(arr) => arr,
    };

    // shutdown_request (1) + shutdown_response (1) + plan_approval_response (1) = 3 rows.
    let row_count_ok = messages.len() == 3;

    let mut payload_kinds: Vec<&str> = messages
        .iter()
        .filter_map(|r| r.get("payload_kind").and_then(|v| v.as_str()))
        .collect();
    payload_kinds.sort();
    let mut expected = vec![
        "shutdown_request",
        "shutdown_response",
        "plan_approval_response",
    ];
    expected.sort();
    let kinds_ok = payload_kinds == expected;

    let request_ids: Vec<&str> = messages
        .iter()
        .filter_map(|r| r.get("request_id").and_then(|v| v.as_str()))
        .collect();
    // Both shutdown rows carry the same correlation id (the RPC pair).
    let shutdown_req_id_round_trip = request_ids.iter().filter(|r| **r == "req-1").count() == 2;
    // The plan-approval response carries its own correlation id from
    // the originating `create_plan` call.
    let plan_req_id_present = request_ids.contains(&"plan-req-1");

    harness::print_result(
        "Agent-Org: typed kinds round-trip",
        &list_resp.to_string(),
        &[
            (
                "inbox has 3 rows (shutdown pair + plan_approval_response)",
                row_count_ok,
            ),
            ("payload_kind set matches serde tags", kinds_ok),
            (
                "request_id correlates shutdown request and response",
                shutdown_req_id_round_trip,
            ),
            (
                "plan_approval_response request_id round-trips",
                plan_req_id_present,
            ),
        ],
    )
}

// ── Error cases ──────────────────────────────────────────────────────

async fn assert_invalid_params(
    cfg: &Config,
    label: &str,
    params: serde_json::Value,
    needle: &str,
) -> bool {
    assert_invalid_params_from(cfg, label, "coord", params, needle).await
}

/// Same as `assert_invalid_params` but with an explicit sender. Required
/// for scenarios that pin sender-dependent invariants — e.g.
/// "shutdown_response must be addressed to the coordinator", which has
/// to be exercised from a worker sender to be meaningful (a coordinator
/// sending shutdown_response to a peer would already be rejected by the
/// retired-kind check on the response originator side once the
/// recipient guard fires).
async fn assert_invalid_params_from(
    cfg: &Config,
    label: &str,
    sender: &str,
    params: serde_json::Value,
    needle: &str,
) -> bool {
    let run_id = unique_run_id(label);
    let body = build_send_body(&run_id, sender, params);
    let resp = match post_send(cfg, body).await {
        Err(err) => return harness::print_error(label, &err),
        Ok(json) => json,
    };
    let ok = resp.get("ok").and_then(|v| v.as_bool()).unwrap_or(true);
    let kind = resp
        .get("error_kind")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let message = resp
        .get("error_message")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let needle_match = message.to_lowercase().contains(&needle.to_lowercase());

    // Negative assertion: nothing should land in the inbox on InvalidParams.
    let list_resp = match list_inbox(cfg, &run_id).await {
        Err(err) => return harness::print_error(label, &err),
        Ok(json) => json,
    };
    let messages = match messages_array(&list_resp) {
        Err(err) => return harness::print_error(label, &err),
        Ok(arr) => arr,
    };
    let nothing_persisted = messages.is_empty();

    harness::print_result(
        label,
        &resp.to_string(),
        &[
            ("ok=false", !ok),
            ("error_kind == invalid_params", kind == "invalid_params"),
            (&format!("error_message contains '{needle}'"), needle_match),
            ("inbox is empty (nothing persisted)", nothing_persisted),
        ],
    )
}

pub async fn rejects_zero_recipients(cfg: &Config) -> bool {
    assert_invalid_params(
        cfg,
        "Agent-Org: rejects zero recipients",
        serde_json::json!({
            "kind": "plain",
            "summary": "x",
            "text": "y",
        }),
        "recipient_member_id is required",
    )
    .await
}

pub async fn rejects_unknown_recipient_name(cfg: &Config) -> bool {
    assert_invalid_params(
        cfg,
        "Agent-Org: rejects unknown recipient name",
        serde_json::json!({
            "recipient_name": "Charlie",
            "kind": "plain",
            "summary": "x",
            "text": "y",
        }),
        "not addressable",
    )
    .await
}

pub async fn rejects_self_routing_by_id(cfg: &Config) -> bool {
    assert_invalid_params(
        cfg,
        "Agent-Org: rejects self-routing by agent_id",
        serde_json::json!({
            "recipient_agent_id": "coord",
            "kind": "plain",
            "summary": "x",
            "text": "y",
        }),
        "not addressable",
    )
    .await
}

pub async fn rejects_unknown_kind(cfg: &Config) -> bool {
    assert_invalid_params(
        cfg,
        "Agent-Org: rejects unknown kind",
        serde_json::json!({
            "recipient_name": "Alice",
            "kind": "telepathy",
            "summary": "x",
            "text": "y",
        }),
        "not allowed",
    )
    .await
}

pub async fn plain_requires_summary_and_text(cfg: &Config) -> bool {
    assert_invalid_params(
        cfg,
        "Agent-Org: plain requires summary and text",
        serde_json::json!({
            "recipient_name": "Alice",
            "kind": "plain",
            "text": "no summary",
        }),
        "summary",
    )
    .await
}

pub async fn shutdown_request_requires_request_id(cfg: &Config) -> bool {
    assert_invalid_params(
        cfg,
        "Agent-Org: shutdown_request requires request_id",
        serde_json::json!({
            "recipient_agent_id": "alice-agent",
            "kind": "shutdown_request",
            "reason": "noop",
        }),
        "request_id",
    )
    .await
}

/// The retired kinds (`permission_request`, `permission_response`,
/// `mode_set_request`, `team_permission_update`, `plan_approval_response`)
/// must surface a stable error string so the LLM can self-correct.
/// Permission and mode-switch flows live in `interaction::permission`
/// and `interaction::mode_switch` and talk to the user, not sibling
/// agents — these were never agent-to-agent in the first place.
/// `plan_approval_request` is reserved for the `create_plan` tool — it
/// is intentionally not LLM-callable so a member session cannot forge
/// a plan request from another member. The send-message-direct helper
/// must reject it with the dedicated error message rather than treating
/// it like any other valid kind.
pub async fn rejects_plan_approval_request_via_send_message(cfg: &Config) -> bool {
    // Sender is the coordinator and recipient is a worker; that combination
    // bypasses the self-routing guard so the LLM-callable check is the one
    // that actually fires. (The reverse — coordinator addressing itself —
    // would short-circuit on self-routing and silently mask a regression
    // of the LLM-callable check, which is exactly this scenario's job.)
    assert_invalid_params(
        cfg,
        "Agent-Org: plan_approval_request is not LLM-callable",
        serde_json::json!({
            "recipient_agent_id": "alice-agent",
            "kind": "plan_approval_request",
            "request_id": "plan-req-forge",
        }),
        "not allowed",
    )
    .await
}

pub async fn rejects_retired_kinds(cfg: &Config) -> bool {
    // Retired kinds (no in-process listener). `plan_approval_request`
    // and `plan_approval_response` are NOT in this list because the
    // inbox-drain side effect consumes them.
    let retired = [
        "permission_request",
        "permission_response",
        "mode_set_request",
        "team_permission_update",
    ];
    for kind in retired {
        let label = format!("Agent-Org: retired kind '{kind}' is rejected");
        let ok = assert_invalid_params(
            cfg,
            &label,
            serde_json::json!({
                "recipient_agent_id": "alice-agent",
                "kind": kind,
                "request_id": "req-retired",
            }),
            "not allowed",
        )
        .await;
        if !ok {
            return false;
        }
    }
    true
}

/// `shutdown_response` is the member's reply to a coordinator-originated
/// `shutdown_request`, so its only legitimate recipient is the
/// coordinator. Allowing a member→member `shutdown_response` would let
/// workers spoof a peer-to-peer "ack" exchange the coordinator never
/// authorised. Pin the recipient guard at the wire boundary so a
/// future refactor that re-broadens recipient resolution surfaces here.
///
/// Note: the happy path (alice-agent → coord shutdown_response succeeds
/// and lands in inbox) is already covered by `typed_kinds_round_trip`'s
/// second case, so this scenario only pins the negative.
pub async fn rejects_shutdown_response_to_peer_member(cfg: &Config) -> bool {
    assert_invalid_params_from(
        cfg,
        "Agent-Org: shutdown_response to peer member is rejected",
        "alice-agent",
        serde_json::json!({
            "recipient_agent_id": "bob-agent",
            "kind": "shutdown_response",
            "request_id": "req-peer-shutdown",
            "accepted": true,
        }),
        "must be sent to recipient_member_id 'coordinator'",
    )
    .await
}

/// A member rejecting the coordinator's `shutdown_request`
/// (`accepted=false`) without explaining why is useless to the
/// coordinator and would silently strand the org. Build-time check
/// requires a non-empty `note`. Pinning two variants (missing key +
/// whitespace-only) so the LLM cannot trivially defeat the rule by
/// sending `"   "`.
pub async fn rejects_shutdown_response_rejection_without_note(cfg: &Config) -> bool {
    let no_note = assert_invalid_params_from(
        cfg,
        "Agent-Org: shutdown_response rejection without note is rejected",
        "alice-agent",
        serde_json::json!({
            "recipient_agent_id": "coord",
            "kind": "shutdown_response",
            "request_id": "req-reject-no-note",
            "accepted": false,
        }),
        "requires a non-empty 'note'",
    )
    .await;
    if !no_note {
        return false;
    }

    assert_invalid_params_from(
        cfg,
        "Agent-Org: shutdown_response rejection with whitespace-only note is rejected",
        "alice-agent",
        serde_json::json!({
            "recipient_agent_id": "coord",
            "kind": "shutdown_response",
            "request_id": "req-reject-blank-note",
            "accepted": false,
            "note": "   ",
        }),
        "requires a non-empty 'note'",
    )
    .await
}

/// Caller-path E2E. Pins the **full coordinator-side
/// shutdown-handshake side effect** end-to-end:
///
/// 1. Alice (a worker) sends `shutdown_response{accepted=true}` to the
///    coordinator. The row lands in the coordinator's inbox.
/// 2. The coordinator drains its inbox (`drain-inbox` debug endpoint).
///    The drain side-effect path observes the accepted-shutdown row,
///    fires the production `MemberShutdownHook` (no-op against a
///    non-existent worker session in this synthetic context — the
///    shutdown of a real session is exercised by the LLM scenarios),
///    and inserts a `MemberTerminated` row authored by the system
///    sender into the coordinator's own inbox.
/// 3. A subsequent list-by-run sees the original `shutdown_response`
///    flipped to read AND the new `MemberTerminated` row pending
///    (unread) for the coordinator's next turn.
pub async fn accepted_shutdown_response_yields_member_terminated_row(cfg: &Config) -> bool {
    let label = "Agent-Org: accepted shutdown_response → coord inbox MemberTerminated row";
    let run_id = match seed_default_running_run(cfg, "shutdown-handshake-accepted", None).await {
        Ok(run_id) => run_id,
        Err(err) => return harness::print_error(label, &err),
    };

    // Step 1 — Alice acks the shutdown.
    let body = build_send_body(
        &run_id,
        "alice-agent",
        serde_json::json!({
            "recipient_agent_id": "coord",
            "kind": "shutdown_response",
            "request_id": "req-shut-c1",
            "accepted": true,
            "note": "winding down cleanly",
        }),
    );
    let send_resp = match post_send(cfg, body).await {
        Err(err) => return harness::print_error(label, &err),
        Ok(json) => json,
    };
    let send_ok = send_resp
        .get("ok")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if !send_ok {
        return harness::print_error(label, &send_resp.to_string());
    }

    // Step 2 — drain the coordinator's inbox.
    let drain_resp = match drain_inbox(cfg, &run_id, "coord").await {
        Err(err) => return harness::print_error(label, &err),
        Ok(json) => json,
    };
    let drain_ok = drain_resp
        .get("ok")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let drained_count = drain_resp
        .get("drained_count")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    if !drain_ok || drained_count != 1 {
        return harness::print_error(label, &drain_resp.to_string());
    }

    // Step 3 — list-by-run; expect original shutdown_response (read=true)
    // and a fresh `member_terminated` row from `_system` (read=false).
    let list_resp = match list_inbox(cfg, &run_id).await {
        Err(err) => return harness::print_error(label, &err),
        Ok(json) => json,
    };
    let messages = match messages_array(&list_resp) {
        Err(err) => return harness::print_error(label, &err),
        Ok(arr) => arr,
    };

    let two_rows = messages.len() == 2;

    let shutdown_row = messages
        .iter()
        .find(|r| r.get("payload_kind").and_then(|v| v.as_str()) == Some("shutdown_response"));
    let terminated_row = messages
        .iter()
        .find(|r| r.get("payload_kind").and_then(|v| v.as_str()) == Some("member_terminated"));

    let shutdown_present = shutdown_row.is_some();
    let terminated_present = terminated_row.is_some();

    let terminated_from_system = terminated_row
        .and_then(|r| r.get("sender_agent_id").and_then(|v| v.as_str()))
        .map(|s| s == "_system")
        .unwrap_or(false);

    let terminated_recipient_is_coord = terminated_row
        .and_then(|r| r.get("recipient_agent_id").and_then(|v| v.as_str()))
        .map(|s| s == "coord")
        .unwrap_or(false);

    // The MemberTerminated payload must reference Alice — agent_id
    // resolved from the org member roster, agent_name preserved, and
    // reason = shutdown. `payload_decoded` is the flat
    // internally-tagged shape ({kind, agent_id, agent_name, reason}),
    // matching the wire format the coordinator's renderer reads.
    let terminated_payload_ok = terminated_row
        .and_then(|r| r.get("payload_decoded"))
        .map(|inner| {
            let member_id_match = inner
                .get("member_id")
                .and_then(|v| v.as_str())
                .map(|s| s == "m1")
                .unwrap_or(false);
            let member_name_match = inner
                .get("member_name")
                .and_then(|v| v.as_str())
                .map(|s| s == "Alice")
                .unwrap_or(false);
            let reason_match = inner
                .get("reason")
                .and_then(|v| v.as_str())
                .map(|s| s == "shutdown")
                .unwrap_or(false);
            member_id_match && member_name_match && reason_match
        })
        .unwrap_or(false);

    let shutdown_marked_read = shutdown_row
        .and_then(|r| r.get("read_at"))
        .map(|v| !v.is_null())
        .unwrap_or(false);

    let terminated_unread = terminated_row
        .and_then(|r| r.get("read_at"))
        .map(|v| v.is_null())
        .unwrap_or(false);

    harness::print_result(
        label,
        &list_resp.to_string(),
        &[
            ("inbox has 2 rows after drain", two_rows),
            ("shutdown_response row present", shutdown_present),
            ("member_terminated row present", terminated_present),
            (
                "member_terminated.sender_agent_id == _system",
                terminated_from_system,
            ),
            (
                "member_terminated.recipient_agent_id == coord",
                terminated_recipient_is_coord,
            ),
            (
                "member_terminated payload references Alice/shutdown reason",
                terminated_payload_ok,
            ),
            (
                "shutdown_response marked read by drain commit",
                shutdown_marked_read,
            ),
            (
                "member_terminated still unread (next-turn delivery)",
                terminated_unread,
            ),
        ],
    )
}

/// Negative pin. A rejected `shutdown_response` (`accepted=false`) is
/// the worker pushing back; the coordinator's drain must NOT enqueue
/// a `MemberTerminated` row, even though the `shutdown_response`
/// itself drains and is rendered. Without this pin a future refactor
/// that conditions the side effect only on the kind (rather than
/// `accepted=true`) would silently terminate in-flight workers on
/// every rejection.
pub async fn rejected_shutdown_response_does_not_yield_member_terminated(cfg: &Config) -> bool {
    let label = "Agent-Org: rejected shutdown_response leaves coord inbox without MemberTerminated";
    let run_id = match seed_default_running_run(cfg, "shutdown-handshake-rejected", None).await {
        Ok(run_id) => run_id,
        Err(err) => return harness::print_error(label, &err),
    };

    let body = build_send_body(
        &run_id,
        "alice-agent",
        serde_json::json!({
            "recipient_agent_id": "coord",
            "kind": "shutdown_response",
            "request_id": "req-shut-c1-neg",
            "accepted": false,
            "note": "still mid-flight, give me a few more turns",
        }),
    );
    let send_resp = match post_send(cfg, body).await {
        Err(err) => return harness::print_error(label, &err),
        Ok(json) => json,
    };
    if !send_resp
        .get("ok")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        return harness::print_error(label, &send_resp.to_string());
    }

    let drain_resp = match drain_inbox(cfg, &run_id, "coord").await {
        Err(err) => return harness::print_error(label, &err),
        Ok(json) => json,
    };
    let drained_count = drain_resp
        .get("drained_count")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    let list_resp = match list_inbox(cfg, &run_id).await {
        Err(err) => return harness::print_error(label, &err),
        Ok(json) => json,
    };
    let messages = match messages_array(&list_resp) {
        Err(err) => return harness::print_error(label, &err),
        Ok(arr) => arr,
    };

    let one_row = messages.len() == 1;
    let only_shutdown = messages
        .first()
        .and_then(|r| r.get("payload_kind").and_then(|v| v.as_str()))
        == Some("shutdown_response");
    let no_member_terminated = !messages
        .iter()
        .any(|r| r.get("payload_kind").and_then(|v| v.as_str()) == Some("member_terminated"));

    harness::print_result(
        label,
        &list_resp.to_string(),
        &[
            ("drained_count == 1", drained_count == 1),
            ("inbox still has just one row", one_row),
            ("the surviving row is the shutdown_response", only_shutdown),
            (
                "no member_terminated row was enqueued",
                no_member_terminated,
            ),
        ],
    )
}

// ── Org-member spawn gate ──────────────────────────────────────────
//
// Caller-path probes for the `org_member_spawn_rejection` chokepoint
// in `AgentTool::execute`. The unit tests cover the helper's full
// truth table; these scenarios prove the helper is wired to the
// public agent-module surface. The endpoint imports the same
// `org_member_spawn_rejection` symbol the production `execute_text`
// body imports, so a regression that deletes the call site or swaps
// in a stub would break both.

async fn post_check_member_spawn_gate(
    cfg: &Config,
    body: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let url = format!("{}{}", cfg.base_url, CHECK_MEMBER_SPAWN_GATE_PATH);
    let resp = http_client()
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|err| format!("HTTP error: {err}"))?;
    resp.json::<serde_json::Value>()
        .await
        .map_err(|err| format!("JSON parse error: {err}"))
}

fn member_spawn_org_context() -> serde_json::Value {
    serde_json::json!({
        "run_id": "run-spawn-gate",
        "org_id": "org-spawn-gate",
        "org_name": "Spawn Gate Org",
        "org_role": "team",
        "coordinator_agent_id": "alice",
        "coordinator_name": "Alice",
        "coordinator_role": "lead",
        "members": [
            { "member_id": "m-bob", "name": "Bob", "role": "worker", "agent_id": "bob" },
            { "member_id": "m-carol", "name": "Carol", "role": "worker", "agent_id": "carol" },
        ],
    })
}

/// Member tries to spawn another org member as a sub-agent.
/// `isTeammate() && teamName && name` throws "Teammates cannot spawn
/// other teammates". We assert: `rejected = true`,
/// `error_kind = execution_failed`, message references the offending
/// target and the alternative (`org_send_message`).
pub async fn member_cannot_spawn_peer_member(cfg: &Config) -> bool {
    let label = "Agent-Org: member cannot spawn peer member";
    let body = serde_json::json!({
        "is_shadow": false,
        "is_org_member": true,
        "org_context": member_spawn_org_context(),
        "target_agent_id": "carol",
        "is_background": false,
    });
    let resp = match post_check_member_spawn_gate(cfg, body).await {
        Err(err) => return harness::print_error(label, &err),
        Ok(json) => json,
    };

    let ok = resp.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
    let rejected = resp
        .get("rejected")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let error_kind = resp
        .get("error_kind")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let error_message = resp
        .get("error_message")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let mentions_target = error_message.contains("carol");
    let mentions_alternative = error_message.contains("org_send_message");

    harness::print_result(
        label,
        &resp.to_string(),
        &[
            ("endpoint returned ok", ok),
            ("helper returned rejected=true", rejected),
            (
                "error_kind == execution_failed",
                error_kind == "execution_failed",
            ),
            ("error message names the offending target", mentions_target),
            (
                "error message points at org_send_message as the correct alternative",
                mentions_alternative,
            ),
        ],
    )
}

/// Member tries to spawn an ordinary sub-agent with `background = true`.
/// `isInProcessTeammate() && teamName && run_in_background === true`
/// throws. We assert the helper rejects with a clear lifecycle reason
/// ("background"). The target itself is a non-org agent so the
/// peer-spawn branch is bypassed.
pub async fn member_cannot_spawn_background(cfg: &Config) -> bool {
    let label = "Agent-Org: member cannot spawn background sub-agent";
    let body = serde_json::json!({
        "is_shadow": false,
        "is_org_member": true,
        "org_context": member_spawn_org_context(),
        "target_agent_id": "builtin:explore",
        "is_background": true,
    });
    let resp = match post_check_member_spawn_gate(cfg, body).await {
        Err(err) => return harness::print_error(label, &err),
        Ok(json) => json,
    };

    let rejected = resp
        .get("rejected")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let error_kind = resp
        .get("error_kind")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let error_message = resp
        .get("error_message")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    harness::print_result(
        label,
        &resp.to_string(),
        &[
            ("helper returned rejected=true", rejected),
            (
                "error_kind == execution_failed",
                error_kind == "execution_failed",
            ),
            (
                "error message mentions the background-spawn rule",
                error_message.contains("background"),
            ),
            (
                "error message names the offending target id",
                error_message.contains("builtin:explore"),
            ),
        ],
    )
}

/// Member spawns an ordinary sub-agent synchronously (the supported
/// pattern). Members keep normal `agent` tool delegation rights;
/// only peer-spawn and background-spawn are blocked. We assert
/// `rejected = false` so the helper does not over-block.
pub async fn member_can_spawn_ordinary_subagent(cfg: &Config) -> bool {
    let label = "Agent-Org: member can spawn ordinary sub-agent synchronously";
    let body = serde_json::json!({
        "is_shadow": false,
        "is_org_member": true,
        "org_context": member_spawn_org_context(),
        "target_agent_id": "builtin:explore",
        "is_background": false,
    });
    let resp = match post_check_member_spawn_gate(cfg, body).await {
        Err(err) => return harness::print_error(label, &err),
        Ok(json) => json,
    };

    let ok = resp.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
    let rejected = resp
        .get("rejected")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    harness::print_result(
        label,
        &resp.to_string(),
        &[
            ("endpoint returned ok", ok),
            (
                "helper returned rejected=false (ordinary sync sub-agent must be allowed)",
                !rejected,
            ),
        ],
    )
}

/// Coordinator cannot spawn a roster member with the `agent` tool.
/// Roster member sessions are created at Agent Org launch time, so
/// coordinator work dispatch must use direct messages or the shared task
/// queue instead of creating a second member session through sub-agent
/// delegation.
pub async fn coordinator_cannot_spawn_materialized_member(cfg: &Config) -> bool {
    let label = "Agent-Org: coordinator cannot spawn materialized org member";
    let body = serde_json::json!({
        "is_shadow": false,
        "is_org_member": false,
        "org_context": member_spawn_org_context(),
        "target_agent_id": "bob",
        "is_background": false,
    });
    let resp = match post_check_member_spawn_gate(cfg, body).await {
        Err(err) => return harness::print_error(label, &err),
        Ok(json) => json,
    };

    let rejected = resp
        .get("rejected")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let error_message = resp
        .get("error_message")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    harness::print_result(
        label,
        &resp.to_string(),
        &[
            (
                "helper rejects coordinator → materialized member spawn",
                rejected,
            ),
            (
                "error message explains launch-time materialization",
                error_message.contains("materialized when the Agent Org launches"),
            ),
            (
                "error message points at teammate coordination alternatives",
                error_message.contains("org_send_message") && error_message.contains("task queue"),
            ),
        ],
    )
}

// ── Member-idle notification ────────────────────────────────────────

/// Drive `POST /test/agent-org/post-member-idle`. Reuses the same
/// `default_context` shape the other scenarios share so the member
/// roster (Alice / Bob) lines up with the inbox-list assertions.
async fn post_member_idle(
    cfg: &Config,
    run_id: &str,
    member_agent_id: &str,
    reason: &str,
) -> Result<serde_json::Value, String> {
    post_member_idle_with_failure_reason(cfg, run_id, member_agent_id, reason, None).await
}

async fn post_member_idle_with_failure_reason(
    cfg: &Config,
    run_id: &str,
    member_agent_id: &str,
    reason: &str,
    failure_reason: Option<&str>,
) -> Result<serde_json::Value, String> {
    let url = format!("{}{}", cfg.base_url, POST_MEMBER_IDLE_PATH);
    let mut body = default_context(run_id, "");
    let obj = body
        .as_object_mut()
        .expect("default_context returns object");
    obj.remove("sender_agent_id");
    obj.insert(
        "member_agent_id".to_string(),
        serde_json::Value::String(member_agent_id.to_string()),
    );
    obj.insert(
        "reason".to_string(),
        serde_json::Value::String(reason.to_string()),
    );
    obj.insert(
        "current_mode".to_string(),
        serde_json::Value::String("plan".to_string()),
    );
    if let Some(value) = failure_reason {
        obj.insert(
            "failure_reason".to_string(),
            serde_json::Value::String(value.to_string()),
        );
    }
    let resp = http_client()
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|err| format!("HTTP error: {err}"))?;
    resp.json::<serde_json::Value>()
        .await
        .map_err(|err| format!("JSON parse error: {err}"))
}

/// Caller-path E2E. Exercises the **full member-idle notification
/// side effect** end-to-end through the production hook stack:
///
/// 1. The probe drives `maybe_emit_member_idle` with Alice's
///    agent_id and `reason = "available"`.
/// 2. The production `InboxStoreMemberIdleHook` (installed at app
///    boot, no test override) persists a `MemberIdle` envelope into
///    `agent_inbox` addressed from `_system` to the coordinator.
/// 3. A subsequent `inbox/list-by-run` asserts the row exists with
///    the right sender / recipient / payload shape — i.e. the LLM
///    cannot forge it from a peer, and the coordinator's next drain
///    will see it and render `<member_idle .../>` into the prompt.
pub async fn member_idle_emit_lands_in_coord_inbox(cfg: &Config) -> bool {
    let label = "Agent-Org: maybe_emit_member_idle → coord inbox member_idle row";
    let run_id = match seed_default_running_run(cfg, "member-idle-emit", None).await {
        Ok(run_id) => run_id,
        Err(err) => return harness::print_error(label, &err),
    };

    let resp = match post_member_idle(cfg, &run_id, "alice-agent", "available").await {
        Err(err) => return harness::print_error(label, &err),
        Ok(json) => json,
    };
    let endpoint_ok = resp.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
    let emitted = resp
        .get("emitted")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let list_resp = match list_inbox(cfg, &run_id).await {
        Err(err) => return harness::print_error(label, &err),
        Ok(json) => json,
    };
    let messages = match messages_array(&list_resp) {
        Err(err) => return harness::print_error(label, &err),
        Ok(arr) => arr,
    };

    let idle_row = messages
        .iter()
        .find(|r| r.get("payload_kind").and_then(|v| v.as_str()) == Some("member_idle"));
    let idle_present = idle_row.is_some();

    let from_system = idle_row
        .and_then(|r| r.get("sender_agent_id").and_then(|v| v.as_str()))
        .map(|s| s == "_system")
        .unwrap_or(false);

    let recipient_is_coord = idle_row
        .and_then(|r| r.get("recipient_agent_id").and_then(|v| v.as_str()))
        .map(|s| s == "coord")
        .unwrap_or(false);

    // The MemberIdle payload must reference Alice with reason `available`
    // and current mode `plan`. `summary` and `failure_reason` are absent
    // for this baseline emit — the production caller does not yet compute
    // a peer-DM summary, so the decoded payload elides those fields.
    let payload_ok = idle_row
        .and_then(|r| r.get("payload_decoded"))
        .map(|inner| {
            let member_id_match = inner
                .get("member_id")
                .and_then(|v| v.as_str())
                .map(|s| s == "m1")
                .unwrap_or(false);
            let member_name_match = inner
                .get("member_name")
                .and_then(|v| v.as_str())
                .map(|s| s == "Alice")
                .unwrap_or(false);
            let reason_match = inner
                .get("reason")
                .and_then(|v| v.as_str())
                .map(|s| s == "available")
                .unwrap_or(false);
            let current_mode_match = inner
                .get("current_mode")
                .and_then(|v| v.as_str())
                .map(|s| s == "plan")
                .unwrap_or(false);
            let summary_blank = inner
                .get("summary")
                .map(|v| v.is_null() || v.as_str().map(|s| s.trim().is_empty()).unwrap_or(false))
                .unwrap_or(true);
            let failure_blank = inner
                .get("failure_reason")
                .map(|v| v.is_null() || v.as_str().map(|s| s.trim().is_empty()).unwrap_or(false))
                .unwrap_or(true);
            member_id_match
                && member_name_match
                && reason_match
                && current_mode_match
                && summary_blank
                && failure_blank
        })
        .unwrap_or(false);

    let still_unread = idle_row
        .and_then(|r| r.get("read_at"))
        .map(|v| v.is_null())
        .unwrap_or(false);

    harness::print_result(
        label,
        &list_resp.to_string(),
        &[
            ("endpoint returned ok", endpoint_ok),
            ("endpoint reported emitted=true", emitted),
            ("inbox has a member_idle row", idle_present),
            (
                "member_idle.sender_agent_id == _system (LLM-unforgeable)",
                from_system,
            ),
            (
                "member_idle.recipient_agent_id == coord",
                recipient_is_coord,
            ),
            (
                "member_idle payload references Alice/available/plan with no extra fields",
                payload_ok,
            ),
            (
                "member_idle still unread (next-turn delivery to coordinator)",
                still_unread,
            ),
        ],
    )
}

/// Negative pin. The coordinator's own turn end is a no-op:
/// `maybe_emit_member_idle` short-circuits when the agent_id matches
/// `coordinator_agent_id`, so the inbox stays untouched. Without this
/// pin a future refactor that drops the coordinator-skip branch
/// would silently fan a coordinator's own idle notifications into
/// its own inbox — feedback-loop heaven.
pub async fn member_idle_emit_skips_coordinator(cfg: &Config) -> bool {
    let label = "Agent-Org: coordinator turn end does NOT emit member_idle to itself";
    let run_id = unique_run_id("member-idle-emit-coord");

    // Probe with member_agent_id == coord. The endpoint reports
    // emitted=false (no row added) and the listed inbox stays empty.
    let resp = match post_member_idle(cfg, &run_id, "coord", "available").await {
        Err(err) => return harness::print_error(label, &err),
        Ok(json) => json,
    };
    let endpoint_ok = resp.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
    let emitted_reported_false = !resp
        .get("emitted")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    let list_resp = match list_inbox(cfg, &run_id).await {
        Err(err) => return harness::print_error(label, &err),
        Ok(json) => json,
    };
    let messages = match messages_array(&list_resp) {
        Err(err) => return harness::print_error(label, &err),
        Ok(arr) => arr,
    };
    let no_idle_rows = messages
        .iter()
        .all(|r| r.get("payload_kind").and_then(|v| v.as_str()) != Some("member_idle"));

    harness::print_result(
        label,
        &list_resp.to_string(),
        &[
            ("endpoint returned ok", endpoint_ok),
            (
                "endpoint reported emitted=false (coordinator skip)",
                emitted_reported_false,
            ),
            (
                "inbox contains no member_idle row from coordinator self-end",
                no_idle_rows,
            ),
        ],
    )
}

/// Reason-propagation pin. The same call path with
/// `reason = "interrupted"` must persist the `interrupted` reason
/// verbatim in the decoded payload — confirming the worker's
/// final_turn_state mapping (Cancelled → Interrupted) is preserved
/// through the hook → store round-trip.
pub async fn member_idle_emit_propagates_interrupted(cfg: &Config) -> bool {
    let label = "Agent-Org: member_idle reason=interrupted round-trips through inbox";
    let run_id = match seed_default_running_run(cfg, "member-idle-emit-interrupted", None).await {
        Ok(run_id) => run_id,
        Err(err) => return harness::print_error(label, &err),
    };

    let resp = match post_member_idle(cfg, &run_id, "alice-agent", "interrupted").await {
        Err(err) => return harness::print_error(label, &err),
        Ok(json) => json,
    };
    let endpoint_ok = resp.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);

    let list_resp = match list_inbox(cfg, &run_id).await {
        Err(err) => return harness::print_error(label, &err),
        Ok(json) => json,
    };
    let messages = match messages_array(&list_resp) {
        Err(err) => return harness::print_error(label, &err),
        Ok(arr) => arr,
    };
    let idle_row = messages
        .iter()
        .find(|r| r.get("payload_kind").and_then(|v| v.as_str()) == Some("member_idle"));
    let reason_interrupted = idle_row
        .and_then(|r| r.get("payload_decoded"))
        .and_then(|p| p.get("reason"))
        .and_then(|v| v.as_str())
        .map(|s| s == "interrupted")
        .unwrap_or(false);

    harness::print_result(
        label,
        &list_resp.to_string(),
        &[
            ("endpoint returned ok", endpoint_ok),
            ("member_idle row present", idle_row.is_some()),
            (
                "member_idle.payload_decoded.reason == interrupted",
                reason_interrupted,
            ),
        ],
    )
}

/// Failure-propagation pin. This is the cheap E2E counterpart to the
/// lifecycle 429/unit coverage: a failed member-idle emit must persist both
/// `reason = failed` and the runtime failure string so the coordinator can
/// make an explicit recovery decision instead of seeing a generic idle event.
pub async fn member_idle_emit_propagates_failed_reason(cfg: &Config) -> bool {
    let label = "Agent-Org: member_idle reason=failed carries failure_reason";
    let run_id = match seed_default_running_run(cfg, "member-idle-emit-failed", None).await {
        Ok(run_id) => run_id,
        Err(err) => return harness::print_error(label, &err),
    };
    let failure_reason = "HTTP 429: rate limit exceeded";

    let resp = match post_member_idle_with_failure_reason(
        cfg,
        &run_id,
        "alice-agent",
        "failed",
        Some(failure_reason),
    )
    .await
    {
        Err(err) => return harness::print_error(label, &err),
        Ok(json) => json,
    };
    let endpoint_ok = resp
        .get("ok")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let emitted = resp
        .get("emitted")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);

    let list_resp = match list_inbox(cfg, &run_id).await {
        Err(err) => return harness::print_error(label, &err),
        Ok(json) => json,
    };
    let messages = match messages_array(&list_resp) {
        Err(err) => return harness::print_error(label, &err),
        Ok(arr) => arr,
    };
    let idle_row = messages.iter().find(|row| {
        row.get("payload_kind").and_then(|value| value.as_str()) == Some("member_idle")
    });

    let payload_ok = idle_row
        .and_then(|row| row.get("payload_decoded"))
        .map(|payload| {
            let member_id_match = payload
                .get("member_id")
                .and_then(|value| value.as_str())
                .map(|value| value == "m1")
                .unwrap_or(false);
            let member_name_match = payload
                .get("member_name")
                .and_then(|value| value.as_str())
                .map(|value| value == "Alice")
                .unwrap_or(false);
            let reason_failed = payload
                .get("reason")
                .and_then(|value| value.as_str())
                .map(|value| value == "failed")
                .unwrap_or(false);
            let failure_reason_match = payload
                .get("failure_reason")
                .and_then(|value| value.as_str())
                .map(|value| value == failure_reason)
                .unwrap_or(false);
            member_id_match && member_name_match && reason_failed && failure_reason_match
        })
        .unwrap_or(false);

    let recipient_is_coord = idle_row
        .and_then(|row| row.get("recipient_agent_id"))
        .and_then(|value| value.as_str())
        .map(|value| value == "coord")
        .unwrap_or(false);

    harness::print_result(
        label,
        &list_resp.to_string(),
        &[
            ("endpoint returned ok", endpoint_ok),
            ("endpoint reported emitted=true", emitted),
            ("member_idle row present", idle_row.is_some()),
            (
                "member_idle.recipient_agent_id == coord",
                recipient_is_coord,
            ),
            (
                "member_idle.payload_decoded.reason == failed with failure_reason",
                payload_ok,
            ),
        ],
    )
}

/// Pause/resume API correctly toggles run status and is idempotent.
///
/// Scenario:
/// 1. Seed an org run in `running` status (via `seed-cli-member`).
/// 2. POST `/run/pause` → run transitions to `paused`; `transitioned=true`.
/// 3. Verify durable invariants show `runStatus = "paused"`.
/// 4. POST `/run/pause` again → idempotent; `transitioned=false`, still `paused`.
/// 5. POST `/run/resume` → run transitions back to `running`; `transitioned=true`.
/// 6. Verify durable invariants show `runStatus = "running"`.
/// 7. POST `/run/resume` again → idempotent; `transitioned=false`, still `running`.
pub async fn run_pause_resume_toggles_status(cfg: &Config) -> bool {
    let label = "Agent-Org: pause/resume API toggles run status and is idempotent";

    let seed_resp = match post_agent_org_json(
        cfg,
        SEED_CLI_MEMBER_RUN_PATH,
        serde_json::json!({
            "cli_agent_type": "claude_code",
            "member_id": "m-pr",
            "status": "idle"
        }),
    )
    .await
    {
        Err(err) => return harness::print_error(label, &err),
        Ok(json) => json,
    };
    let seed_ok = seed_resp.get("ok").and_then(|v| v.as_bool()) == Some(true);
    if !seed_ok {
        return harness::print_error(label, &seed_resp.to_string());
    }
    let org_run_id = match seed_resp.get("org_run_id").and_then(|v| v.as_str()) {
        Some(value) if !value.is_empty() => value.to_string(),
        _ => return harness::print_error(label, "seed did not return org_run_id"),
    };
    let root_session_id = match seed_resp.get("root_session_id").and_then(|v| v.as_str()) {
        Some(value) if !value.is_empty() => value.to_string(),
        _ => return harness::print_error(label, "seed did not return root_session_id"),
    };
    if let Err(err) = seed_task(
        cfg,
        &org_run_id,
        &format!("pause-keep-open-{org_run_id}"),
        "Keep pause/resume fixture open",
        "m-pr",
        TASK_STATUS_PENDING,
    )
    .await
    {
        return harness::print_error(label, &err);
    }

    // (2) First pause — should transition
    let pause1 = match post_agent_org_json(
        cfg,
        PAUSE_RUN_PATH,
        serde_json::json!({ "org_run_id": org_run_id }),
    )
    .await
    {
        Err(err) => return harness::print_error(label, &err),
        Ok(json) => json,
    };
    let pause1_ok = pause1.get("ok").and_then(|v| v.as_bool()) == Some(true);
    let pause1_transitioned = pause1.get("transitioned").and_then(|v| v.as_bool()) == Some(true);

    // (3) Durable invariants after first pause
    let inv_after_pause = match post_agent_org_json(
        cfg,
        DURABLE_INVARIANTS_PATH,
        serde_json::json!({ "org_run_id": org_run_id, "root_session_id": root_session_id }),
    )
    .await
    {
        Err(err) => return harness::print_error(label, &err),
        Ok(json) => json,
    };
    let run_status_after_pause = inv_after_pause
        .get("runStatus")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    // (4) Second pause — idempotent
    let pause2 = match post_agent_org_json(
        cfg,
        PAUSE_RUN_PATH,
        serde_json::json!({ "org_run_id": org_run_id }),
    )
    .await
    {
        Err(err) => return harness::print_error(label, &err),
        Ok(json) => json,
    };
    let pause2_ok = pause2.get("ok").and_then(|v| v.as_bool()) == Some(true);
    let pause2_not_transitioned =
        pause2.get("transitioned").and_then(|v| v.as_bool()) == Some(false);

    // (5) First resume — should transition
    let resume1 = match post_agent_org_json(
        cfg,
        RESUME_RUN_PATH,
        serde_json::json!({ "org_run_id": org_run_id }),
    )
    .await
    {
        Err(err) => return harness::print_error(label, &err),
        Ok(json) => json,
    };
    let resume1_ok = resume1.get("ok").and_then(|v| v.as_bool()) == Some(true);
    let resume1_transitioned = resume1.get("transitioned").and_then(|v| v.as_bool()) == Some(true);

    // (6) Durable invariants after resume
    let inv_after_resume = match post_agent_org_json(
        cfg,
        DURABLE_INVARIANTS_PATH,
        serde_json::json!({ "org_run_id": org_run_id, "root_session_id": root_session_id }),
    )
    .await
    {
        Err(err) => return harness::print_error(label, &err),
        Ok(json) => json,
    };
    let run_status_after_resume = inv_after_resume
        .get("runStatus")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    // (7) Second resume — idempotent
    let resume2 = match post_agent_org_json(
        cfg,
        RESUME_RUN_PATH,
        serde_json::json!({ "org_run_id": org_run_id }),
    )
    .await
    {
        Err(err) => return harness::print_error(label, &err),
        Ok(json) => json,
    };
    let resume2_ok = resume2.get("ok").and_then(|v| v.as_bool()) == Some(true);
    let resume2_not_transitioned =
        resume2.get("transitioned").and_then(|v| v.as_bool()) == Some(false);

    harness::print_result(
        label,
        &serde_json::json!({
            "seed": seed_resp,
            "pause1": pause1,
            "inv_after_pause": inv_after_pause,
            "pause2": pause2,
            "resume1": resume1,
            "inv_after_resume": inv_after_resume,
            "resume2": resume2,
        })
        .to_string(),
        &[
            ("seed ok", seed_ok),
            ("pause1: endpoint ok", pause1_ok),
            ("pause1: transitioned=true", pause1_transitioned),
            (
                "run status after pause is 'paused'",
                run_status_after_pause == "paused",
            ),
            ("pause2 (idempotent): endpoint ok", pause2_ok),
            (
                "pause2 (idempotent): transitioned=false",
                pause2_not_transitioned,
            ),
            ("resume1: endpoint ok", resume1_ok),
            ("resume1: transitioned=true", resume1_transitioned),
            (
                "run status after resume is 'running'",
                run_status_after_resume == "running",
            ),
            ("resume2 (idempotent): endpoint ok", resume2_ok),
            (
                "resume2 (idempotent): transitioned=false",
                resume2_not_transitioned,
            ),
        ],
    )
}

/// Production-caller-path restart pin for one interrupted TaskExecution.
/// The debug helper establishes only the crash-cut precondition; the restart
/// endpoint executes the same recovery owner and exact receipt dispatch as
/// application startup.
pub async fn app_restart_recovers_exact_task_execution_once(cfg: &Config) -> bool {
    let label = "app-restart-recovers-exact-task-execution-once";
    let crashed_task_id = unique_run_id("restart-crashed-task");

    let seed_resp = match post_agent_org_json(
        cfg,
        SEED_CRASHED_TASK_EXECUTION_PATH,
        serde_json::json!({
            "member_id": "m-restart",
            "task_id": crashed_task_id,
        }),
    )
    .await
    {
        Err(err) => return harness::print_error(label, &err),
        Ok(json) => json,
    };
    let seed_ok = seed_resp.get("ok").and_then(|v| v.as_bool()) == Some(true);
    let org_run_id = match seed_resp.get("org_run_id").and_then(|v| v.as_str()) {
        Some(value) if !value.is_empty() => value.to_string(),
        _ => return harness::print_error(label, "seed did not return org_run_id"),
    };
    let root_session_id = match seed_resp.get("root_session_id").and_then(|v| v.as_str()) {
        Some(value) if !value.is_empty() => value.to_string(),
        _ => return harness::print_error(label, "seed did not return root_session_id"),
    };
    let crashed_session_id = seed_resp
        .get("crashed_session_id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string();
    let crashed_turn_intent_id = seed_resp
        .get("crashed_turn_intent_id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string();

    let inv_before_restart = match post_agent_org_json(
        cfg,
        DURABLE_INVARIANTS_PATH,
        serde_json::json!({ "org_run_id": org_run_id, "root_session_id": root_session_id }),
    )
    .await
    {
        Err(err) => return harness::print_error(label, &err),
        Ok(json) => json,
    };
    let run_status_before = inv_before_restart
        .get("runStatus")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    let first_restart =
        match post_agent_org_json(cfg, SIMULATE_APP_RESTART_PATH, serde_json::json!({})).await {
            Err(err) => return harness::print_error(label, &err),
            Ok(json) => json,
        };
    let recovered = first_restart
        .get("recovery_plan")
        .and_then(|value| value.get("recoveredTasks"))
        .and_then(serde_json::Value::as_array)
        .and_then(|items| {
            items.iter().find(|item| {
                item.get("task")
                    .and_then(|task| task.get("id"))
                    .and_then(serde_json::Value::as_str)
                    == Some(crashed_task_id.as_str())
            })
        });
    let exact_recovery_ok = recovered.is_some_and(|recovery| {
        recovery
            .get("task")
            .and_then(|task| task.get("status"))
            .and_then(serde_json::Value::as_str)
            == Some("pending")
            && recovery
                .get("task")
                .and_then(|task| task.get("owner"))
                .is_some_and(serde_json::Value::is_null)
            && recovery
                .get("previousOwnerMemberId")
                .and_then(serde_json::Value::as_str)
                == Some("m-restart")
            && recovery
                .get("failedSessionId")
                .and_then(serde_json::Value::as_str)
                == Some(crashed_session_id.as_str())
            && recovery
                .get("failedTurnIntentId")
                .and_then(serde_json::Value::as_str)
                == Some(crashed_turn_intent_id.as_str())
            && recovery
                .get("receiptId")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|value| !value.is_empty())
    });

    let second_restart =
        match post_agent_org_json(cfg, SIMULATE_APP_RESTART_PATH, serde_json::json!({})).await {
            Err(err) => return harness::print_error(label, &err),
            Ok(json) => json,
        };
    let third_restart =
        match post_agent_org_json(cfg, SIMULATE_APP_RESTART_PATH, serde_json::json!({})).await {
            Err(err) => return harness::print_error(label, &err),
            Ok(json) => json,
        };

    let inv_after_restart = match post_agent_org_json(
        cfg,
        DURABLE_INVARIANTS_PATH,
        serde_json::json!({ "org_run_id": org_run_id, "root_session_id": root_session_id }),
    )
    .await
    {
        Err(err) => return harness::print_error(label, &err),
        Ok(json) => json,
    };
    let run_status_after_restart = inv_after_restart
        .get("runStatus")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let inbox = match list_inbox(cfg, &org_run_id).await {
        Err(err) => return harness::print_error(label, &err),
        Ok(json) => json,
    };
    let matching_recovery_inbox_count = inbox
        .get("messages")
        .and_then(serde_json::Value::as_array)
        .map(|messages| {
            messages
                .iter()
                .filter(|message| {
                    message
                        .get("payload_kind")
                        .and_then(serde_json::Value::as_str)
                        == Some("member_idle")
                        && message
                            .get("payload_decoded")
                            .and_then(|payload| payload.get("reason"))
                            .and_then(serde_json::Value::as_str)
                            == Some("failed")
                        && message
                            .get("payload_decoded")
                            .and_then(|payload| payload.get("unfinished_task_ids"))
                            .and_then(serde_json::Value::as_array)
                            .is_some_and(|ids| {
                                ids.iter()
                                    .any(|id| id.as_str() == Some(crashed_task_id.as_str()))
                            })
                })
                .count()
        })
        .unwrap_or(0);

    harness::print_result(
        label,
        &serde_json::json!({
            "seed": seed_resp,
            "first_restart": first_restart,
            "second_restart": second_restart,
            "third_restart": third_restart,
            "inv_before_restart": inv_before_restart,
            "inv_after_restart": inv_after_restart,
            "inbox": inbox,
        })
        .to_string(),
        &[
            ("seed ok", seed_ok),
            (
                "crash fixture has exact session",
                !crashed_session_id.is_empty(),
            ),
            (
                "crash fixture has exact Turn",
                !crashed_turn_intent_id.is_empty(),
            ),
            (
                "run status before restart is 'running'",
                run_status_before == "running",
            ),
            (
                "first restart endpoint ok",
                first_restart.get("ok").and_then(serde_json::Value::as_bool) == Some(true),
            ),
            (
                "exact TaskExecution became safe ownerless Pending",
                exact_recovery_ok,
            ),
            (
                "one typed Coordinator recovery inbox fact exists",
                matching_recovery_inbox_count == 1,
            ),
            (
                "running Team lifecycle is preserved",
                run_status_after_restart == "running",
            ),
            (
                "no ownerless in-progress Task remains",
                inv_after_restart
                    .get("ownerlessInProgressCount")
                    .and_then(serde_json::Value::as_u64)
                    == Some(0),
            ),
            (
                "second and third restart are idempotent",
                second_restart
                    .get("tasks_requeued")
                    .and_then(serde_json::Value::as_u64)
                    == Some(0)
                    && third_restart
                        .get("tasks_requeued")
                        .and_then(serde_json::Value::as_u64)
                        == Some(0),
            ),
        ],
    )
}
