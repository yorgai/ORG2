/**
 * Cloud-org "Team sessions" sidebar section (managed ORG2 Cloud scope).
 *
 * Replaces the Cloud Org panel's shared-sessions list: when the sidebar's
 * active scope is a cloud org, teammates' shared sessions render as
 * collapsible fork-threaded groups under a separator-headed section.
 * Threads come from the pure `buildCloudSessionThreads` helper; replay/fork
 * ride the same canonical `useCloudSessionActions` used by Kanban List.
 *
 * Team Conversations is remote-only: exact local-device rows are filtered
 * before grouping and stay under My Sessions. Same-account rows without a
 * matching local session id are retained because they came from another
 * device. Every rendered row gets a `cloudremote-<orgId>|<rowId>` id.
 *
 * Parent-row choice: a thread root sets `navigableParent`, so a body/label
 * click OPENS the source session (replay/open) while the dedicated chevron
 * toggles the fork thread — without the flag the primitive treats a
 * children-bearing row as a group header whose whole body only toggles,
 * which stranded fork sources as unclickable once a fork added a child row.
 * The primitive renders hover rowActions on LEAF rows only, so Replay/Fork
 * hover buttons appear on descendants and on single-row threads (rendered
 * as leaves); a multi-row thread's root keeps click-to-replay but has no
 * hover fork button — no self-duplicate child row is injected.
 *
 * This hook is a coordinator: row construction, menu-item assembly, roster
 * loading, local-hydration bookkeeping, and the member-filter dropdown each
 * live in a sibling `cloudSessionsSection.*` module (see those files' own
 * header comments).
 */
import { useAtom, useAtomValue, useSetAtom, useStore } from "jotai";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { deleteSession as deleteLocalSession } from "@src/api/tauri/agent";
import { deleteOrgtrackCollaborationSession } from "@src/api/tauri/lineage";
import Message from "@src/components/Message";
import {
  hiddenRemoteSessionKey,
  readHiddenRemoteSessionIds,
  writeHiddenRemoteSessionIds,
} from "@src/features/Org2Cloud/cloudHiddenRemoteSessions";
import {
  isRemoteSessionPinned,
  readPinnedRemoteSessionIds,
  togglePinnedRemoteSession,
  writePinnedRemoteSessionIds,
} from "@src/features/Org2Cloud/cloudPinnedRemoteSessions";
import { dismissCloudReferenceOpeningToast } from "@src/features/Org2Cloud/cloudReferenceOpeningToast";
import {
  buildCloudRemoteItemId,
  includeRevealedCloudRow,
  parseCloudRemoteItemId,
} from "@src/features/Org2Cloud/cloudRemoteItemId";
import { cloudDownloadStartRequestAtom } from "@src/features/Org2Cloud/cloudSessionDownloadControlAtoms";
import { filterCloudSessionRows } from "@src/features/Org2Cloud/cloudSessionFilter";
import { buildCloudSessionReference } from "@src/features/Org2Cloud/cloudSessionReference";
import {
  buildCloudSessionThreads,
  collectCloudFlatListExcludedSessionIds,
  collectTeamConversationSessionIds,
} from "@src/features/Org2Cloud/cloudSessionThreads";
import { org2CloudAuthAtom } from "@src/features/Org2Cloud/org2CloudAuthAtom";
import { org2CloudPresenceAtom } from "@src/features/Org2Cloud/org2CloudPresenceAtom";
import { useCloudOrgRemoteSessions } from "@src/features/Org2Cloud/org2CloudRemoteSessionsAtom";
import {
  org2CloudPushCursorsAtom,
  org2CloudPushedMetadataAtom,
} from "@src/features/Org2Cloud/org2CloudSyncAtoms";
import { REFUSAL_MESSAGE_DURATION_MS } from "@src/features/Org2Cloud/referenceRefusalMessage";
import {
  type CloudSessionReplayOptions,
  useCloudSessionActions,
} from "@src/features/Org2Cloud/useCloudSessionActions";
import {
  useCloudSessionDownloadProgressEntry,
  useCloudSessionPendingPlayEntry,
} from "@src/features/Org2Cloud/useCloudSessionDownloadSurface";
import { useSessionView } from "@src/hooks/ui/tabs/useSessionView";
import { useRefreshSpin } from "@src/hooks/ui/useRefreshSpin";
import type { NavigationMenuItem } from "@src/scaffold/NavigationSidebar/components/NavigationMenu/config";
import { openOrReplaceSessionInChatPanelTabAtom } from "@src/store/chatPanel/chatPanelTabOpen/session";
import type { RemoteTeammateSessionMetadata } from "@src/store/collaboration/types";
import { loadSidebarSessionById, removeSession } from "@src/store/session";
import { copyText } from "@src/util/data/clipboard";

