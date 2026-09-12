/**
 * Agent Session API
 *
 * Session lifecycle, persistence, interaction, and message processing.
 */
import { invoke } from "@tauri-apps/api/core";

import { rpc } from "@src/api/tauri/rpc";
import type { CliAgentType, NativeHarnessType } from "@src/api/types/keys";
import { requireAgentOrgEnabled } from "@src/config/agentOrgAvailability";
import type { OrgMemberLaunchOverride } from "@src/modules/MainApp/AgentOrgs/types";
import type { WorkspaceSnapshot } from "@src/services/context/workspaceSnapshot";
import type { SessionStatus } from "@src/types/session/session";
import { isCliSession } from "@src/util/session/sessionDispatch";

import type {
  AgentStatusInfo,
  DeleteSessionReceipt,
  FileResolution,
  FileResolutionValue,
  HousekeeperContextCompactionState,
  ManualCompactResult,
  ModeSwitchChoice,
  PendingQuestion,
  PermissionResponseValue,
  PlanApprovalChoice,
  RevertResult,
  SessionFileRecord,
  SessionInfo,
  SessionMessage,
  SessionMeta,
  SnapshotRecord,
  TodoItem,
} from "./types";

export async function listSessions(): Promise<string[]> {
  return rpc.agentSession.listSessions();
}

export async function getSessionInfo(
  sessionId: string
): Promise<SessionInfo | null> {
  return rpc.agentSession.getSessionInfo({ sessionId });
}

export async function manualCompactSession(
  sessionId: string,
  instructions?: string
): Promise<ManualCompactResult> {
  return rpc.agentSession.manualCompact({
    sessionId,
    ...(instructions ? { instructions } : {}),
  });
}

export async function getHousekeeperContextCompactionStatus(
  sessionId: string
): Promise<HousekeeperContextCompactionState> {
  return rpc.agentSession.housekeeperContextCompactionStatus({ sessionId });
}

export async function setHousekeeperContextCompactionEnabled(
  sessionId: string,
  enabled: boolean
): Promise<HousekeeperContextCompactionState> {
  return rpc.agentSession.setHousekeeperContextCompactionEnabled({
    sessionId,
    enabled,
  });
}

export async function compactHousekeeperContextNow(
  sessionId: string
): Promise<HousekeeperContextCompactionState> {
  return rpc.agentSession.compactHousekeeperContextNow({ sessionId });
}

/** Cancel the active turn for a session using an explicit control-flow reason. */
export const CANCEL_REASON = {
  USER_STOP: "user_stop",
  FORCE_SEND: "force_send",
  ORG_PAUSE: "org_pause",
  PROGRAMMATIC_SHUTDOWN: "programmatic_shutdown",
  SESSION_EVICTION: "session_eviction",
  MODE_SWITCH_ABORT: "mode_switch_abort",
} as const;

export type CancelReason = (typeof CANCEL_REASON)[keyof typeof CANCEL_REASON];

export async function cancelSession(
  sessionId: string,
  reason: CancelReason = CANCEL_REASON.USER_STOP
): Promise<boolean> {
  return rpc.agentSession.cancelSession({
    sessionId,
    reason,
  });
}

export async function removeSession(sessionId: string): Promise<void> {
  return rpc.agentSession.removeSession({ sessionId });
}

export async function isAgentRunning(): Promise<boolean> {
  return rpc.agentSession.isAgentRunning();
}

export async function loadMessages(
  sessionId: string
): Promise<SessionMessage[]> {
  return rpc.agentSession.loadMessages({ sessionId });
}

export async function getSession(
  sessionId: string
): Promise<SessionMeta | null> {
  return rpc.agentSession.getSession({ sessionId });
}

export async function listAllSessions(): Promise<SessionMeta[]> {
  return rpc.agentSession.listAllSessions();
}

export async function deleteSession(
  sessionId: string
): Promise<DeleteSessionReceipt> {
  return rpc.agentSession.deleteSession({ sessionId });
}

export async function clearMessages(sessionId: string): Promise<void> {
  await rpc.agentSession.clearMessages({ sessionId });
}

/**
 * Truncate messages at or after `createdAt`.
 *
 * When `revertFiles` is true (default), the per-session file-history is
 * rewound so edited files are restored to their pre-turn bytes. Pass `false`
 * to keep current file contents (e.g. "continue with my changes" flow).
 */
export async function truncateAfterMessage(
  sessionId: string,
  createdAt: string,
  options?: { revertFiles?: boolean; messageId?: string }
): Promise<number> {
  return rpc.agentSession.truncateAfterMessage({
    sessionId,
    createdAt,
    revertFiles: options?.revertFiles ?? true,
    messageId: options?.messageId,
  });
}

