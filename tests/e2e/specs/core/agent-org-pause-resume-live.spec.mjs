/* global browser, before, describe, it, process */
import { execFileSync } from "node:child_process";
import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";

import {
  RENDER_TIMEOUT_MS,
  REPLY_TIMEOUT_MS,
  configureCreatorForAgentOrg,
  execJS,
  getApiAccount,
  invokeE2E,
  openAgentOrgOverviewPanel,
  removeAgentOrgsByName,
  selectPreferredModel,
  selectRenderedAgentOrg,
  sendFromRenderedCreator,
  unwrap,
  waitForAgentOrgRunView,
  waitForAgentOrgRunViewByOrg,
  waitForApp,
} from "../../support/core/agentOrgUiDriver.mjs";

const BASE_URL = `http://127.0.0.1:${process.env.E2E_IDE_SERVER_PORT ?? "13847"}`;
const PHASE = process.env.E2E_AGENT_ORG_LIVE_PHASE ?? "";
const ROUND = (process.env.E2E_AGENT_ORG_LIVE_ROUND_ID ?? "")
  .replace(/[^a-zA-Z0-9_-]/g, "-")
  .slice(0, 40);
const PROVIDER_MODE = process.env.E2E_PROVIDER_MODE ?? "mock";
const ORG_ID = `e2e-pause-resume-live-${ROUND}`;
const ORG_NAME = `E2E Pause Resume Live ${ROUND}`;
const MEMBER_ID = "pause-worker";
// Keep the durable/process marker compatible with sendRenderedChatPrompt's
// compact marker probe. A hyphenated round id makes that helper fall back to
// comparing the entire shell-heavy prompt inside JSON-escaped state.
const MARKER_ROUND = ROUND.replaceAll("-", "_");
const PROCESS_MARKER = `ORGII_PAUSE_LIVE_${MARKER_ROUND}`;
const FINALITY_MARKER = `ORGII_RESUME_FINALITY_${MARKER_ROUND}`;
const ARCHIVE_MARKER = `ORGII_ARCHIVE_LIVE_${MARKER_ROUND}`;
const ORGII_HOME = process.env.E2E_ORGII_HOME ?? "";

async function postJson(pathname, body = {}, timeoutMs = 15_000) {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  try {
    const response = await fetch(`${BASE_URL}${pathname}`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body),
      signal: controller.signal,
    });
    const json = await response.json();
    if (!response.ok || json?.ok !== true) {
      throw new Error(`${pathname} failed: ${JSON.stringify(json)}`);
    }
    return json;
  } finally {
    clearTimeout(timer);
  }
}

async function visibleProductButton(selector, marker) {
  let state = null;
  await browser.waitUntil(
    async () => {
      state = await execJS(`
        const visible = (element) => {
          const rect = element.getBoundingClientRect();
          const style = getComputedStyle(element);
          return rect.width > 0 && rect.height > 0 && style.display !== "none" && style.visibility !== "hidden";
        };
        const elements = Array.from(document.querySelectorAll(${JSON.stringify(selector)}));
        for (const element of elements) element.removeAttribute(${JSON.stringify(marker)});
        const element = elements.find(visible) ?? null;
        if (element) element.setAttribute(${JSON.stringify(marker)}, "true");
        return { count: elements.length, marked: Boolean(element), disabled: element?.disabled ?? null };
      `);
      return state?.marked && state.disabled === false;
    },
    {
      timeout: RENDER_TIMEOUT_MS,
      interval: 50,
      timeoutMsg: `No enabled visible product control for ${selector}: ${JSON.stringify(state)}`,
    }
  );
  return browser.$(`[${marker}="true"]`);
}

function processGroupSnapshot(processGroupId) {
  return execFileSync("ps", ["-ax", "-o", "pid=,ppid=,pgid=,command="], {
    encoding: "utf8",
  })
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean)
    .map((line) => {
      const match = line.match(/^(\d+)\s+(\d+)\s+(\d+)\s+(.*)$/);
      return match
        ? {
            pid: Number(match[1]),
            parentPid: Number(match[2]),
            processGroupId: Number(match[3]),
            command: match[4],
          }
        : null;
    })
    .filter((row) => row?.processGroupId === processGroupId);
}

function pidExists(pid) {
  try {
    process.kill(pid, 0);
    return true;
  } catch (error) {
    if (error?.code === "ESRCH") return false;
    throw error;
  }
}