import type { SidebarTabDisposition } from "../sidebarTabNavigation";
import { CLOUD_TEAM_SESSIONS_LOAD_MORE_ID } from "./cloudScopedMenuItems";
import { buildCloudSessionNativeMenuItems } from "./cloudSessionNativeMenuItems";
import { useCloudMemberFilterDropdown } from "./cloudSessionsSection.MemberFilterDropdown";
import {
  type CloudAutoReplaySkipReason,
  useCloudSessionAutoReplayReveal,
} from "./cloudSessionsSection.autoReplayReveal";
import { useCloudLocalSessionHydration } from "./cloudSessionsSection.localHydration";
import { useCloudTeamSessionMenuItems } from "./cloudSessionsSection.menuItems";
import { useCloudRemoteRowMaps } from "./cloudSessionsSection.remoteRowMaps";
import { useCloudOrgRosterMembers } from "./cloudSessionsSection.rosterMembers";
import { useCloudSessionRowItemBuilder } from "./cloudSessionsSection.rowItemBuilder";
import { resolveCloudDownloadMenuItemId } from "./cloudSessionsSection.selection";
import type {
  MemberFilterMenuState,
  UseCloudSessionsSectionParams,
  UseCloudSessionsSectionResult,
} from "./cloudSessionsSection.types";
import { resetScopedSectionPagination } from "./sectionPagination";