/**
 * Returns true if rewinding to `createdAt` would actually modify files on
 * disk. Used by the edit/regenerate flow to decide whether to prompt the user
 * with a "keep or revert changes" dialog.
 */
export async function checkSnapshotChanges(
  sessionId: string,
  createdAt: string
): Promise<boolean> {
  return rpc.agentSession.checkSnapshotChanges({
    sessionId,
    createdAt,
  });
}

export async function updateSessionStatus(
  sessionId: string,
  status: SessionStatus
): Promise<boolean> {
  return rpc.agentSession.updateSessionStatus({
    sessionId,
    status,
  });
}

export async function saveSession(session: SessionMeta): Promise<void> {
  return rpc.agentSession.saveSession({ session });
}

export async function linkSessionToWorkItem(input: {
  sessionId: string;
  projectSlug: string;
  workItemId: string;
  agentRole?: string;
}): Promise<SessionMeta> {
  return rpc.agentSession.linkSessionToWorkItem(input);
}

/** Track this / Convert to Project (orgtrack/v1 §7.2): switch the
 *  session to the Project product mode, invalidate any pending Plan
 *  snapshot, and create-or-replay the root WorkItem from the recorded
 *  first user input. */
export async function trackSessionAsProject(sessionId: string): Promise<{
  productMode: string;
  agentExecMode: string;
  workItemId?: string | null;
}> {
  return rpc.agentSession.trackSessionAsProject({ sessionId });
}

export async function respondQuestion(
  sessionId: string,
  requestId: string,
  answers: string[][]
): Promise<void> {
  if (isCliSession(sessionId) || requestId.startsWith("native-interaction-")) {
    const handled = await invoke<boolean>("cli_native_question_response", {
      sessionId,
      requestId,
      answers,
    });
    if (handled) return;
  }
  return rpc.agentSession.respondQuestion({
    sessionId,
    requestId,
    answers,
  });
}

export async function rejectQuestion(
  sessionId: string,
  requestId: string
): Promise<void> {
  if (isCliSession(sessionId) || requestId.startsWith("native-interaction-")) {
    const handled = await invoke<boolean>("cli_native_question_response", {
      sessionId,
      requestId,
      answers: null,
    });
    if (handled) return;
  }
  return rpc.agentSession.rejectQuestion({ sessionId, requestId });
}

export async function respondPermission(
  sessionId: string,
  requestId: string,
  response: PermissionResponseValue,
  toolName?: string,
  toolArgs?: Record<string, unknown>
): Promise<void> {
  return rpc.agentSession.respondPermission({
    sessionId,
    requestId,
    response,
    toolName,
    toolArgs,
  });
}

/**
 * Resolve a permission request parked on the CLI side: a managed
 * session's PermissionRequest hook (`origin: "cli_hook"`) or an ACP
 * agent's `session/request_permission` (`origin: "acp"`) from the
 * session-scoped permission request store. Routes to
 * `cli_agent_approval_response`, which checks the hook registry first
 * and falls back to the ACP registry by `requestId`. For hooks,
 * `always_allow` is treated as a plain allow — persistent rules stay
 * with the CLI's own permission store; ACP agents receive their
 * `allow_always` option when offered.
 */
export async function respondCliHookPermission(
  sessionId: string,
  requestId: string,
  response: PermissionResponseValue
): Promise<void> {
  return invoke("cli_agent_approval_response", {
    sessionId,
    requestId,
    approved: response === "allow" || response === "always_allow",
    alwaysAllow: response === "always_allow",
  });
}

export async function getPendingQuestions(
  sessionId: string
): Promise<{ pendingQuestions: PendingQuestion[] }> {
  return rpc.agentSession.getPendingQuestions({ sessionId });
}

export async function respondModeSwitch(
  sessionId: string,
  choice: ModeSwitchChoice,
  targetMode?: string
): Promise<void> {
  return rpc.agentSession.respondModeSwitch({
    sessionId,
    choice,
    targetMode,
  });
}

/**
 * Query the pending plan approval snapshot for a session.
 * Returns the snapshot when a plan is awaiting Build, null otherwise.
 * Used on session mount/switch to rehydrate `pendingPlanApprovalsAtom`
 * after a page refresh or window re-focus.
 */
export interface PendingPlanApprovalSnapshot {
  sessionId: string;
  planPath: string;
  planTitle: string;
  planContent: string;
  toolCallId?: string;
  planId?: string;
  planRevisionId?: string;
  originToolCallId?: string;
  autoApproveAt?: number | null;
}

export async function getPendingPlanApproval(
  sessionId: string
): Promise<PendingPlanApprovalSnapshot | null> {
  return rpc.agentSession.getPendingPlanApproval({ sessionId });
}