function filesContainingMarker(root, marker) {
  const matches = [];
  const visit = (path) => {
    let entries;
    try {
      entries = readdirSync(path, { withFileTypes: true });
    } catch {
      return;
    }
    for (const entry of entries) {
      const child = join(path, entry.name);
      if (entry.isDirectory()) {
        visit(child);
        continue;
      }
      try {
        if (readFileSync(child, "utf8").includes(marker)) matches.push(child);
      } catch {
        // Ignore non-text runtime files; only shell replay logs can match.
      }
    }
  };
  visit(root);
  return matches;
}

function durableRunIdForRootSession(sessionId) {
  try {
    return execFileSync(
      "sqlite3",
      [
        join(ORGII_HOME, "sessions.db"),
        `SELECT id FROM agent_org_runtime_runs WHERE root_session_id='${sessionId.replaceAll("'", "''")}' ORDER BY created_at DESC LIMIT 1;`,
      ],
      { encoding: "utf8" }
    ).trim();
  } catch {
    return "";
  }
}

function sqlLiteral(value) {
  return `'${String(value).replaceAll("'", "''")}'`;
}

function sqliteRow(sql) {
  const output = execFileSync(
    "sqlite3",
    ["-json", "-cmd", ".timeout 5000", join(ORGII_HOME, "sessions.db"), sql],
    { encoding: "utf8" }
  ).trim();
  const rows = output ? JSON.parse(output) : [];
  return rows[0] ?? null;
}

function convergenceSnapshot({
  runId,
  taskId,
  memberSessionId,
  continuationId,
}) {
  const run = sqlLiteral(runId);
  const task = sqlLiteral(taskId);
  const session = sqlLiteral(memberSessionId);
  const continuation = sqlLiteral(continuationId);
  const finalityMarker = sqlLiteral(`%${FINALITY_MARKER}%`);
  return sqliteRow(`
    SELECT
      (SELECT status FROM agent_org_runtime_runs WHERE id=${run}) AS run_status,
      (SELECT status FROM agent_org_runtime_tasks
         WHERE org_run_id=${run} AND id=${task}) AS task_status,
      (SELECT status FROM session_turn_intents
         WHERE session_id=${session} AND turn_intent_id=${continuation}) AS continuation_status,
      (SELECT COUNT(*) FROM session_turn_intents
         WHERE org_run_id=${run} AND status IN ('queued','running')) AS active_intent_count,
      (SELECT COUNT(*) FROM session_turn_intents
         WHERE org_run_id=${run}) AS intent_count,
      (SELECT COUNT(*) FROM agent_org_runtime_turn_contexts
         WHERE org_run_id=${run}) AS context_count,
      (SELECT COUNT(*) FROM agent_org_runtime_inbox inbox
         WHERE inbox.org_run_id=${run}
           AND inbox.payload_kind='task_assigned'
           AND json_extract(inbox.payload_json,'$.task_id')=${task}
           AND inbox.read_at IS NULL
           AND NOT EXISTS (
             SELECT 1 FROM agent_org_runtime_inbox_delivery_resolutions resolution
             WHERE resolution.inbox_id=inbox.id
           )) AS unresolved_assignment_count,
      (SELECT COUNT(*)
         FROM agent_org_runtime_inbox_materializations materialization
         JOIN agent_org_runtime_inbox inbox ON inbox.id=materialization.inbox_id
         WHERE inbox.org_run_id=${run}
           AND inbox.payload_kind='task_assigned'
           AND json_extract(inbox.payload_json,'$.task_id')=${task}) AS assignment_materialization_count,
      (SELECT COUNT(*) FROM agent_messages
         WHERE session_id=${session} AND role='assistant') AS assistant_count,
      (SELECT COUNT(*) FROM agent_messages
         WHERE session_id=${session} AND role='assistant'
           AND content LIKE ${finalityMarker}) AS finality_assistant_count
  `);
}

async function assertFeatureGatePreflight() {
  const frontendGate = await execJS(`
    return {
      helperPresent: Boolean(window.__e2e),
      enablePresent: typeof window.__e2e?.debugAgentOrgEnable === "function",
      compiledIdeUrl: window.__ORGII_E2E_IDE_SERVER_WS_URL__ ?? null,
    };
  `);
  if (!frontendGate.helperPresent || !frontendGate.enablePresent) {
    throw new Error(
      `frontend WebDriver compile gate is disabled: ${JSON.stringify(frontendGate)}`
    );
  }
  const enabled = unwrap(
    await invokeE2E("debugAgentOrgEnable"),
    "enable Agent Org through webdriver-only Rust gate"
  );
  if (enabled.enabled !== true) {
    throw new Error(
      `Rust Agent Org runtime gate did not enable: ${JSON.stringify(enabled)}`
    );
  }
  console.info(
    `[agent-org-live-feature-gates] ${JSON.stringify({ frontendGate, cargoWebdriverAndRuntimeGate: true })}`
  );
}