export function useCloudSessionsSection({
  orgId,
  sessions,
  filter,
  activeSessionId,
  localSessionHydrationLimit,
  groupVisibleCount,
  revealedMenuItemId,
  openSessionAtDestination,
  onFilterChange,
}: UseCloudSessionsSectionParams): UseCloudSessionsSectionResult {
  const { t } = useTranslation("navigation");
  const { t: tCommon } = useTranslation("common");
  const { t: tSessions } = useTranslation("sessions");
  const store = useStore();
  const { rows, state, fetchedAt, documentVisible, refresh } =
    useCloudOrgRemoteSessions(orgId);
  const { spinClass: refreshSpinClass, handleClick: handleRefreshClick } =
    useRefreshSpin(
      refresh,
      false,
      orgId ? `cloud-team-sessions:${orgId}` : undefined
    );
  const { replaySession, forkSession, busySessionRows } =
    useCloudSessionActions(orgId);
  const presenceMap = useAtomValue(org2CloudPresenceAtom);
  const pushedMetadata = useAtomValue(org2CloudPushedMetadataAtom);
  const pushCursors = useAtomValue(org2CloudPushCursorsAtom);
  const [auth, setAuth] = useAtom(org2CloudAuthAtom);
  const selfUserId = auth?.userId ?? null;
  const rosterMembers = useCloudOrgRosterMembers({
    orgId,
    auth,
    setAuth,
    store,
  });
  const [memberMenu, setMemberMenu] = useState<MemberFilterMenuState | null>(
    null
  );
  const [hiddenRemoteSessionIds, setHiddenRemoteSessionIds] = useState(
    readHiddenRemoteSessionIds
  );
  const [pinnedRemoteSessionIds, setPinnedRemoteSessionIds] = useState(
    readPinnedRemoteSessionIds
  );

  const { localOwnSessionIds, cloudLocalSessionIds } =
    useCloudLocalSessionHydration({
      orgId,
      sessions,
      pushedMetadata,
      pushCursors,
      selfUserId,
      rows,
      documentVisible,
      localSessionHydrationLimit,
    });

  const unhiddenRows = useMemo(
    () =>
      rows.filter(
        (row) =>
          !hiddenRemoteSessionIds.has(hiddenRemoteSessionKey(row.orgId, row.id))
      ),
    [hiddenRemoteSessionIds, rows]
  );

  const visibleRows = useMemo(() => {
    const filtered = filterCloudSessionRows(unhiddenRows, filter);
    // Cross-surface navigation bypasses presentation filters for one row but
    // never mutates the user's persistent Team Sessions filter.
    return includeRevealedCloudRow(
      filtered,
      unhiddenRows,
      orgId,
      revealedMenuItemId
    );
  }, [filter, orgId, revealedMenuItemId, unhiddenRows]);

  const threads = useMemo(
    () =>
      orgId
        ? buildCloudSessionThreads(visibleRows, {
            // Filtering happens before grouping so duplicate suppression and
            // thread roots derive from the exact visible row set.
            memberFilter: null,
            localOwnSessionIds,
            viewerUserId: selfUserId,
          })
        : [],
    [orgId, visibleRows, localOwnSessionIds, selfUserId]
  );
  const teamPaginationScopeKey = useMemo(() => {
    if (!orgId) return "";
    const memberKey = filter.kind === "member" ? filter.ownerUserId : "";
    return `${orgId}\u001f${filter.kind}\u001f${memberKey}`;
  }, [filter, orgId]);
  const [teamPagination, setTeamPagination] = useState<{
    scopeKey: string;
    visibleCount: number;
  }>({
    scopeKey: "",
    visibleCount: groupVisibleCount,
  });
  const requestedTeamVisibleCount =
    teamPagination.scopeKey === teamPaginationScopeKey
      ? teamPagination.visibleCount
      : groupVisibleCount;
  const revealedThreadIndex = useMemo(() => {
    if (!revealedMenuItemId) return -1;
    return threads.findIndex((thread) =>
      [thread.root, ...thread.descendants].some((threadRow) => {
        const itemId = buildCloudRemoteItemId(
          threadRow.row.orgId,
          threadRow.row.id
        );
        return itemId === revealedMenuItemId;
      })
    );
  }, [revealedMenuItemId, threads]);
  const teamVisibleCount = Math.max(
    requestedTeamVisibleCount,
    revealedThreadIndex + 1
  );
  const visibleThreads = useMemo(
    () => threads.slice(0, teamVisibleCount),
    [teamVisibleCount, threads]
  );
  const resetCloudTeamPagination = useCallback(() => {
    setTeamPagination((current) =>
      resetScopedSectionPagination(current, groupVisibleCount)
    );
  }, [groupVisibleCount]);

  // Imported teammate replays materialize a local read-only cache row: hide
  // those caches from My Sessions. Own sessions that belong to a MULTI-owner
  // conversation family hide too — the family's Team Sessions thread is the
  // conversation's single sidebar entry (badge and thread included).
  const cloudFlatListExcludedSessionIds = useMemo(() => {
    if (!orgId) return new Set<string>();
    const excluded = collectCloudFlatListExcludedSessionIds(sessions, orgId);
    for (const sessionId of collectTeamConversationSessionIds(
      rows,
      selfUserId
    )) {
      excluded.add(sessionId);
    }
    return excluded;
  }, [orgId, sessions, rows, selfUserId]);

  const pendingPlay = useCloudSessionPendingPlayEntry(activeSessionId);
  const downloadProgress =
    useCloudSessionDownloadProgressEntry(activeSessionId);

  const selectedCloudMenuItemId = useMemo(() => {
    if (!orgId || !activeSessionId) return null;
    const downloadMenuItemId = resolveCloudDownloadMenuItemId(
      orgId,
      pendingPlay ?? downloadProgress
    );
    if (downloadMenuItemId) return downloadMenuItemId;
    const active = sessions.find(
      (session) => session.session_id === activeSessionId
    );
    const imported = active?.importedFrom;
    if (!imported || imported.orgId !== orgId) return null;
    const sourceRow = visibleThreads
      .flatMap((thread) => [thread.root, ...thread.descendants])
      .map((threadRow) => threadRow.row)
      .find(
        (row) =>
          !row.deletedAt && row.sourceSessionId === imported.sourceSessionId
      );
    return sourceRow ? buildCloudRemoteItemId(orgId, sourceRow.id) : null;
  }, [
    activeSessionId,
    downloadProgress,
    orgId,
    pendingPlay,
    sessions,
    visibleThreads,
  ]);

  const findRow = useCallback(
    (rowId: string): RemoteTeammateSessionMetadata | undefined =>
      rows.find((row) => row.id === rowId),
    [rows]
  );

  // Starting a manual replay for a hidden row clears its local hide marker so
  // the imported replay and teammate row become visible again.
  const resubscribeRemoteRow = useCallback(
    (row: RemoteTeammateSessionMetadata) => {
      setHiddenRemoteSessionIds((current) => {
        const key = hiddenRemoteSessionKey(row.orgId, row.id);
        if (!current.has(key)) return current;
        const next = new Set(current);
        next.delete(key);
        writeHiddenRemoteSessionIds(next);
        return next;
      });
    },
    []
  );

  const runReplay = useCallback(
    (
      row: RemoteTeammateSessionMetadata,
      options?: CloudSessionReplayOptions
    ) => {
      // The replay is starting: the pre-phase toast has served its purpose
      // (a no-op for sidebar-origin clicks that never showed one).
      dismissCloudReferenceOpeningToast();
      resubscribeRemoteRow(row);
      void replaySession(row, options);
    },
    [replaySession, resubscribeRemoteRow]
  );

  // The pane's play/resume cards cannot reach the replay hook; they park a
  // start request here. First mounted consumer wins (the store re-read makes
  // the second connector's effect a no-op), and the busy registry dedups any
  // race that slips through.
  const downloadStartRequest = useAtomValue(cloudDownloadStartRequestAtom);
  useEffect(() => {
    if (!downloadStartRequest || downloadStartRequest.orgId !== orgId) return;
    if (store.get(cloudDownloadStartRequestAtom) !== downloadStartRequest) {
      return;
    }
    const row = findRow(downloadStartRequest.rowId);
    if (!row) return;
    const startKind = downloadStartRequest.kind;
    store.set(cloudDownloadStartRequestAtom, null);
    // queueMicrotask to satisfy react-hooks/set-state-in-effect: the
    // resubscribe inside runReplay touches React state, and the request
    // slot was already consumed synchronously above.
    queueMicrotask(() => {
      if (startKind === "fork") {
        void forkSession(row, { skipDownloadGate: true });
      } else {
        runReplay(row, { skipDownloadGate: true });
      }
    });
  }, [downloadStartRequest, findRow, forkSession, orgId, runReplay, store]);

  const runFork = useCallback(
    (row: RemoteTeammateSessionMetadata) => {
      void forkSession(row);
    },
    [forkSession]
  );

  const openTeamSessionAtDestination = useCallback(
    (
      row: RemoteTeammateSessionMetadata,
      destination: SidebarTabDisposition | "my-station" | "new-window"
    ) => {
      const openLocalSession = (sessionId: string) => {
        openSessionAtDestination(destination, {
          sessionId,
          title: row.title,
        });
      };

      // A viewer's own row in a multi-owner conversation remains the writable
      // local original. Never mint a read-only imported copy for it.
      if (
        row.ownerUserId === selfUserId &&
        localOwnSessionIds.has(row.sourceSessionId)
      ) {
        openLocalSession(row.sourceSessionId);
        return;
      }

      // A replay already in flight has already chosen its deterministic local
      // id. The destination action should still work instead of becoming a
      // dead menu item while the transcript downloads.
      const busy = busySessionRows.get(row.id);
      if (busy) {
        if (busy.kind === "replay" && busy.localSessionId) {
          openLocalSession(busy.localSessionId);
        }
        return;
      }

      runReplay(row, {
        openSurface: ({ localSessionId }) => openLocalSession(localSessionId),
      });
    },
    [
      busySessionRows,
      localOwnSessionIds,
      openSessionAtDestination,
      runReplay,
      selfUserId,
    ]
  );

  const { openSession } = useSessionView();
  const openOrReplaceSessionTab = useSetAtom(
    openOrReplaceSessionInChatPanelTabAtom
  );
  // The chip's contract is "take me to this transcript". For the viewer's
  // own session that means opening the live local original — the bare
  // sidebar reveal alone highlights nothing unless the session is already
  // active, which read as a dead click.
  //
  // The reveal-local decision trusts persisted push markers, which can
  // outlive the local session (deleted locally while the cloud row and
  // marker survive until the vanished sweep). Confirm the session actually
  // exists — demand-hydrating one that is merely unloaded — before
  // repointing any tab at it; a stale marker earns the same refusal a
  // missing cloud row would.
  const handleRevealLocal = useCallback(
    (sessionId: string) => {
      dismissCloudReferenceOpeningToast();
      void loadSidebarSessionById(sessionId).then((local) => {
        if (!local) {
          Message.error(t("cloud.sessionRef.sessionNotFound"), {
            duration: REFUSAL_MESSAGE_DURATION_MS,
            closable: true,
          });
          return;
        }
        const sessionName = local.name ?? sessionId;
        openOrReplaceSessionTab({ sessionId, sessionName });
        openSession(sessionId, sessionName);
      });
    },
    [openOrReplaceSessionTab, openSession, t]
  );

  const handleAutoReplaySkip = useCallback(
    (reason: CloudAutoReplaySkipReason) => {
      dismissCloudReferenceOpeningToast();
      // Same rationale as the admission refusal toast: this skip is the ONLY
      // visible outcome of the click, and the 1s default reads as a dead chip.
      Message.error(
        reason === "not-found"
          ? t("cloud.sessionRef.sessionNotFound")
          : t("cloud.sidebar.notPublished"),
        { duration: REFUSAL_MESSAGE_DURATION_MS, closable: true }
      );
    },
    [t]
  );

  // A reference aimed at a row that is ALREADY downloading refocuses the
  // tab that download opened — same contract as clicking the busy row.
  const handleAutoReplayFocusBusy = useCallback(
    (row: RemoteTeammateSessionMetadata, localSessionId?: string) => {
      dismissCloudReferenceOpeningToast();
      if (!localSessionId) return;
      openOrReplaceSessionTab({
        sessionId: localSessionId,
        sessionName: row.title,
      });
    },
    [openOrReplaceSessionTab]
  );

  useCloudSessionAutoReplayReveal({
    orgId,
    rows,
    state,
    fetchedAt,
    busySessionRows,
    selfUserId,
    localOwnSessionIds,
    // The spin wrapper, not the raw refresh: the freshness probe a chip
    // triggers should be visible on the section's refresh icon.
    refresh: handleRefreshClick,
    runReplay,
    onRevealLocal: handleRevealLocal,
    onFocusBusy: handleAutoReplayFocusBusy,
    onSkip: handleAutoReplaySkip,
  });

  const handleCloudSessionItemClick = useCallback(
    (item: NavigationMenuItem, disposition: SidebarTabDisposition): boolean => {
      if (item.id === CLOUD_TEAM_SESSIONS_LOAD_MORE_ID) {
        setTeamPagination((current) => ({
          scopeKey: teamPaginationScopeKey,
          visibleCount:
            (current.scopeKey === teamPaginationScopeKey
              ? current.visibleCount
              : groupVisibleCount) + groupVisibleCount,
        }));
        return true;
      }
      const parsed = parseCloudRemoteItemId(item.id);
      if (!parsed) return false;
      const row = findRow(parsed.rowId);
      // Unpublished / vanished rows swallow the click (no-op).
      if (!row || row.eventsEpoch === undefined) {
        return true;
      }
      // The viewer's own member row of a team conversation (multi-owner
      // families surface own rows in this section): open the LOCAL session
      // directly — replaying a copy of one's own transcript is never right.
      if (
        row.ownerUserId === selfUserId &&
        localOwnSessionIds.has(row.sourceSessionId)
      ) {
        openTeamSessionAtDestination(row, disposition);
        return true;
      }
      // A row already downloading refocuses its tab instead of a dead click;
      // other rows are NOT blocked by someone else's in-flight action.
      const busy = busySessionRows.get(row.id);
      if (busy) {
        if (busy.kind === "replay" && busy.localSessionId) {
          openTeamSessionAtDestination(row, disposition);
        }
        return true;
      }
      openTeamSessionAtDestination(row, disposition);
      return true;
    },
    [
      busySessionRows,
      findRow,
      localOwnSessionIds,
      openTeamSessionAtDestination,
      selfUserId,
      groupVisibleCount,
      teamPaginationScopeKey,
    ]
  );

  const hideRemoteSession = useCallback(
    (row: RemoteTeammateSessionMetadata) => {
      const importedCopies = sessions.filter(
        (session) =>
          session.importedFrom?.orgId === row.orgId &&
          session.importedFrom.sourceSessionId === row.sourceSessionId
      );
      void Promise.all(
        importedCopies.map(async (session) => {
          try {
            await deleteOrgtrackCollaborationSession(session.session_id);
          } catch {
            // Derived blame rows are best-effort cleanup; the session cache
            // deletion below remains the user's primary hide action.
          }
          try {
            await deleteLocalSession(session.session_id);
            removeSession(session.session_id);
          } catch {
            // Hiding the remote row remains useful even when a stale local
            // cache was already removed by another path.
          }
        })
      );
      setHiddenRemoteSessionIds((current) => {
        const next = new Set(current);
        next.add(hiddenRemoteSessionKey(row.orgId, row.id));
        writeHiddenRemoteSessionIds(next);
        return next;
      });
    },
    [sessions]
  );

  // A pin is the viewer's own view state: it never touches the shared cloud
  // row, and two viewers of the same session pin independently.
  const toggleRemoteSessionPin = useCallback((orgId: string, rowId: string) => {
    setPinnedRemoteSessionIds((current) => {
      const next = togglePinnedRemoteSession(current, orgId, rowId);
      writePinnedRemoteSessionIds(next);
      return next;
    });
  }, []);

  const buildRemoteSessionMenuItems = useCallback(
    (row: RemoteTeammateSessionMetadata) => {
      const isPinned = isRemoteSessionPinned(
        pinnedRemoteSessionIds,
        row.orgId,
        row.id
      );
      return buildCloudSessionNativeMenuItems({
        labels: {
          openInNewTab: tCommon("actions.openInNewTab", "Open in New Tab"),
          openInNewWindow: tCommon(
            "actions.openInNewWindow",
            "Open in New Window"
          ),
          openInMyStation: tSessions(
            "controlTower.sidebar.openInMyStation",
            "Open in My Station"
          ),
          copyUrl: t("cloud.sidebar.copyUrl"),
          togglePin: isPinned
            ? tCommon("sessions:chat.unpinSession", "Unpin")
            : tCommon("sessions:chat.pinSession", "Pin"),
          remove: tCommon("actions.remove", "Remove"),
        },
        onOpenInNewTab: () => openTeamSessionAtDestination(row, "new-tab"),
        onOpenInNewWindow: () =>
          openTeamSessionAtDestination(row, "new-window"),
        onOpenInMyStation: () =>
          openTeamSessionAtDestination(row, "my-station"),
        onCopyUrl: () => {
          void copyText(buildCloudSessionReference(row))
            .then(() => {
              Message.success(tCommon("status.copied"));
            })
            .catch(() => {
              Message.error(tCommon("status.copyFailed"));
            });
        },
        onTogglePin: () => toggleRemoteSessionPin(row.orgId, row.id),
        onRemove: () => hideRemoteSession(row),
      });
    },
    [
      hideRemoteSession,
      openTeamSessionAtDestination,
      pinnedRemoteSessionIds,
      t,
      tCommon,
      tSessions,
      toggleRemoteSessionPin,
    ]
  );

  const buildCloudRemoteItemMenuItems = useCallback(
    (item: NavigationMenuItem) => {
      const parsed = parseCloudRemoteItemId(item.id);
      if (!parsed) return [];
      const row = findRow(parsed.rowId);
      if (!row || row.eventsEpoch === undefined) return [];
      return buildRemoteSessionMenuItems(row);
    },
    [buildRemoteSessionMenuItems, findRow]
  );

  const buildRowItem = useCloudSessionRowItemBuilder({
    presenceMap,
    selfUserId,
    t,
    tCommon,
    runFork,
    buildNativeMenuItems: buildRemoteSessionMenuItems,
    busySessionRows,
    pinnedRemoteSessionIds,
    toggleRemoteSessionPin,
  });

  const cloudMenuItems = useCloudTeamSessionMenuItems({
    orgId,
    threads,
    visibleThreads,
    state,
    filter,
    memberMenu,
    setMemberMenu,
    refreshSpinClass,
    handleRefreshClick,
    buildRowItem,
    t,
    tCommon,
  });

  const { cloudRemoteRowMap, cloudRemoteViewerMap } = useCloudRemoteRowMaps({
    visibleThreads,
    presenceMap,
    selfUserId,
  });

  const cloudMemberFilterDropdown = useCloudMemberFilterDropdown({
    orgId,
    filter,
    memberMenu,
    setMemberMenu,
    rows,
    rosterMembers,
    hiddenRemoteSessionIds,
    setHiddenRemoteSessionIds,
    presenceMap,
    onFilterChange,
    t,
  });

  return {
    cloudMenuItems,
    cloudFlatListExcludedSessionIds,
    cloudLocalSessionIds,
    selectedCloudMenuItemId,
    handleCloudSessionItemClick,
    resetCloudTeamPagination,
    buildCloudRemoteItemMenuItems,
    cloudMemberFilterDropdown,
    cloudRemoteRowMap,
    cloudRemoteViewerMap,
  };
}
