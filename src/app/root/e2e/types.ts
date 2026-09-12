import type { useStore } from "jotai";

import type { PromptDumpResult } from "@src/api/tauri/agent/promptDump";
import type {
  CliAgentType,
  KeyInfo,
  ModelType,
  NativeHarnessType,
} from "@src/api/tauri/rpc/schemas/validation";
import type { DispatchCategory } from "@src/api/tauri/session";
import type { AgentExecMode } from "@src/config/sessionCreatorConfig";
import type {
  CustomRoleDefinition,
  UserPresenceState,
  UserPresenceWire,
} from "@src/types/userPresence";

export interface AddAccountOptions {
  openaiApiKey: string;
  model: string;
  baseUrl?: string;
  accountName?: string;
  agentDefinitionId?: string;
  agentOrgId?: string;
  repoPath?: string;
}

export interface AddCursorNativeAccountOptions {
  apiKey?: string;
  sessionToken: string;
  accountName?: string;
  availableModels?: string[];
  enabledModels?: string[];
}

export interface AddClaudeCodeAccountOptions {
  sessionToken: string;
  refreshToken?: string;
  accountName?: string;
  availableModels?: string[];
  enabledModels?: string[];
}

export interface AddCodexAccountOptions {
  sessionToken: string;
  refreshToken?: string;
  idToken?: string;
  accountName?: string;
  availableModels?: string[];
  enabledModels?: string[];
}

export interface CloneCursorNativeAccountWithoutApiKeyOptions {
  sourceAccountName: string;
  targetAccountName: string;
}

export interface PinSessionOptions {
  accountId: string;
  model: string;
  accountName?: string;
  modelType?: ModelType;
  agentDefinitionId?: string;
  agentOrgId?: string;
  category?: DispatchCategory;
  cliAgentType?: CliAgentType;
  nativeHarnessType?: NativeHarnessType;
  agentExecMode?: AgentExecMode;
  repoPath?: string;
}

export interface EnsureRepoSelectedOptions {
  repoPath?: string;
  repoName?: string;
}

export interface SeedMultiRootWorkspaceOptions {
  workspaceId?: string;
  workspaceName?: string;
  folders: Array<{
    id?: string;
    name: string;
    path: string;
    isPrimary?: boolean;
  }>;
}

export interface ConfigureExistingOptions {
  accountName: string;
  model?: string;
  agentType?: ModelType;
  agentDefinitionId?: string;
  agentOrgId?: string;
  category?: DispatchCategory;
  cliAgentType?: CliAgentType;
  nativeHarnessType?: NativeHarnessType;
  agentExecMode?: AgentExecMode;
  repoPath?: string;
}

type Ok<T> = { ok: true } & T;
export type Err = { ok: false; error: string };
export type Result<T> = Ok<T> | Err;
export type Json = Record<string, unknown>;
export type E2EStore = ReturnType<typeof useStore>;