function assertLiveInputs() {
  if (!ROUND) throw new Error("E2E_AGENT_ORG_LIVE_ROUND_ID is required");
  if (!ORGII_HOME) throw new Error("E2E_ORGII_HOME is required");
  if (!new Set(["pause", "resume", "archive", "task-smoke"]).has(PHASE)) {
    throw new Error(`unsupported E2E_AGENT_ORG_LIVE_PHASE=${PHASE}`);
  }
  if (PROVIDER_MODE === "mock") {
    throw new Error("live acceptance refuses E2E_PROVIDER_MODE=mock");
  }
}

function archiveConvergenceSnapshot(runId) {
  const run = sqlLiteral(runId);
  return sqliteRow(`
    SELECT
      (SELECT status FROM agent_org_runtime_runs WHERE id=${run}) AS run_status,
      (SELECT teardown_status FROM agent_org_runtime_archive_episodes
         WHERE org_run_id=${run}) AS teardown_status,
      (SELECT teardown_attempt_count FROM agent_org_runtime_archive_episodes
         WHERE org_run_id=${run}) AS teardown_attempt_count,
      (SELECT retained_runtime_count FROM agent_org_runtime_archive_episodes
         WHERE org_run_id=${run}) AS retained_runtime_count,
      (SELECT COUNT(*) FROM agent_org_runtime_tasks
         WHERE org_run_id=${run}) AS task_count,
      (SELECT COUNT(*) FROM agent_org_runtime_tasks
         WHERE org_run_id=${run} AND status NOT IN ('completed','failed','cancelled')) AS open_task_count,
      (SELECT COUNT(*) FROM session_turn_intents
         WHERE org_run_id=${run} AND status IN ('queued','running')) AS active_intent_count,
      (SELECT COUNT(*) FROM agent_org_runtime_turn_contexts
         WHERE org_run_id=${run}) AS context_count,
      (SELECT COUNT(*) FROM agent_org_runtime_inbox
         WHERE org_run_id=${run}) AS inbox_count,
      (SELECT COUNT(*) FROM agent_messages message
         JOIN agent_org_runtime_member_materializations member
           ON member.session_id=message.session_id
         WHERE member.org_run_id=${run}) AS member_message_count,
      (SELECT MAX(updated_at) FROM agent_org_runtime_tasks
         WHERE org_run_id=${run}) AS latest_task_updated_at
  `);
}

