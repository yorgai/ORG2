import { invoke } from "@tauri-apps/api/core";

import { asError } from "../result";
import type { E2EHelpers, Json, Result } from "../types";
import { e2eUrl } from "./e2eBaseUrl";

type AgentOrgE2EHelpers = Pick<
  E2EHelpers,
  | "listAgentOrgs"
  | "removeAgentOrg"
  | "debugAgentOrgEnable"
  | "debugSessionOrgRuntimeSnapshot"
  | "debugSessionExecuteTool"
  | "debugSessionExecuteOrgTool"
  | "debugAgentOrgExecuteToolAsAgent"
  | "debugAgentOrgEmitMemberIdle"
  | "debugAgentOrgInboxList"
  | "listAgentOrgSessionInbox"
  | "debugAgentOrgTasksList"
  | "agentOrgSessionRunView"
  | "agentOrgGroupProjectionPage"
  | "agentOrgSessionInterventionState"
  | "agentOrgRunList"
  | "agentOrgPauseRun"
  | "agentOrgResumeRun"
  | "agentOrgSimulateAppRestart"
>;

export function createAgentOrgHelpers(): AgentOrgE2EHelpers {
  const debugAgentOrgEnable = async (): Promise<Result<{ enabled: true }>> => {
    try {
      await invoke("debug_agent_org_enable");
      return { ok: true, enabled: true };
    } catch (err) {
      return asError(err);
    }
  };

  const listAgentOrgs = async (): Promise<Result<{ orgs: Json[] }>> => {
    try {
      const orgs = (await invoke("agent_orgs_list")) as Json[];
      return { ok: true, orgs };
    } catch (err) {
      return asError(err);
    }
  };

  const removeAgentOrg = async (
    orgId: string
  ): Promise<Result<{ removed: boolean }>> => {
    try {
      if (!orgId) {
        return { ok: false, error: "removeAgentOrg: `orgId` is required" };
      }
      await invoke("agent_orgs_remove", { orgId });
      return { ok: true, removed: true };
    } catch (err) {
      return asError(err);
    }
  };

  const debugSessionOrgRuntimeSnapshot = async (
    sessionId: string
  ): Promise<Result<{ snapshot: Json }>> => {
    try {
      if (!sessionId) {
        return {
          ok: false,
          error: "debugSessionOrgRuntimeSnapshot: `sessionId` is required",
        };
      }
      const snapshot = (await invoke("debug_session_org_runtime_snapshot", {
        sessionId,
      })) as Json;
      return { ok: true, snapshot };
    } catch (err) {
      return asError(err);
    }
  };

  const debugSessionExecuteTool = async (
    sessionId: string,
    toolName: string,
    params: Json
  ): Promise<Result<{ result: Json }>> => {
    try {
      if (!sessionId) {
        return {
          ok: false,
          error: "debugSessionExecuteTool: `sessionId` is required",
        };
      }
      if (!toolName) {
        return {
          ok: false,
          error: "debugSessionExecuteTool: `toolName` is required",
        };
      }
      const result = (await invoke("debug_session_execute_tool", {
        sessionId,
        toolName,
        params,
      })) as Json;
      return { ok: true, result };
    } catch (err) {
      return asError(err);
    }
  };

  const debugSessionExecuteOrgTool = async (
    sessionId: string,
    toolName: string,
    params: Json
  ): Promise<Result<{ result: Json }>> => {
    try {
      if (!sessionId) {
        return {
          ok: false,
          error: "debugSessionExecuteOrgTool: `sessionId` is required",
        };
      }
      if (!toolName) {
        return {
          ok: false,
          error: "debugSessionExecuteOrgTool: `toolName` is required",
        };
      }
      const result = (await invoke("debug_session_execute_org_tool", {
        sessionId,
        toolName,
        params,
      })) as Json;
      return { ok: true, result };
    } catch (err) {
      return asError(err);
    }
  };

  const debugAgentOrgExecuteToolAsAgent = async (
    runId: string,
    senderMemberId: string,
    toolName: string,
    params: Json
  ): Promise<Result<{ result: Json }>> => {
    try {
      if (!runId) {
        return {
          ok: false,
          error: "debugAgentOrgExecuteToolAsAgent: `runId` is required",
        };
      }
      if (!senderMemberId) {
        return {
          ok: false,
          error:
            "debugAgentOrgExecuteToolAsAgent: `senderMemberId` is required",
        };
      }
      if (!toolName) {
        return {
          ok: false,
          error: "debugAgentOrgExecuteToolAsAgent: `toolName` is required",
        };
      }
      const result = (await invoke("debug_agent_org_execute_tool_as_agent", {
        runId,
        senderMemberId,
        toolName,
        params,
      })) as Json;
      return { ok: true, result };
    } catch (err) {
      return asError(err);
    }
  };

  const debugAgentOrgEmitMemberIdle = async (
    runId: string,
    memberId: string,
    reason: string,
    failureReason?: string | null,
    currentMode?: string | null
  ): Promise<Result<{ result: Json }>> => {
    try {
      if (!runId) {
        return {
          ok: false,
          error: "debugAgentOrgEmitMemberIdle: `runId` is required",
        };
      }
      if (!memberId) {
        return {
          ok: false,
          error: "debugAgentOrgEmitMemberIdle: `memberId` is required",
        };
      }
      const result = (await invoke("debug_agent_org_emit_member_idle", {
        runId,
        memberId,
        reason,
        failureReason: failureReason ?? null,
        currentMode: currentMode ?? null,
      })) as Json;
      return { ok: true, result };
    } catch (err) {
      return asError(err);
    }
  };

  const debugAgentOrgInboxList = async (
    runId: string
  ): Promise<Result<{ rows: Json[] }>> => {
    try {
      if (!runId) {
        return {
          ok: false,
          error: "debugAgentOrgInboxList: `runId` is required",
        };
      }
      const rows = (await invoke("debug_agent_org_inbox_list", {
        runId,
      })) as Json[];
      return { ok: true, rows };
    } catch (err) {
      return asError(err);
    }
  };

  const listAgentOrgSessionInbox = async (
    sessionId: string
  ): Promise<Result<{ rows: Json[] }>> => {
    try {
      if (!sessionId) {
        return {
          ok: false,
          error: "listAgentOrgSessionInbox: `sessionId` is required",
        };
      }
      const view = (await invoke("agent_org_session_run_view", {
        sessionId,
      })) as {
        currentMemberId?: string | null;
        inbox?: Json[];
      } | null;
      const currentMemberId = view?.currentMemberId;
      if (!currentMemberId) {
        return { ok: true, rows: [] };
      }
      const rows = (view?.inbox ?? []).filter((row) => {
        if (typeof row !== "object" || row === null) return false;
        return (
          (row as { recipientMemberId?: unknown }).recipientMemberId ===
          currentMemberId
        );
      });
      return { ok: true, rows };
    } catch (err) {
      return asError(err);
    }
  };

  const debugAgentOrgTasksList = async (
    runId: string
  ): Promise<Result<{ tasks: Json[] }>> => {
    try {
      if (!runId) {
        return {
          ok: false,
          error: "debugAgentOrgTasksList: `runId` is required",
        };
      }
      const tasks = (await invoke("debug_agent_org_tasks_list", {
        runId,
      })) as Json[];
      return { ok: true, tasks };
    } catch (err) {
      return asError(err);
    }
  };

  const agentOrgSessionRunView = async (
    sessionId: string
  ): Promise<Result<{ view: Json | null }>> => {
    try {
      if (!sessionId) {
        return {
          ok: false,
          error: "agentOrgSessionRunView: `sessionId` is required",
        };
      }
      const view = (await invoke("agent_org_session_run_view", {
        sessionId,
      })) as Json | null;
      return { ok: true, view };
    } catch (err) {
      return asError(err);
    }
  };

  const agentOrgGroupProjectionPage = async (
    sessionId: string,
    cursor?: string | null,
    limit?: number
  ): Promise<Result<{ page: Json }>> => {
    try {
      if (!sessionId) {
        return {
          ok: false,
          error: "agentOrgGroupProjectionPage: `sessionId` is required",
        };
      }
      const page = (await invoke("agent_org_group_projection_page", {
        sessionId,
        cursor: cursor ?? null,
        limit: limit ?? 50,
      })) as Json;
      return { ok: true, page };
    } catch (err) {
      return asError(err);
    }
  };

  const agentOrgSessionInterventionState = async (
    sessionId: string
  ): Promise<Result<{ state: Json }>> => {
    try {
      if (!sessionId) {
        return {
          ok: false,
          error: "agentOrgSessionInterventionState: `sessionId` is required",
        };
      }
      const state = (await invoke("agent_org_session_intervention_state", {
        sessionId,
      })) as Json;
      return { ok: true, state };
    } catch (err) {
      return asError(err);
    }
  };

  const agentOrgRunList = async (
    limit?: number
  ): Promise<Result<{ runs: Json[] }>> => {
    try {
      const runs = (await invoke("agent_org_run_list", {
        limit: limit ?? null,
      })) as Json[];
      return { ok: true, runs };
    } catch (err) {
      return asError(err);
    }
  };

  const agentOrgPauseRun = async (
    sessionId: string,
    requestId: string = crypto.randomUUID()
  ): Promise<Result<{ outcome: Json }>> => {
    try {
      if (!sessionId) {
        return {
          ok: false,
          error: "agentOrgPauseRun: `sessionId` is required",
        };
      }
      const outcome = (await invoke("agent_org_pause_run", {
        sessionId,
        requestId,
      })) as Json;
      return { ok: true, outcome };
    } catch (err) {
      return asError(err);
    }
  };

  const agentOrgResumeRun = async (
    sessionId: string,
    requestId: string = crypto.randomUUID()
  ): Promise<Result<{ outcome: Json }>> => {
    try {
      if (!sessionId) {
        return {
          ok: false,
          error: "agentOrgResumeRun: `sessionId` is required",
        };
      }
      const outcome = (await invoke("agent_org_resume_run", {
        sessionId,
        requestId,
      })) as Json;
      return { ok: true, outcome };
    } catch (err) {
      return asError(err);
    }
  };

  const agentOrgSimulateAppRestart = async (): Promise<
    Result<{
      intentsReconciled: number;
      terminalSessionsReconciled: number;
      sessionsAbandoned: number;
      tasksRequeued: number;
      runsCompleted: number;
      runsPaused: number;
      interventionsCleared: number;
    }>
  > => {
    try {
      const response = await fetch(
        e2eUrl("/agent/test/agent-org/simulate-app-restart"),
        { method: "POST", headers: { "Content-Type": "application/json" } }
      );
      const body = (await response.json()) as {
        ok: boolean;
        error?: string;
        intents_reconciled?: number;
        terminal_sessions_reconciled?: number;
        sessions_abandoned?: number;
        tasks_requeued?: number;
        runs_completed?: number;
        runs_paused?: number;
        interventions_cleared?: number;
      };
      if (!body.ok) {
        return {
          ok: false,
          error: body.error ?? "simulate-app-restart failed",
        };
      }
      return {
        ok: true,
        intentsReconciled: body.intents_reconciled ?? 0,
        terminalSessionsReconciled: body.terminal_sessions_reconciled ?? 0,
        sessionsAbandoned: body.sessions_abandoned ?? 0,
        tasksRequeued: body.tasks_requeued ?? 0,
        runsCompleted: body.runs_completed ?? 0,
        runsPaused: body.runs_paused ?? 0,
        interventionsCleared: body.interventions_cleared ?? 0,
      };
    } catch (err) {
      return asError(err);
    }
  };

  return {
    debugAgentOrgEnable,
    listAgentOrgs,
    removeAgentOrg,
    debugSessionOrgRuntimeSnapshot,
    debugSessionExecuteTool,
    debugSessionExecuteOrgTool,
    debugAgentOrgExecuteToolAsAgent,
    debugAgentOrgEmitMemberIdle,
    debugAgentOrgInboxList,
    listAgentOrgSessionInbox,
    debugAgentOrgTasksList,
    agentOrgSessionRunView,
    agentOrgGroupProjectionPage,
    agentOrgSessionInterventionState,
    agentOrgRunList,
    agentOrgPauseRun,
    agentOrgResumeRun,
    agentOrgSimulateAppRestart,
  };
}