export async function updatePendingPlanContent(
  sessionId: string,
  content: string,
  planRevisionId?: string
): Promise<PendingPlanApprovalSnapshot> {
  return rpc.agentSession.updatePendingPlanContent({
    sessionId,
    content,
    planRevisionId,
  });
}

/**
 * Build-button response from the plan card. Triggers the backend to:
 *  - consume the pending plan snapshot
 *  - (optional) overwrite the plan file with `editedContent`
 *  - flip `AgentExecMode` back to the pre-plan mode
 *  - broadcast `agent:exit_plan_mode`
 *
 * A reject response consumes the pending plan without starting the Build turn.
 */
export interface PlanApprovalIdentity {
  /** Model id from the composer's current selection (own-key: `model`, hosted-key: `listingModel`). */
  model?: string;
  /** Account id for own-key sources; omitted for hosted-key. */
  accountId?: string;
  /** Active repo path; falls back to runtime/DB when omitted. */
  workspacePath?: string;
}

export async function respondPlanApproval(
  sessionId: string,
  choice: PlanApprovalChoice,
  editedContent?: string,
  identity?: PlanApprovalIdentity
): Promise<void> {
  return rpc.agentSession.respondPlanApproval({
    sessionId,
    choice,
    editedContent,
    model: identity?.model ?? null,
    accountId: identity?.accountId ?? null,
    workspacePath: identity?.workspacePath ?? null,
  });
}

export async function getSessionFiles(
  sessionId: string
): Promise<SessionFileRecord[]> {
  return rpc.agentSession.getSessionFiles({ sessionId });
}

export async function getSessionWorkspacePath(
  sessionId: string
): Promise<string | null> {
  return rpc.agentSession.getSessionWorkspacePath({ sessionId });
}

export async function getSnapshots(
  sessionId: string
): Promise<SnapshotRecord[]> {
  return rpc.agentSession.getSnapshots({ sessionId });
}

/**
 * Revert ALL file-history snapshots taken at or after `createdAt` for the
 * session. This undoes every agent edit in the current review round, not
 * just the first snapshot.
 */
export async function revertToSnapshot(
  sessionId: string,
  createdAt: string
): Promise<RevertResult> {
  return rpc.agentSession.revertToSnapshot({
    createdAt,
    sessionId,
  });
}

export async function restoreSnapshot(
  sessionId: string,
  snapshotId: string
): Promise<RevertResult> {
  return rpc.agentSession.restoreSnapshot({
    sessionId,
    snapshotId,
  });
}

export async function revertFileReview(
  sessionId: string,
  createdAt: string,
  filePath: string,
  workspacePath?: string
): Promise<boolean> {
  return rpc.agentSession.revertFileReview({
    workspacePath: workspacePath ?? "",
    filePath,
    sessionId,
    createdAt,
  });
}

/**
 * Revert a single file within a snapshot back to its captured bytes.
 * `sessionId` is required. `workspacePath` is used only to resolve relative
 * `filePath` inputs; if `filePath` is already absolute, `workspacePath` can be
 * empty.
 */
export async function revertFile(
  sessionId: string,
  snapshotHash: string,
  filePath: string,
  workspacePath?: string
): Promise<boolean> {
  return rpc.agentSession.revertFile({
    workspacePath: workspacePath ?? "",
    snapshotHash,
    filePath,
    sessionId,
  });
}

export async function getTodos(sessionId: string): Promise<TodoItem[]> {
  return rpc.agentSession.getTodos({ sessionId });
}

export async function resolveReview(sessionId: string): Promise<number> {
  return rpc.agentSession.resolveReview({ sessionId });
}

export async function saveFileResolution(
  sessionId: string,
  filePath: string,
  resolution: FileResolutionValue
): Promise<void> {
  return rpc.agentSession.saveFileResolution({
    sessionId,
    filePath,
    resolution,
  });
}

export async function getFileResolutions(
  sessionId: string
): Promise<FileResolution[]> {
  return rpc.agentSession.getFileResolutions({
    sessionId,
  });
}

export async function getAgentStatus(): Promise<AgentStatusInfo> {
  return rpc.agentSession.getAgentStatus();
}

// ============================================
// Unified Session Launch
// ============================================

export interface SessionLaunchParams {
  category: "rust_agent" | "cli_agent";
  content: string;
  workspacePath?: string;
  keySource?: string;
  accountId?: string;
  model?: string;
  nativeHarnessType?: NativeHarnessType;
  platform?: CliAgentType;
  branch?: string;
  /** Git base ref used only when creating a fresh isolated worktree. */
  worktreeBaseRef?: string;
  hostedToken?: string;
  tier?: string;
  name?: string;
  background?: boolean;
  images?: string[];
  ideContext?: WorkspaceSnapshot;
  agentDefinitionId?: string;
  agentOrgId?: string;
  agentOrgMemberOverrides?: Record<string, OrgMemberLaunchOverride>;
  applyAgentOrgMemberOverridesForFuture?: boolean;
  isolate?: boolean;
  mode?: string;
  orgId?: string;
  projectId?: string;
  projectName?: string;
  workItemId?: string;
  agentRole?: string;
  worktreePath?: string;
  projectSlug?: string;
  parentSessionId?: string;