async function runArchivePhase() {
  const account = await getApiAccount();
  const model = selectPreferredModel(account);
  await seedOneMemberOrg();
  await configureCreatorForAgentOrg({ account, model, agentOrgId: ORG_ID });
  await selectRenderedAgentOrg(ORG_ID);

  const backgroundCommand = [
    "trap '' TERM;",
    `sh -c 'trap "" TERM; while :; do sleep 120; done' & child=$!;`,
    `printf '${ARCHIVE_MARKER} parent=%s child=%s\\n' "$$" "$child";`,
    "wait",
  ].join(" ");
  const prompt = [
    `This is live Archive acceptance round ${ROUND}.`,
    `Use task_graph_create exactly once to create exactly one Task assigned to ${MEMBER_ID}.`,
    `The Task subject must contain ${ARCHIVE_MARKER}.`,
    "Its description must tell the Member to call run_shell exactly once with mode=background",
    `and this exact command: ${backgroundCommand}`,
    "After starting it, keep the Task in progress and do not kill or await the shell.",
    "Do not run the command as coordinator and do not create another Task.",
  ].join(" ");
  const sessionId = await sendFromRenderedCreator(prompt);
  if (!sessionId)
    throw new Error("live Archive launch produced no root Session");

  const view = await waitForAgentOrgRunView(
    sessionId,
    (candidate) =>
      candidate?.runStatus === "running" &&
      candidate?.tasks?.length === 1 &&
      candidate.tasks[0]?.owner === MEMBER_ID,
    "real Provider created one assigned Archive Task",
    REPLY_TIMEOUT_MS * 2
  );
  const runId = view.context.runId;
  let before = null;
  let targetShell = null;
  await browser.waitUntil(
    async () => {
      before = await postJson("/agent/test/agent-org/runtime-evidence", {
        org_run_id: runId,
      });
      targetShell = before.runtime.background_shells.find((shell) =>
        String(shell.command).includes(ARCHIVE_MARKER)
      );
      return (
        Boolean(targetShell) &&
        before.runtime.execution_blockers.some(
          (job) => job.handle === String(targetShell.pid)
        ) &&
        processGroupSnapshot(targetShell.pid).length >= 3
      );
    },
    {
      timeout: REPLY_TIMEOUT_MS * 2,
      interval: 250,
      timeoutMsg: `real Provider Member never started the Archive parent/child process group: ${JSON.stringify(before)}`,
    }
  );
  const processRows = processGroupSnapshot(targetShell.pid);
  const knownPids = processRows.map((row) => row.pid);
  const replayFiles = filesContainingMarker(
    join(ORGII_HOME, "shell-replays"),
    ARCHIVE_MARKER
  );
  if (replayFiles.length !== 1) {
    throw new Error(
      `Archive command did not start exactly once: ${JSON.stringify(replayFiles)}`
    );
  }

  await openAgentOrgOverviewPanel("real Provider Archive control");
  await execJS("window.__orgiiE2EAutoConfirmDestructive = true; return true;");
  const archiveButton = await visibleProductButton(
    '[data-testid="agent-org-overview-archive-button"]',
    "data-e2e-live-archive"
  );
  const archiveStartedAt = Date.now();
  await archiveButton.click();
  await browser.waitUntil(
    async () =>
      execJS(`
        const panel = document.querySelector('[data-testid="agent-org-overview-panel"]');
        return panel?.getAttribute('data-run-phase') === 'archived'
          && !!document.querySelector('[data-testid="agent-org-archived-composer"]');
      `),
    {
      timeout: RENDER_TIMEOUT_MS,
      interval: 50,
      timeoutMsg: "Archive did not project the immediate read-only UI",
    }
  );
  const readOnlyProjectionMs = Date.now() - archiveStartedAt;

  let quiesced = null;
  await browser.waitUntil(
    async () => {
      quiesced = await postJson("/agent/test/agent-org/runtime-evidence", {
        org_run_id: runId,
      });
      return (
        quiesced.durable.run_status === "archived" &&
        quiesced.durable.archive?.status === "quiesced" &&
        quiesced.durable.archive?.retainedRuntimeCount === 0 &&
        quiesced.runtime.active_runtime_count === 0 &&
        quiesced.runtime.active_turns.length === 0 &&
        quiesced.runtime.background_jobs.length === 0 &&
        quiesced.runtime.execution_blockers.length === 0
      );
    },
    {
      timeout: 65_000,
      interval: 100,
      timeoutMsg: `Archive receipt became read-only but never proved runtime/job finality: ${JSON.stringify(quiesced)}`,
    }
  );
  const quiescenceMs = Date.now() - archiveStartedAt;
  const survivors = processGroupSnapshot(targetShell.pid);
  const liveKnownPids = knownPids.filter(pidExists);
  if (survivors.length > 0 || liveKnownPids.length > 0) {
    throw new Error(
      `Archive left parent/child processes alive: ${JSON.stringify({ survivors, liveKnownPids, processRows })}`
    );
  }

  const quietBefore = archiveConvergenceSnapshot(runId);
  await browser.pause(5_000);
  const quietEvidence = await postJson(
    "/agent/test/agent-org/runtime-evidence",
    { org_run_id: runId }
  );
  const quietAfter = archiveConvergenceSnapshot(runId);
  if (
    JSON.stringify(quietBefore) !== JSON.stringify(quietAfter) ||
    quietEvidence.runtime.active_runtime_count !== 0 ||
    quietEvidence.runtime.active_turns.length !== 0 ||
    quietEvidence.runtime.background_jobs.length !== 0 ||
    processGroupSnapshot(targetShell.pid).length !== 0 ||
    filesContainingMarker(join(ORGII_HOME, "shell-replays"), ARCHIVE_MARKER)
      .length !== 1
  ) {
    throw new Error(
      `late Provider, Task, Inbox, message, replay, or process activity appeared after Archive: ${JSON.stringify({ quietBefore, quietAfter, quietEvidence })}`
    );
  }

  console.info(
    `[agent-org-live-archive-evidence] ${JSON.stringify({
      round: ROUND,
      provider: { accountId: account.id, accountName: account.name, model },
      runId,
      taskId: view.tasks[0]?.id,
      processGroupId: targetShell.pid,
      processRows,
      replayFiles,
      readOnlyProjectionMs,
      quiescenceMs,
      retainedTombstoneCount: quietEvidence.runtime.retained_tombstone_count,
      quiet: quietAfter,
    })}`
  );
}

async function seedOneMemberOrg() {
  await removeAgentOrgsByName(ORG_NAME);
  await postJson("/agent/test/agent-org/seed", {
    id: ORG_ID,
    name: ORG_NAME,
    coordinator_agent_id: "builtin:sde",
    members: [
      {
        id: MEMBER_ID,
        name: "Pause Worker",
        role: "Own the exact shell lifecycle acceptance task",
        agent_id: "builtin:sde",
      },
    ],
  });
}

