import { useCallback, useMemo, useSyncExternalStore } from "react";

import { AGENT_ORG_ENABLED } from "@src/config/agentOrgAvailability";
import {
  getAgentOrgRunViewSnapshot,
  refreshAgentOrgRunView,
  subscribeAgentOrgRunView,
} from "@src/engines/ChatPanel/InputArea/components/agentOrgRunViewStore";
import {
  isCliSession,
  isImportedHistorySession,
} from "@src/util/session/sessionDispatch";

const EMPTY_RESULT = { view: null, error: null } as const;

/**
 * Reads the shared Agent Org projection for a session.
 *
 * All mounted consumers participate in one visibility-aware reconciliation
 * loop per org run. Local Agent Org mutations invalidate the projection
 * immediately; the slow fallback poll only covers background member activity.
 */
export function useAgentOrgRunView(sessionId: string | null) {
  const canFetchRunView =
    AGENT_ORG_ENABLED &&
    !!sessionId &&
    !isCliSession(sessionId) &&
    !isImportedHistorySession(sessionId);

  const subscribe = useCallback(
    (subscriber: () => void) => {
      if (!canFetchRunView || !sessionId) return () => {};
      return subscribeAgentOrgRunView(sessionId, subscriber);
    },
    [canFetchRunView, sessionId]
  );
  const getSnapshot = useCallback(() => {
    if (!canFetchRunView || !sessionId) return EMPTY_RESULT;
    return getAgentOrgRunViewSnapshot(sessionId);
  }, [canFetchRunView, sessionId]);
  const snapshot = useSyncExternalStore(subscribe, getSnapshot, getSnapshot);

  const refresh = useCallback(async () => {
    if (!canFetchRunView || !sessionId) return;
    await refreshAgentOrgRunView(sessionId);
  }, [canFetchRunView, sessionId]);

  return useMemo(
    () => ({
      view: snapshot.view,
      error: snapshot.error,
      refresh,
    }),
    [refresh, snapshot]
  );
}