  /**
   * Extra workspace folders granted at launch time (multi-root IDE
   * workspaces). The primary folder is passed via `workspacePath`; this
   * list must only contain the *other* roots. Backend injects each
   * entry into `SessionWorkspace.additional_directories` with
   * `Session` scope before the first turn runs.
   *
   * Omit or pass `[]` for single-repo launches.
   */
  additionalDirectories?: string[];
}

export interface SessionLaunchResult {
  sessionId: string;
  category: string;
  name: string;
  status: string;
  createdAt: string;
  userInput: string;
  workspacePath?: string | null;
  branch?: string | null;
  background: boolean;
  model?: string | null;
  cliAgentType?: CliAgentType | null;
  accountId?: string | null;
  agentOrgId?: string | null;
  agentOrgRunId?: string | null;
  orgId?: string | null;
  projectId?: string | null;
  projectName?: string | null;
  projectSlug?: string | null;
  workItemId?: string | null;
  agentRole?: string | null;
  productMode?: string | null;
  worktreePath?: string | null;
  /** Actual branch checked out in the worktree (for example `agent/<id>`). */
  worktreeBranch?: string | null;
  /** Base ref used to create the isolated worktree. */
  baseRef?: string | null;
}

export async function sessionLaunch(
  params: SessionLaunchParams
): Promise<SessionLaunchResult> {
  if (params.agentOrgId) requireAgentOrgEnabled();
  return rpc.agentSession.sessionLaunch({
    params,
  }) as Promise<SessionLaunchResult>;
}

// ============================================
// Wingman Mode
// ============================================

/**
 * Start the Wingman observation loop for an existing session.
 *
 * `mission` is the user's stated goal, e.g.:
 * "Watch me implement the auth flow and tell me if I'm doing anything wrong."
 */
export async function wingmanStart(
  sessionId: string,
  mission: string,
  monitorIndex?: number
): Promise<void> {
  return rpc.agentSession.wingmanStart({
    sessionId,
    mission,
    monitorIndex,
  });
}

/**
 * Stop the Wingman observation loop. No-op if not currently running.
 */
export async function wingmanStop(sessionId: string): Promise<void> {
  return rpc.agentSession.wingmanStop({ sessionId });
}

export async function showDesktopOperationVisibilityTest(
  monitorIndex?: number
): Promise<void> {
  return rpc.agentSession.wingmanShowDesktopControlTest({ monitorIndex });
}

/**
 * Close Wingman UI surfaces. Fire-and-forget; used when the user clicks Stop / Close.
 */
export async function wingmanCloseWindows(): Promise<void> {
  return rpc.agentSession.wingmanCloseWindows();
}

/**
 * Describes a single connected display returned by {@link wingmanListMonitors}.
 *
 * All coordinates/sizes are in logical pixels; `workX/Y/Width/Height` is the
 * visible rect minus the menu bar and Dock (equivalent to macOS visibleFrame).
 */
export interface WingmanMonitor {
  index: number;
  name: string;
  x: number;
  y: number;
  width: number;
  height: number;
  workX: number;
  workY: number;
  workWidth: number;
  workHeight: number;
  scaleFactor: number;
  isPrimary: boolean;
}

/**
 * Enumerate connected displays so the UI can prompt the user to pick one.
 * The returned `index` is what you pass to Wingman commands.
 */
export async function wingmanListMonitors(): Promise<WingmanMonitor[]> {
  return rpc.agentSession.wingmanListMonitors();
}

export interface AdeActionResultPayload {
  success: boolean;
  message: string;
  data?: unknown;
}

function normalizeAdeActionResult(
  result: AdeActionResultPayload | null | undefined
): AdeActionResultPayload {
  if (!result) {
    return {
      success: false,
      message: "ADE action returned an empty result",
    };
  }

  return result;
}

export async function sendAdeActionResult(
  correlationId: string,
  result: AdeActionResultPayload | null | undefined
): Promise<void> {
  const normalizedResult = normalizeAdeActionResult(result);

  return rpc.agentSession.sendAdeActionResult({
    correlationId,
    success: normalizedResult.success,
    message: normalizedResult.message,
    ...(typeof normalizedResult.data === "undefined"
      ? {}
      : { data: normalizedResult.data }),
  });
}