async function runPausePhase() {
  const account = await getApiAccount();
  const model = selectPreferredModel(account);
  await seedOneMemberOrg();
  await configureCreatorForAgentOrg({ account, model, agentOrgId: ORG_ID });
  await selectRenderedAgentOrg(ORG_ID);

  const backgroundCommand = [
    "trap '' TERM;",
    `sh -c 'trap \"\" TERM; while :; do sleep 120; done' & child=$!;`,
    `printf '${PROCESS_MARKER} parent=%s child=%s\\n' \"$$\" \"$child\";`,
    "wait",
  ].join(" ");
  const convergenceCommand = `sleep 20; printf '${FINALITY_MARKER} terminal\\n'`;
  const prompt = [
    `This is live acceptance round ${ROUND}.`,
    "Use task_graph_create exactly once to create exactly one Task assigned to member pause-worker.",
    `The Task subject must contain ${PROCESS_MARKER}.`,
    "Its description must instruct the Member to do exactly this:",
    `before Pause, call run_shell exactly once with mode=background and command: ${backgroundCommand}`,
    "If that command is interrupted by Pause, the Resume continuation must not repeat it.",
    `On Resume, call run_shell exactly once with mode=background and command: ${convergenceCommand}`,
    "Immediately after launching that Resume-only background command, try task_update operation=complete and try to give the final answer without calling await_output first.",
    `If the system blocks early completion, consume the terminal background result in this same Turn, retry task_update operation=complete once, and include ${FINALITY_MARKER} in the final answer.`,
    "Do not run either command as coordinator and do not create another Task.",
  ].join(" ");
  const sessionId = await sendFromRenderedCreator(prompt);
  if (!sessionId) throw new Error("live Pause launch produced no root Session");

  const view = await waitForAgentOrgRunView(
    sessionId,
    (candidate) =>
      candidate?.runStatus === "running" &&
      candidate?.tasks?.length === 1 &&
      candidate.tasks[0]?.owner === MEMBER_ID,
    "real Provider created one assigned Pause Task",
    REPLY_TIMEOUT_MS * 2
  );
  const runId = view.context.runId;
  let runningEvidence = null;
  let targetShell = null;
  await browser.waitUntil(
    async () => {
      runningEvidence = await postJson("/agent/test/agent-org/pause/evidence", {
        org_run_id: runId,
      });
      targetShell = runningEvidence.background_shells?.find(
        (shell) =>
          shell.session_id !== sessionId &&
          String(shell.command).includes(PROCESS_MARKER)
      );
      return (
        Boolean(targetShell) &&
        runningEvidence.active_turns?.some(
          (turn) => turn.session_id === targetShell.session_id
        ) &&
        processGroupSnapshot(targetShell.pid).length >= 3
      );
    },
    {
      timeout: REPLY_TIMEOUT_MS * 2,
      interval: 250,
      timeoutMsg: `real Provider Member never started the owned parent/child process group: ${JSON.stringify(runningEvidence)}`,
    }
  );
  const processRows = processGroupSnapshot(targetShell.pid);
  const knownPids = processRows.map((row) => row.pid);
  const taskIdsBeforePause = runningEvidence.durable.tasks.map(
    (task) => task.id
  );
  const replayFilesBeforePause = filesContainingMarker(
    join(ORGII_HOME, "shell-replays"),
    PROCESS_MARKER
  );
  if (replayFilesBeforePause.length !== 1) {
    throw new Error(
      `live background command did not start exactly once: ${JSON.stringify(replayFilesBeforePause)}`
    );
  }

  await openAgentOrgOverviewPanel("real Provider Pause control");
  const pauseButton = await visibleProductButton(
    '[data-testid="agent-org-overview-pause-button"]',
    "data-e2e-live-pause"
  );
  const pauseStartedAt = Date.now();
  await pauseButton.click();

  let drained = null;
  await browser.waitUntil(
    async () => {
      drained = await postJson("/agent/test/agent-org/pause/evidence", {
        org_run_id: runId,
      });
      return (
        drained.durable.run_status === "paused" &&
        drained.active_runtime_count === 0 &&
        drained.active_turns.length === 0 &&
        drained.background_shells.length === 0 &&
        drained.durable.handoffs.length > 0 &&
        drained.durable.handoffs.every((handoff) =>
          ["released", "runtime_absent"].includes(handoff.drain_status)
        )
      );
    },
    {
      timeout: 10_000,
      interval: 50,
      timeoutMsg: `real Provider Pause did not drain every owned runtime/process: ${JSON.stringify(drained)}`,
    }
  );
  const drainMs = Date.now() - pauseStartedAt;
  const memberHandoff = drained.durable.handoffs.find(
    (handoff) => handoff.session_id === targetShell.session_id
  );
  if (
    !memberHandoff?.runtime_lease_id ||
    !memberHandoff?.dialog_turn_generation ||
    memberHandoff.drain_status !== "released" ||
    memberHandoff.drain_timeout_at
  ) {
    throw new Error(
      `live Member handoff lacked exact released owner identity: ${JSON.stringify(memberHandoff)}`
    );
  }
  const survivors = processGroupSnapshot(targetShell.pid);
  const liveKnownPids = knownPids.filter(pidExists);
  if (survivors.length > 0 || liveKnownPids.length > 0) {
    throw new Error(
      `live Pause left parent/child processes alive: ${JSON.stringify({ survivors, liveKnownPids, processRows })}`
    );
  }
  if (
    drained.durable.tasks.length !== 1 ||
    drained.durable.tasks[0].id !== taskIdsBeforePause[0]
  ) {
    throw new Error(
      `live Pause duplicated or replaced its Task: ${JSON.stringify(drained)}`
    );
  }

  const renderedBeforeQuietWindow = await execJS(`
    return {
      assistants: document.querySelectorAll('[data-testid="chat-message-assistant"]').length,
      groupMessages: document.querySelectorAll('[data-testid="agent-org-group-projection-item"]').length,
    };
  `);
  await browser.pause(2_500);
  const quietEvidence = await postJson("/agent/test/agent-org/pause/evidence", {
    org_run_id: runId,
  });
  const renderedAfterQuietWindow = await execJS(`
    return {
      assistants: document.querySelectorAll('[data-testid="chat-message-assistant"]').length,
      groupMessages: document.querySelectorAll('[data-testid="agent-org-group-projection-item"]').length,
    };
  `);
  if (
    quietEvidence.active_runtime_count !== 0 ||
    quietEvidence.active_turns.length !== 0 ||
    quietEvidence.background_shells.length !== 0 ||
    JSON.stringify(renderedBeforeQuietWindow) !==
      JSON.stringify(renderedAfterQuietWindow)
  ) {
    throw new Error(
      `late Provider/shell activity appeared after Pause: ${JSON.stringify({ quietEvidence, renderedBeforeQuietWindow, renderedAfterQuietWindow })}`
    );
  }

  console.info(
    `[agent-org-live-pause-evidence] ${JSON.stringify({
      round: ROUND,
      provider: { accountId: account.id, accountName: account.name, model },
      runId,
      taskIds: taskIdsBeforePause,
      owner: {
        sessionId: memberHandoff.session_id,
        turnIntentId: memberHandoff.original_turn_intent_id,
        runtimeLeaseId: memberHandoff.runtime_lease_id,
        dialogTurnGeneration: memberHandoff.dialog_turn_generation,
      },
      processGroupId: targetShell.pid,
      processRows,
      drainMs,
      replayFiles: replayFilesBeforePause,
      activeRuntimeCountAfter: quietEvidence.active_runtime_count,
      activeTurnCountAfter: quietEvidence.active_turns.length,
    })}`
  );
}