export interface E2EHelpers {
  addAccount: (
    opts: AddAccountOptions
  ) => Promise<Result<{ account: KeyInfo }>>;
  addCursorNativeAccount: (
    opts: AddCursorNativeAccountOptions
  ) => Promise<Result<{ account: KeyInfo }>>;
  addClaudeCodeAccount: (
    opts: AddClaudeCodeAccountOptions
  ) => Promise<Result<{ account: KeyInfo }>>;
  addCodexAccount: (
    opts: AddCodexAccountOptions
  ) => Promise<Result<{ account: KeyInfo }>>;
  cloneCursorNativeAccountWithoutApiKey: (
    opts: CloneCursorNativeAccountWithoutApiKeyOptions
  ) => Promise<Result<{ account: KeyInfo }>>;
  listAccounts: () => Promise<Result<{ accounts: KeyInfo[] }>>;
  inspectProviderMatrix: () => Promise<
    Result<{
      agents: unknown[];
      apiProviders: unknown[];
      providerConfigs: Record<string, unknown>;
    }>
  >;
  autoDetectKeyForE2E: (
    agentType: ModelType
  ) => Promise<Result<{ result: unknown }>>;
  removeAccount: (id: string) => Promise<{ ok: true } | Err>;
  createCliPatchSession: (opts: {
    cliAgentType: CliAgentType;
    model: string;
    accountId: string;
    workspacePath?: string;
    name?: string;
  }) => Promise<Result<{ sessionId: string }>>;
  patchSessionModel: (
    sessionId: string,
    model: string,
    accountId?: string
  ) => Promise<
    Result<{
      session: {
        sessionId: string;
        category: string;
        model?: string;
        accountId?: string;
        cliAgentType?: string;
      };
    }>
  >;
  pinSession: (opts: PinSessionOptions) => Promise<Result<{ repoId: string }>>;
  configure: (
    opts: AddAccountOptions
  ) => Promise<Result<{ accountId: string; modelId: string; repoId: string }>>;
  configureWithExistingKey: (
    opts: ConfigureExistingOptions
  ) => Promise<Result<{ accountId: string; modelId: string; repoId: string }>>;
  inspectCreatorSelection: () => Promise<
    Result<{ creator: Json; modelSelection: Json | null }>
  >;
  setAgentOrgMemberDraftConfig: (
    config: Json,
    orgId?: string | null
  ) => Promise<Result<{ config: Json }>>;
  getAgentDef: (agentId: string) => Promise<Result<{ def: Json }>>;
  updateAgentDefPatch: (
    agentId: string,
    patch: Json
  ) => Promise<{ ok: true } | Err>;
  addAgentDef: (definition: Json) => Promise<Result<{ agentId: string }>>;
  updateAgentDef: (definition: Json) => Promise<{ ok: true } | Err>;
  resetAgentDefBuiltin: (agentId: string) => Promise<Result<{ def: Json }>>;
  removeAgentDef: (agentId: string) => Promise<Result<{ removed: boolean }>>;
  listAgentDefs: () => Promise<Result<{ defs: Json[] }>>;
  refreshAgentDefs: () => Promise<Result<{ defs: Json[] }>>;
  listAllTools: () => Promise<Result<{ tools: Json[] }>>;
  getIntegrations: () => Promise<Result<{ integrations: Json }>>;
  updateIntegrationsPatch: (patch: Json) => Promise<{ ok: true } | Err>;
  getAgentConfigBlob: (
    agentType: "os" | "sde"
  ) => Promise<Result<{ blob: Json }>>;
  updateAgentConfigBlob: (
    agentType: "os" | "sde",
    update: Json
  ) => Promise<{ ok: true } | Err>;
  readSettings: () => Promise<Result<{ settings: Json }>>;
  writeSettingsPartial: (partial: Json) => Promise<{ ok: true } | Err>;
  getSettingsRegistryKeys: () => Promise<Result<{ keys: string[] }>>;
  getOrgiiRoot: () => Promise<Result<{ path: string }>>;
  getSelectedRepoPath: () => Promise<Result<{ path: string }>>;
  ensureRepoSelected: (
    opts?: EnsureRepoSelectedOptions
  ) => Promise<Result<{ repoId: string; path: string }>>;
  seedMultiRootWorkspace: (opts: SeedMultiRootWorkspaceOptions) => Promise<
    Result<{
      workspaceId: string;
      primaryPath: string;
      additionalDirectories: string[];
    }>
  >;
  clearWorkspaceRepos: () => Promise<Result<{ cleared: true }>>;
  setActiveWorkspaceFolderForTest: (folderId: string | null) => Promise<
    Result<{
      primaryFolder: Json | null;
      activeFolder: Json | null;
      folders: Json[];
      selectedRepoId: string;
      repoPath: string;
    }>
  >;
  readSessionWorkspaceFromDb: (
    sessionId: string
  ) => Promise<Result<{ result: Json }>>;
  getGitStatusForPath: (
    repoPath: string
  ) => Promise<Result<{ exists: boolean; httpStatus: number; raw: Json }>>;
  pinFolderWorkspace: (
    folderPath: string,
    folderName?: string
  ) => Promise<Result<{ folderId: string; path: string }>>;
  readSessionPromptEnvironmentBlock: (
    sessionId: string
  ) => Promise<Result<{ result: Json }>>;
  readSdeTranscript: (sessionId: string) => Promise<Result<{ result: Json }>>;
  writeProject: (
    slug: string,
    meta: Json,
    description: string,
    expectNew?: boolean
  ) => Promise<{ ok: true } | Err>;
  deleteProject: (slug: string) => Promise<{ ok: true } | Err>;
  listRoutines: () => Promise<Result<{ routines: Json[] }>>;
  upsertRoutine: (routine: Json) => Promise<Result<{ routine: Json }>>;
  deleteRoutine: (routineId: string) => Promise<Result<{ removed: boolean }>>;
  fireRoutine: (routineId: string) => Promise<Result<{ result: Json }>>;
  listRoutineFires: (routineId: string) => Promise<Result<{ fires: Json[] }>>;
  projects: {
    listRoutines: () => Promise<Result<{ routines: Json[] }>>;
    upsertRoutine: (routine: Json) => Promise<Result<{ routine: Json }>>;
    deleteRoutine: (routineId: string) => Promise<Result<{ removed: boolean }>>;
    fireRoutine: (routineId: string) => Promise<Result<{ result: Json }>>;
    listRoutineFires: (routineId: string) => Promise<Result<{ fires: Json[] }>>;
  };
  readWorkItem: (
    projectSlug: string,
    shortId: string
  ) => Promise<Result<{ item: Json }>>;
  writeWorkItem: (
    projectSlug: string,
    shortId: string,
    frontmatter: Json,
    body: string
  ) => Promise<{ ok: true } | Err>;
  allocateStandaloneWorkItemId: () => Promise<Result<{ shortId: string }>>;
  readStandaloneWorkItems: (
    orgId?: string
  ) => Promise<Result<{ items: Json[] }>>;
  readProjectOrgs: () => Promise<Result<{ orgs: Json[] }>>;
  readStandaloneWorkItem: (shortId: string) => Promise<Result<{ item: Json }>>;
  writeStandaloneWorkItem: (
    shortId: string,
    frontmatter: Json,
    body: string
  ) => Promise<{ ok: true } | Err>;
  deleteWorkItem: (
    projectSlug: string,
    shortId: string
  ) => Promise<{ ok: true } | Err>;
  updateWorkItemPartial: (
    projectSlug: string,
    shortId: string,
    updates: Json
  ) => Promise<Result<{ item: Json }>>;
  readWorkItemsEnriched: (
    projectSlug: string
  ) => Promise<Result<{ items: Json[] }>>;
  testWorkItemScheduleLookup: (
    projectName: string,
    title: string
  ) => Promise<Result<Json>>;
  runWorkItemSchedulerOnce: () => Promise<Result<{ result: Json }>>;
  launchWorkItemRuntimeProbe: (
    params: Json
  ) => Promise<Result<{ result: Json }>>;
  listAgentOrgs: () => Promise<Result<{ orgs: Json[] }>>;
  removeAgentOrg: (orgId: string) => Promise<Result<{ removed: boolean }>>;
  debugAgentOrgExecuteToolAsAgent: (
    runId: string,
    senderMemberId: string,
    toolName: string,
    params: Json
  ) => Promise<Result<{ result: Json }>>;
  debugAgentOrgEmitMemberIdle: (
    runId: string,
    memberId: string,
    reason: string,
    failureReason?: string | null,
    currentMode?: string | null
  ) => Promise<Result<{ result: Json }>>;
  debugAgentOrgInboxList: (runId: string) => Promise<Result<{ rows: Json[] }>>;
  listAgentOrgSessionInbox: (
    sessionId: string
  ) => Promise<Result<{ rows: Json[] }>>;
  debugAgentOrgTasksList: (runId: string) => Promise<Result<{ tasks: Json[] }>>;
  agentOrgSessionRunView: (
    sessionId: string
  ) => Promise<Result<{ view: Json | null }>>;
  agentOrgGroupProjectionPage: (
    sessionId: string,
    cursor?: string | null,
    limit?: number
  ) => Promise<Result<{ page: Json }>>;
  agentOrgSessionInterventionState: (
    sessionId: string
  ) => Promise<Result<{ state: Json }>>;
  agentOrgPauseRun: (
    sessionId: string,
    requestId?: string
  ) => Promise<Result<{ outcome: Json }>>;
  agentOrgResumeRun: (
    sessionId: string,
    requestId?: string
  ) => Promise<Result<{ outcome: Json }>>;
  agentOrgSimulateAppRestart: () => Promise<
    Result<{
      intentsReconciled: number;
      terminalSessionsReconciled: number;
      sessionsAbandoned: number;
      tasksRequeued: number;
      runsCompleted: number;
      runsPaused: number;
      interventionsCleared: number;
    }>
  >;
  getDesktopConfig: () => Promise<Result<{ config: Json }>>;
  setDesktopConfig: (config: Json) => Promise<{ ok: true } | Err>;
  listPolicies: (
    workspacePath?: string
  ) => Promise<Result<{ policies: Json[] }>>;
  createPolicy: (opts: Json) => Promise<{ ok: true } | Err>;
  readPolicy: (
    name: string,
    source: string,
    workspacePath?: string
  ) => Promise<Result<{ content: string }>>;
  updatePolicy: (
    name: string,
    content: string,
    source: string,
    workspacePath?: string
  ) => Promise<{ ok: true } | Err>;
  setPolicyAgents: (
    name: string,
    source: string,
    agents: string[],
    workspacePath?: string
  ) => Promise<{ ok: true } | Err>;
  setPolicyScope: (
    name: string,
    source: string,
    scopeRepoPaths: string[] | null,
    scopeExcludeRepoPaths: string[] | null,
    workspacePath?: string
  ) => Promise<{ ok: true } | Err>;
  togglePolicy: (
    name: string,
    source: string,
    enabled: boolean,
    workspacePath?: string
  ) => Promise<{ ok: true } | Err>;
  deletePolicy: (
    name: string,
    source: string,
    workspacePath?: string
  ) => Promise<{ ok: true } | Err>;
  listWorkspaceMemory: (
    workspace: string
  ) => Promise<Result<{ files: Json[] }>>;
  readWorkspaceMemory: (
    workspace: string,
    filename: string
  ) => Promise<Result<{ detail: Json }>>;
  writeWorkspaceMemory: (
    workspace: string,
    filename: string,
    content: string
  ) => Promise<{ ok: true } | Err>;
  deleteWorkspaceMemory: (
    workspace: string,
    filename: string
  ) => Promise<{ ok: true } | Err>;
  clearWorkspaceMemory: (
    workspace: string
  ) => Promise<Result<{ removed: number }>>;
  workspaceMemoryStatus: (
    workspace: string
  ) => Promise<Result<{ status: Json }>>;
  workspaceMemoryIndex: (
    workspace: string
  ) => Promise<Result<{ content: string }>>;
  debugMemoryPrefetchSection: (
    workspace: string,
    userQuery?: string | null
  ) => Promise<Result<{ section: string | null }>>;
  learningsList: (input?: {
    agentScope?: string | null;
    status?: string | null;
    source?: string | null;
    category?: string | null;
    search?: string | null;
    limit?: number | null;
  }) => Promise<Result<{ learnings: Json[] }>>;
  learningsUpdateBody: (
    learningId: string,
    content: string,
    takeaway?: string | null
  ) => Promise<{ ok: true } | Err>;
  learningsSetStatus: (
    learningId: string,
    next: "pending" | "active" | "merged" | "deprecated"
  ) => Promise<{ ok: true } | Err>;
  learningsDelete: (learningId: string) => Promise<{ ok: true } | Err>;
  learningsGetStatus: (
    agentScope?: string | null
  ) => Promise<Result<{ report: Json }>>;
  learningsTriggerReflection: (
    sessionId: string
  ) => Promise<Result<{ result: Json }>>;
  learningsDeprecate: (learningId: string) => Promise<{ ok: true } | Err>;
  debugSeedLearning: (input: {
    agentScope: string;
    content: string;
    takeaway?: string | null;
    status?: string | null;
    source?: string | null;
    category?: string | null;
  }) => Promise<Result<{ learningId: string }>>;
  promptDump: (
    sessionId: string
  ) => Promise<Result<{ dump: PromptDumpResult }>>;
  getActiveSessionId: () => Promise<Result<{ sessionId: string | null }>>;
  openWorkstationFile: (
    filePath: string
  ) => Promise<Result<{ filePath: string }>>;
  inspectOrgtrackFileSessionHistory: (input: {
    repoPath: string;
    filePath: string;
    limit?: number;
    offset?: number;
  }) => Promise<Result<{ history: Json }>>;
  inspectCliSessionStatus: (
    sessionId: string
  ) => Promise<Result<{ session: Json | null }>>;
  inspectCliHistoryMutation: (
    sessionId: string
  ) => Promise<Result<{ mutation: Json | null }>>;
  resetToNewSession: () => Promise<{ ok: true } | Err>;
  openSession: (sessionId: string) => Promise<Result<{ sessionId: string }>>;
  debugSessionSecuritySnapshot: (
    sessionId: string
  ) => Promise<Result<{ snapshot: Json }>>;
  debugSessionValidateCommand: (
    sessionId: string,
    command: string,
    approved?: boolean | null
  ) => Promise<Result<{ validation: Json }>>;
  debugSessionSubagentSnapshot: (
    sessionId: string
  ) => Promise<Result<{ snapshot: Json }>>;
  debugSessionModelSnapshot: (
    sessionId: string
  ) => Promise<Result<{ snapshot: Json }>>;
  debugSessionToolsSnapshot: (
    sessionId: string
  ) => Promise<Result<{ snapshot: Json }>>;
  listEffectiveToolsForSession: (
    sessionId: string,
    agentExecMode?: string | null
  ) => Promise<Result<{ tools: Json }>>;
  debugSessionOrgRuntimeSnapshot: (
    sessionId: string
  ) => Promise<Result<{ snapshot: Json }>>;
  debugAgentOrgEnable: () => Promise<Result<{ enabled: true }>>;
  debugSessionExecuteTool: (
    sessionId: string,
    toolName: string,
    params: Json
  ) => Promise<Result<{ result: Json }>>;
  debugSessionExecuteOrgTool: (
    sessionId: string,
    toolName: string,
    params: Json
  ) => Promise<Result<{ result: Json }>>;
  launchSession: (params: Json) => Promise<Result<{ result: Json }>>;
  reloadSessionList: () => Promise<
    Result<{ count: number; sessionIds: string[] }>
  >;
  primeSidebarEntityCache: () => Promise<Result<{ count: number }>>;
  inspectSidebarPagination: (
    sessionIds?: string[]
  ) => Promise<Result<{ pagination: Json; sessions: Json[] }>>;
  getSessionAggregateRow: (
    sessionId: string
  ) => Promise<Result<{ session: Json | null }>>;
  getSessionAggregateRowFromList: (
    sessionId: string
  ) => Promise<Result<{ session: Json | null; diagnostics?: Json }>>;
  findSessionAggregateByWorkItem: (
    workItemId: string
  ) => Promise<Result<{ session: Json | null }>>;
  seedChatEvents: (
    sessionId: string,
    events: Json[],
    options?: {
      chatPanelMaximized?: boolean;
      chatWidth?: number;
      currentEventId?: string;
      runtimeStatus?:
        | "idle"
        | "running"
        | "installing"
        | "waiting_for_user"
        | "waiting_for_funds";
      stationMode?: "my-station" | "agent-station";
      selectedApp?: "CODE_EDITOR" | "CHANNELS";
    }
  ) => Promise<Result<{ eventCount: number; chatEventCount: number }>>;
  seedPersistedCachedSession: (input: {
    sessionId: string;
    events: Json[];
    name?: string;
    userInput?: string;
    category?: "cli_agent" | "rust_agent";
    status?: string;
  }) => Promise<Result<{ sessionId: string; eventCount: number }>>;
  seedSessionContextUsage: (
    usage: Json
  ) => Promise<Result<{ usedTokens: number }>>;
  seedSidebarSession: (input: {
    sessionId: string;
    name?: string;
    repoPath?: string;
    status?: string;
    orgId?: string;
    touchedFiles?: string[];
  }) => Promise<Result<{ sessionId: string }>>;
  openWorkManagementTab: () => Promise<Result<{ tabId: string }>>;
  seedModeSwitchSession: (input: {
    sessionId?: string;
    repoPath?: string;
    userText: string;
    reason?: string;
    targetMode?: string;
  }) => Promise<Result<{ sessionId: string; eventId: string }>>;
  seedPlanCard: (input: {
    sessionId: string;
    title?: string;
    content: string;
  }) => Promise<Result<{ planRevisionId: string }>>;
  seedShellProcess: (input: {
    sessionId: string;
    pid: number;
    callId?: string;
    command: string;
    status?: "running" | "background";
  }) => Promise<Result<{ sessionId: string; pid: number }>>;
  seedSubagentJob: (input: {
    sessionId: string;
    handle: string;
    agentName: string;
    subagentType?: string;
    status?: "running" | "completed" | "failed" | "killed";
  }) => Promise<Result<{ sessionId: string; handle: string }>>;
  debugSeedSubagentJobWire: (input: {
    sessionId: string;
    handle: string;
    agentName: string;
    subagentType?: string;
  }) => Promise<Result<{ sessionId: string; handle: string }>>;
  debugSeedCommitLinkWire: (input: {
    sessionId: string;
    commitSha: string;
  }) => Promise<Result<{ sessionId: string; commitSha: string }>>;
  debugSeedFinalDiffWire: (input: {
    sessionId: string;
    source: string;
    filePath: string;
    diff: string;
  }) => Promise<Result<{ sessionId: string; filePath: string }>>;
  debugReadFinalDiffCountWire: (input: {
    sessionId: string;
    source?: string;
  }) => Promise<Result<{ sessionId: string; count: number }>>;
  killSubagentJobWire: (handle: string) => Promise<{ ok: true } | Err>;
  listRunningSubagentJobsWire: () => Promise<Result<{ jobs: Json[] }>>;
  debugSeedChildSessionWire: (input: {
    parentSessionId: string;
    sessionId: string;
    name: string;
    status: string;
    createdAt: string;
    updatedAt: string;
  }) => Promise<Result<{ sessionId: string }>>;
  debugSeedSidebarCodingSessionWire: (input: {
    sessionId: string;
    name: string;
    status: string;
    createdAt: string;
    updatedAt: string;
    pinned?: boolean;
  }) => Promise<Result<{ sessionId: string }>>;
  debugSeedPendingPlanWire: (input: {
    sessionId: string;
    planPath: string;
    planTitle: string;
    planContent: string;
  }) => Promise<Result<{ sessionId: string }>>;
  deleteSessionWire: (sessionId: string) => Promise<{ ok: true } | Err>;
  patchSessionExecModeWire: (
    sessionId: string,
    agentExecMode: string
  ) => Promise<{ ok: true } | Err>;
  getPendingPlanApprovalWire: (
    sessionId: string
  ) => Promise<Result<{ snapshot: Json | null }>>;
  respondPlanApprovalWire: (
    sessionId: string,
    choice: "approve" | "approve_with_edits" | "reject"
  ) => Promise<{ ok: true } | Err>;
  inspectChatState: () => Promise<
    Result<{
      activeSessionId: string | null;
      activeSession: Json | null;
      coreSessionId: string | null;
      stationMode: "my-station" | "agent-station";
      chatPanelMaximized: boolean;
      snapshotEventCount: number;
      snapshotChatEventCount: number;
      chatEventCount: number;
      chatEventIds: string[];
      runtimeStatus: string;
      isSessionActive: boolean;
      isPendingCancel: boolean;
      isQueueEditing: boolean;
      userInitiatedCancel: boolean;
      queuedMessages: Array<{ id: string; sessionId: string; content: string }>;
      runtimeError: string | null;
      rawEvents: Array<{
        id: string;
        source: string;
        actionType: string;
        uiCanonical: string;
        functionName: string;
        displayText: string;
        activityStatus?: string;
        resultStatus?: string | null;
        planRevisionId?: string | null;
        args?: Json;
        result?: Json;
      }>;
      chatEvents: Array<{
        id: string;
        source: string;
        displayText: string;
        displayVariant: string;
      }>;
    }>
  >;
  agentOrgRunList: (limit?: number) => Promise<Result<{ runs: Json[] }>>;
  debugSessionSkillsSnapshot: (
    sessionId: string
  ) => Promise<Result<{ snapshot: Json }>>;
  debugSessionGeneralSnapshot: (
    sessionId: string
  ) => Promise<Result<{ snapshot: Json }>>;
  importDetect: (
    repoPath?: string | null
  ) => Promise<Result<{ items: Json[] }>>;
  importApply: (selections: Json[]) => Promise<Result<{ report: Json }>>;
  listSkills: (
    workspacePath?: string | null,
    agentId?: string | null
  ) => Promise<Result<{ skills: Json[] }>>;
  readSkill: (
    name: string,
    workspacePath?: string | null
  ) => Promise<Result<{ content: string }>>;
  createSkill: (opts: {
    name: string;
    frontmatter: string;
    body: string;
    workspacePath?: string | null;
  }) => Promise<Result<{ skill: Json }>>;
  validateSkillName: (
    name: string,
    workspacePath?: string | null
  ) => Promise<{ ok: true } | Err>;
  toggleSkill: (
    name: string,
    enabled: boolean,
    agentId?: string | null,
    workspacePath?: string | null
  ) => Promise<{ ok: true } | Err>;
  moveSkill: (
    skillPath: string,
    targetScope: "global" | "workspace",
    workspacePath?: string | null
  ) => Promise<Result<{ newPath: string }>>;
  readSkillFiles: (
    skillName: string,
    relativePaths: string[],
    workspacePath?: string | null
  ) => Promise<Result<{ files: Json[] }>>;
  writeSkillFiles: (
    skillName: string,
    files: Array<{ relativePath: string; content: string }>,
    workspacePath?: string | null
  ) => Promise<Result<{ results: Json[] }>>;
  mcpListServers: (
    workspacePath?: string | null
  ) => Promise<Result<{ servers: Json[] }>>;
  mcpGetConfig: (
    scope?: "global" | "workspace" | null,
    workspacePath?: string | null
  ) => Promise<Result<{ config: Json }>>;
  mcpUpdateServers: (
    config: Json,
    scope?: "global" | "workspace" | null,
    workspacePath?: string | null
  ) => Promise<{ ok: true } | Err>;
  mcpTestServer: (
    serverName: string,
    config: Json
  ) => Promise<Result<{ result: Json }>>;
  mcpListServerTools: (
    serverName: string
  ) => Promise<Result<{ tools: Json[] }>>;
  mcpReconnectServer: (serverName: string) => Promise<{ ok: true } | Err>;
  mcpSetServerDisabled: (
    serverName: string,
    disabled: boolean,
    workspacePath?: string | null
  ) => Promise<{ ok: true } | Err>;
  navigateTo: (path: string) => Promise<{ ok: true } | Err>;
  openAgentStationDiff: () => Promise<{ ok: true } | Err>;
  openWorkspaceWorkItemsTab: () => Promise<{ ok: true } | Err>;
  openProjectWorkItemsTab: (
    projectId: string,
    projectName: string,
    projectSlug?: string
  ) => Promise<{ ok: true } | Err>;
  openChatPanelWorkItem: (
    projectSlug: string,
    shortId: string
  ) => Promise<{ ok: true } | Err>;
  openAgentTab: (
    agentId: string,
    tab: string
  ) => Promise<
    | {
        ok: true;
        activeTabId: string | null;
        tabIds: string[];
        stationMode: string;
        pathname: string;
      }
    | Err
  >;
  openOrgTab: (
    orgId: string,
    displayName?: string
  ) => Promise<{ ok: true } | Err>;
  inspectWorkstationSurface: () => Promise<
    Result<{
      pathname: string;
      stationMode: string;
      activeHost: string;
      activeTabId: string | null;
      activeTabType: string | null;
      activeTabCategory: string | null;
      selectedRepoId: string;
      selectedRepoPath: string | null;
      codeEditorPresent: boolean;
      agentConfigRootCount: number;
    }>
  >;
  seedUserPresence: (opts: {
    roles: CustomRoleDefinition[];
    presence: UserPresenceState;
  }) => Promise<
    Result<{
      roleCount: number;
      mode: string;
      wire: UserPresenceWire | undefined;
    }>
  >;
  cloudSeedAuthState: (opts: {
    supabaseUrl: string;
    anonKey: string;
    userId: string;
    accessToken: string;
    refreshToken: string;
    expiresAt: number;
    displayName?: string;
  }) => Promise<Result<{ userId: string }>>;
  cloudClearAuthState: () => Promise<{ ok: true } | Err>;
  cloudReadAuthState: () => Promise<
    Result<{ signedIn: boolean; userId: string | null }>
  >;
  cloudSeedProjectOrgAlias: (opts: {
    localOrgId: string;
    externalOrgId: string;
    name: string;
  }) => Promise<Result<{ localOrgId: string; externalOrgId: string }>>;
  cloudSeedOrgs: (opts: {
    orgs: Array<{ orgId: string; name: string; role: string }>;
  }) => Promise<Result<{ count: number }>>;
  cloudListOrgs: () => Promise<Result<{ orgs: Json[] }>>;
  cloudInspectMemberRoster: (opts: { orgId: string }) => Promise<
    Result<{
      rosterVersion: number;
      members: Json[] | null;
    }>
  >;
  cloudInspectRosterState: () => Promise<
    Result<{
      orgs: Json[];
      directOrgs: Json[] | null;
      loaded: boolean;
      requestEpoch: number;
      converging: boolean;
    }>
  >;
  cloudInspectProjectState: (opts: {
    projectSlug: string;
    workItemId?: string;
  }) => Promise<
    Result<{
      cachedPresent: boolean;
      freshPresent: boolean;
      cachedSlugs: string[];
      freshSlugs: string[];
      projectOrgId: string | null;
      workItem: Json | null;
      enrichedWorkItem: Json | null;
      scopedEnrichedWorkItem: Json | null;
      selectedWorkItem: Json | null;
      dataChangedSignal: number;
      pendingOutbox: Json[];
    }>
  >;
  cloudSeedRepoScopes: (opts: {
    orgId: string;
    repoScopes: string[];
  }) => Promise<Result<{ count: number }>>;
  cloudResolveRepoScopeKeys: (opts: {
    repoPath: string;
  }) => Promise<Result<{ keys: string[] | null }>>;
  cloudSeedRemoteSessions: (opts: {
    orgId: string;
    sessions: Json[];
  }) => Promise<Result<{ count: number }>>;
  cloudInspectDebugState: (opts: {
    sessionId?: string;
  }) => Promise<Result<{ debug: Json }>>;
  cloudInspectPresence: () => Promise<
    Result<{
      presence: Json;
      outbound: Json;
      activeSessionId: string | null;
      resolvedSessionRefs: Json[];
      visibilityState: string;
    }>
  >;
  cloudPublishSeededSessionEvents: (opts: {
    orgId: string;
    sessionId: string;
    newEpoch?: number;
  }) => Promise<Result<{ eventCount: number; epoch: number }>>;
  cloudRunSyncPass: () => Promise<{ ok: true } | Err>;
  cloudSeedPendingInvite: (opts: {
    link: string;
  }) => Promise<Result<{ inviteCode: string }>>;
  cloudSeedPendingShare: (opts: {
    link: string;
  }) => Promise<Result<{ shareToken: string }>>;
  cloudTagSessionToOrg: (opts: {
    sessionId: string;
    orgId: string;
  }) => Promise<{ ok: true } | Err>;
  cloudOpenSyncLevelDialog: (opts: {
    sessionId: string;
  }) => Promise<Result<{ sessionId: string }>>;
  cloudCloseSyncLevelDialog: () => Promise<{ ok: true } | Err>;
  getLocationPathname: () => string;
}

declare global {
  interface Window {
    __e2e?: E2EHelpers;
    __ORGII_E2E_CLAUDE_OAUTH_MOCK__?: boolean;
    __ORGII_E2E_CODEX_OAUTH_MOCK__?: boolean;
    __ORGII_E2E_GEMINI_OAUTH_MOCK__?: boolean;
    __ORGII_E2E_MODE_SWITCH_MOCK__?: boolean;
  }
}