async function runResumePhase() {
  const state = await waitForAgentOrgRunViewByOrg(
    ORG_ID,
    (view) => view?.runStatus === "paused",
    `persisted Paused run after real app restart ${ROUND}`
  );
  const sessionId = state.run.rootSessionId;
  const runId = state.view.context.runId;
  // Opening the persisted Session is restart pre-state only. Resume itself is
  // still driven through the rendered product control below. This avoids
  // coupling the lifecycle acceptance to unrelated sidebar pagination.
  unwrap(
    await invokeE2E("openSession", sessionId),
    "open persisted Paused root Session after real app restart"
  );
  await openAgentOrgOverviewPanel("real restart Resume control");

  const before = await postJson("/agent/test/agent-org/pause/evidence", {
    org_run_id: runId,
  });
  const taskIds = before.durable.tasks.map((task) => task.id);
  const replayFilesBefore = filesContainingMarker(
    join(ORGII_HOME, "shell-replays"),
    PROCESS_MARKER
  );
  if (
    before.durable.run_status !== "paused" ||
    before.active_runtime_count !== 0 ||
    before.active_turns.length !== 0 ||
    before.background_shells.length !== 0 ||
    before.durable.tasks.length !== 1 ||
    replayFilesBefore.length !== 1
  ) {
    throw new Error(
      `restart did not restore exact Paused state: ${JSON.stringify(before)}`
    );
  }

  const resumeButton = await visibleProductButton(
    '[data-testid="agent-org-overview-resume-button"]',
    "data-e2e-live-resume"
  );
  await resumeButton.click();
  let resumed = null;
  await browser.waitUntil(
    async () => {
      resumed = await postJson("/agent/test/agent-org/pause/evidence", {
        org_run_id: runId,
      });
      return (
        resumed.durable.run_status === "running" &&
        resumed.durable.episode?.status === "consumed" &&
        resumed.durable.handoffs.every(
          (handoff) => handoff.continuation_status === "dispatched"
        )
      );
    },
    {
      timeout: REPLY_TIMEOUT_MS,
      interval: 100,
      timeoutMsg: `restart Resume did not dispatch one durable continuation: ${JSON.stringify(resumed)}`,
    }
  );
  const continuationIds = resumed.durable.handoffs.map(
    (handoff) => handoff.continuation_turn_intent_id
  );
  if (
    continuationIds.some((id) => !id) ||
    new Set(continuationIds).size !== continuationIds.length ||
    resumed.durable.tasks.length !== 1 ||
    resumed.durable.tasks[0].id !== taskIds[0]
  ) {
    throw new Error(
      `restart Resume duplicated a continuation or Task: ${JSON.stringify(resumed)}`
    );
  }
  const memberHandoff = resumed.durable.handoffs.find(
    (handoff) => handoff.task_id === taskIds[0]
  );
  const memberContinuationId = memberHandoff?.continuation_turn_intent_id;
  if (!memberHandoff?.session_id || !memberContinuationId) {
    throw new Error(
      `Resume lacked exact Member continuation: ${JSON.stringify(resumed)}`
    );
  }
  const initialConvergence = convergenceSnapshot({
    runId,
    taskId: taskIds[0],
    memberSessionId: memberHandoff.session_id,
    continuationId: memberContinuationId,
  });

  let heldEvidence = null;
  let heldConvergence = null;
  let convergenceShell = null;
  await browser.waitUntil(
    async () => {
      heldEvidence = await postJson("/agent/test/agent-org/pause/evidence", {
        org_run_id: runId,
      });
      if (
        heldEvidence.background_shells.some((shell) =>
          String(shell.command).includes(PROCESS_MARKER)
        )
      ) {
        throw new Error(
          `Resume restarted the Pause command: ${JSON.stringify(heldEvidence)}`
        );
      }
      convergenceShell = heldEvidence.background_shells.find((shell) =>
        String(shell.command).includes(FINALITY_MARKER)
      );
      heldConvergence = convergenceSnapshot({
        runId,
        taskId: taskIds[0],
        memberSessionId: memberHandoff.session_id,
        continuationId: memberContinuationId,
      });
      return (
        Boolean(convergenceShell) &&
        heldConvergence?.continuation_status === "running" &&
        heldConvergence?.task_status === "in_progress" &&
        heldEvidence.active_turns.some(
          (turn) => turn.session_id === memberHandoff.session_id
        )
      );
    },
    {
      timeout: REPLY_TIMEOUT_MS,
      interval: 100,
      timeoutMsg: `same-Turn finality gate never held the resumed Member while its job was running: ${JSON.stringify({ heldEvidence, heldConvergence })}`,
    }
  );

  let convergedEvidence = null;
  let converged = null;
  await browser.waitUntil(
    async () => {
      convergedEvidence = await postJson(
        "/agent/test/agent-org/pause/evidence",
        {
          org_run_id: runId,
        }
      );
      converged = convergenceSnapshot({
        runId,
        taskId: taskIds[0],
        memberSessionId: memberHandoff.session_id,
        continuationId: memberContinuationId,
      });
      return (
        convergedEvidence.durable.run_status === "idle" &&
        convergedEvidence.active_turns.length === 0 &&
        convergedEvidence.background_shells.length === 0 &&
        converged?.run_status === "idle" &&
        converged?.task_status === "completed" &&
        converged?.continuation_status === "completed" &&
        converged?.active_intent_count === 0 &&
        converged?.unresolved_assignment_count === 0 &&
        converged?.assignment_materialization_count === 0 &&
        converged?.finality_assistant_count >= 1
      );
    },
    {
      timeout: REPLY_TIMEOUT_MS * 2,
      interval: 250,
      timeoutMsg: `Resume did not converge Task, Turn, Inbox, assistant, job, and Run to terminal/Idle: ${JSON.stringify({ convergedEvidence, converged })}`,
    }
  );
  const replayFilesAfter = filesContainingMarker(
    join(ORGII_HOME, "shell-replays"),
    PROCESS_MARKER
  );
  if (
    replayFilesAfter.length !== 1 ||
    replayFilesAfter[0] !== replayFilesBefore[0]
  ) {
    throw new Error(
      `Resume created another command replay: ${JSON.stringify({ replayFilesBefore, replayFilesAfter })}`
    );
  }
  const finalityReplayFiles = filesContainingMarker(
    join(ORGII_HOME, "shell-replays"),
    FINALITY_MARKER
  );
  if (finalityReplayFiles.length !== 1) {
    throw new Error(
      `Resume finality command did not execute exactly once: ${JSON.stringify(finalityReplayFiles)}`
    );
  }

  const quietBefore = {
    convergence: converged,
    taskUpdatedAt: convergedEvidence.durable.tasks[0]?.updated_at,
    replayFilesAfter,
    finalityReplayFiles,
  };
  await browser.pause(5_000);
  const quietEvidence = await postJson("/agent/test/agent-org/pause/evidence", {
    org_run_id: runId,
  });
  const quietConvergence = convergenceSnapshot({
    runId,
    taskId: taskIds[0],
    memberSessionId: memberHandoff.session_id,
    continuationId: memberContinuationId,
  });
  if (
    quietEvidence.durable.run_status !== "idle" ||
    quietEvidence.active_turns.length !== 0 ||
    quietEvidence.background_shells.length !== 0 ||
    quietConvergence.intent_count !== converged.intent_count ||
    quietConvergence.context_count !== converged.context_count ||
    quietConvergence.assistant_count !== converged.assistant_count ||
    quietEvidence.durable.tasks[0]?.updated_at !== quietBefore.taskUpdatedAt ||
    filesContainingMarker(join(ORGII_HOME, "shell-replays"), PROCESS_MARKER)
      .length !== 1 ||
    filesContainingMarker(join(ORGII_HOME, "shell-replays"), FINALITY_MARKER)
      .length !== 1
  ) {
    throw new Error(
      `Resume quiet window detected a duplicate Wake, Provider Turn, Task mutation, message, or process: ${JSON.stringify({ quietBefore, quietEvidence, quietConvergence })}`
    );
  }
  console.info(
    `[agent-org-live-resume-evidence] ${JSON.stringify({
      round: ROUND,
      runId,
      taskIds,
      continuationIds,
      memberContinuationId,
      initialConvergence,
      heldConvergence,
      convergenceShell,
      converged,
      quietConvergence,
      replayFilesBefore,
      replayFilesAfter,
      finalityReplayFiles,
    })}`
  );
}

async function runTaskSmokePhase() {
  const account = await getApiAccount();
  const model = selectPreferredModel(account);
  await seedOneMemberOrg();
  await configureCreatorForAgentOrg({ account, model, agentOrgId: ORG_ID });
  await selectRenderedAgentOrg(ORG_ID);
  const marker = `TASK_LIFECYCLE_SMOKE_${ROUND}`;
  const sessionId = await sendFromRenderedCreator(
    [
      `Use task_graph_create exactly once to create one Task assigned to ${MEMBER_ID}.`,
      `The subject must be ${marker}.`,
      "The Member must inspect README.md, mark the Task in progress, then mark it completed and reply briefly.",
      "Do not create any other Task.",
    ].join(" ")
  );
  let runId = "";
  await browser.waitUntil(
    async () => {
      runId = durableRunIdForRootSession(sessionId);
      return Boolean(runId);
    },
    {
      timeout: REPLY_TIMEOUT_MS,
      interval: 250,
      timeoutMsg: `real Provider Task lifecycle run never became durable for Session ${sessionId}`,
    }
  );
  let evidence = null;
  await browser.waitUntil(
    async () => {
      evidence = await postJson("/agent/test/agent-org/pause/evidence", {
        org_run_id: runId,
      });
      return (
        evidence.durable.tasks.length === 1 &&
        evidence.durable.tasks[0]?.status === "completed"
      );
    },
    {
      timeout: REPLY_TIMEOUT_MS * 2,
      interval: 500,
      timeoutMsg: `real Provider Task never completed durably: ${JSON.stringify(evidence)}`,
    }
  );
  console.info(
    `[agent-org-live-task-lifecycle-smoke] ${JSON.stringify({
      round: ROUND,
      provider: { accountId: account.id, accountName: account.name, model },
      runId,
      task: evidence.durable.tasks[0],
    })}`
  );
}

describe("Agent Org lifecycle live Provider process ownership", function () {
  before(async () => {
    assertLiveInputs();
    await waitForApp();
    await assertFeatureGatePreflight();
  });

  it(`runs live acceptance phase ${PHASE || "missing"} for ${ROUND || "missing"}`, async function () {
    this.timeout(900_000);
    if (PHASE === "pause") return runPausePhase();
    if (PHASE === "resume") return runResumePhase();
    if (PHASE === "archive") return runArchivePhase();
    return runTaskSmokePhase();
  });
});
