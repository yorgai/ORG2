/**
 * Builds one `NavigationMenuItem` row for a Team Sessions fork thread
 * (`cloudSessionsSection.tsx`): icon/title/relative-time, the unresolved
 * comments badge, live-viewer chips, and the row's hover actions (Fork,
 * pin, and the canonical Team Conversation menu). Split out because it is the single
 * largest piece of that section's row-construction logic.
 */
import type { TFunction } from "i18next";
import { useAtomValue } from "jotai";
import { useCallback } from "react";

import PersonAvatar from "@src/components/PersonAvatar";
import { dismissHoverCard } from "@src/components/SessionHoverCard/singletonStore";
import { resolveAgentIcon } from "@src/config/agentIcons";
import {
  discussionSeenCountsAtom,
  discussionSeenKey,
  unreadDiscussionCount,
} from "@src/features/Org2Cloud/SessionConversation/discussionSeenAtom";
import { isRemoteSessionPinned } from "@src/features/Org2Cloud/cloudPinnedRemoteSessions";
import { buildCloudRemoteItemId } from "@src/features/Org2Cloud/cloudRemoteItemId";
import type { CloudSessionBusyEntry } from "@src/features/Org2Cloud/cloudSessionBusyAtom";
import {
  cloudDownloadEtaMs,
  cloudDownloadPercent,
  formatCloudDownloadEta,
} from "@src/features/Org2Cloud/cloudSessionDownloadProgressAtom";
import { buildCloudSessionReference } from "@src/features/Org2Cloud/cloudSessionReference";
import {
  type CloudSessionThreadRow,
  isCloudThreadRowDisabled,
} from "@src/features/Org2Cloud/cloudSessionThreads";
import type { Org2CloudPresenceEntry } from "@src/features/Org2Cloud/org2CloudPresenceAtom";
import { viewersForSession } from "@src/features/Org2Cloud/org2CloudPresenceAtom";
import { useCloudSessionDownloadProgressEntry } from "@src/features/Org2Cloud/useCloudSessionDownloadSurface";
import {
  GitForkIcon,
  HugeiconsIcon,
  Loading03Icon,
  MoreHorizontalIcon,
  PinIcon,
  PinOffIcon,
} from "@src/icons";
import type { NavigationMenuItem } from "@src/scaffold/NavigationSidebar/components/NavigationMenu/config";
import type { RemoteTeammateSessionMetadata } from "@src/store/collaboration/types";
import {
  type NativeMenuItemOptions,
  popupNativeMenu,
} from "@src/util/platform/tauri/nativeMenuPopup";
import { resolveSessionDisplayMetadata } from "@src/util/session/sessionDisplayMetadata";
import { formatCompactAge } from "@src/util/time/formatRelativeTime";

const RowBusyIndicator: React.FC<{
  t: TFunction;
  bareSessionId: string;
  localSessionId: string | undefined;
}> = ({ t, bareSessionId, localSessionId }) => {
  const progress = useCloudSessionDownloadProgressEntry(localSessionId);
  const percent = progress ? cloudDownloadPercent(progress) : null;
  const etaMs = progress ? cloudDownloadEtaMs(progress) : null;
  const busyLabel =
    percent === null
      ? t("cloud.sidebar.loadingSession")
      : etaMs === null
        ? t("cloud.sidebar.downloadProgress", { percent })
        : t("cloud.sidebar.downloadProgressEta", {
            percent,
            eta: formatCloudDownloadEta(etaMs),
          });
  return (
    <span
      className="inline-flex items-center gap-1"
      role="img"
      aria-label={busyLabel}
      title={busyLabel}
      data-testid={`cloud-session-row-busy-${bareSessionId}`}
    >
      {percent !== null && (
        <span className="text-[9px] leading-none font-medium text-text-3 tabular-nums">
          {percent}%
        </span>
      )}
      <HugeiconsIcon
        icon={Loading03Icon}
        data-icon="loader-2"
        aria-hidden="true"
        className="size-3.5 animate-spin text-text-3"
      />
    </span>
  );
};

interface UseCloudSessionRowItemBuilderParams {
  presenceMap: Record<string, Record<string, Org2CloudPresenceEntry>>;
  selfUserId: string | null;
  t: TFunction;
  tCommon: TFunction;
  runFork: (row: RemoteTeammateSessionMetadata) => void;
  buildNativeMenuItems: (
    row: RemoteTeammateSessionMetadata
  ) => NativeMenuItemOptions[];
  /** Per-row in-flight replay/fork registry — busy rows render a spinner. */
  busySessionRows: ReadonlyMap<string, CloudSessionBusyEntry>;
  /** Viewer-local pin keys (`<orgId>|<rowId>`); never a property of the shared row. */
  pinnedRemoteSessionIds: ReadonlySet<string>;
  toggleRemoteSessionPin: (orgId: string, rowId: string) => void;
}

export type BuildCloudSessionRowItem = (
  threadRow: CloudSessionThreadRow,
  /** Family members folded into this row (badge aggregation only). */
  familyDescendants?: readonly CloudSessionThreadRow[]
) => NavigationMenuItem;

export function useCloudSessionRowItemBuilder({
  presenceMap,
  selfUserId,
  t,
  tCommon,
  runFork,
  buildNativeMenuItems,
  busySessionRows,
  pinnedRemoteSessionIds,
  toggleRemoteSessionPin,
}: UseCloudSessionRowItemBuilderParams): BuildCloudSessionRowItem {
  const seenCounts = useAtomValue(discussionSeenCountsAtom);
  const buildRowItem = useCallback(
    (
      threadRow: CloudSessionThreadRow,
      familyDescendants?: readonly CloudSessionThreadRow[]
    ) => {
      const { row, bareSessionId } = threadRow;
      const isFork = Boolean(row.forkedFrom);
      const disabled = isCloudThreadRowDisabled(threadRow);
      const itemId = buildCloudRemoteItemId(row.orgId, row.id);
      // Keep sidebar rows in bare compact form ("2m"/"2h"/"2d", no "ago")
      // regardless of the app's display language (see menuItemBuilders.tsx
      // for the same choice on local sessions).
      const relativeTime = row.lastActivityAt
        ? formatCompactAge(row.lastActivityAt)
        : "";
      const display = resolveSessionDisplayMetadata({
        kind: "remote",
        session: row,
      });
      const sessionIcon =
        isFork && !display.externalSource && !display.agentType
          ? GitForkIcon
          : resolveAgentIcon(display.agentIconId);
      // Unread discussion messages: the 0014 live-comment counters minus the
      // viewer-local seen watermarks (stamped while the conversation is
      // open), so the badge clears once the viewer has read the discussion.
      // Summed across the WHOLE family — the fold renders one row per
      // conversation, and a comment sitting on any member's plane must
      // still light that single row. Suppress the badge on rows the viewer
      // cannot open: a disabled teammate metadata_only row (eventsEpoch ===
      // undefined) has no reachable conversation surface, so advertising a
      // count the viewer cannot read is a pure dead end.
      const unreadComments = disabled
        ? 0
        : [threadRow, ...(familyDescendants ?? [])].reduce(
            (sum, member) =>
              sum +
              unreadDiscussionCount(
                member.row.commentCount,
                seenCounts[
                  discussionSeenKey(member.row.orgId, member.bareSessionId)
                ]
              ),
            0
          );
      const commentsBadge =
        unreadComments > 0 ? (
          <span
            data-testid="session-comments-badge"
            aria-label={t("cloud.comments.unreadBadge", {
              count: unreadComments,
            })}
            className="inline-flex h-3.5 min-w-3.5 items-center justify-center rounded-full bg-primary-6 px-1 text-[9px] leading-none font-medium text-white"
          >
            {unreadComments}
          </span>
        ) : undefined;
      // Live viewers: other org members currently viewing this session.
      const viewers = viewersForSession(
        presenceMap,
        row.orgId,
        bareSessionId,
        selfUserId
      );
      const overflowViewers = viewers.slice(3);
      const viewerChips =
        viewers.length > 0 ? (
          <span className="inline-flex items-center -space-x-1">
            {viewers.slice(0, 3).map((viewer) => (
              <span
                key={viewer.userId}
                data-testid="session-viewer-chip"
                aria-label={t("cloud.sidebar.viewerTooltip", {
                  name: viewer.displayName,
                })}
                title={t("cloud.sidebar.viewerTooltip", {
                  name: viewer.displayName,
                })}
                className="inline-flex rounded-full ring-1 ring-bg-1"
              >
                <PersonAvatar name={viewer.displayName} size={14} />
              </span>
            ))}
            {overflowViewers.length > 0 && (
              <span
                data-testid="session-viewer-overflow"
                title={`${t("cloud.sidebar.viewerOverflow", {
                  count: overflowViewers.length,
                })}\n${overflowViewers
                  .map((viewer) => viewer.displayName)
                  .join(", ")}`}
                className="inline-flex h-3.5 min-w-3.5 items-center justify-center rounded-full bg-fill-3 px-0.5 text-[8px] leading-none font-semibold text-text-2 ring-1 ring-bg-1"
              >
                +{overflowViewers.length}
              </span>
            )}
          </span>
        ) : undefined;
      // In-flight replay/fork: spinner + live percent in the trailing slot.
      // Without this the shared busy registry would manifest as nothing but
      // an unresponsive row. The indicator subscribes to its own session's
      // progress slice so ticks re-render one row, not the whole menu.
      const busy = busySessionRows.get(row.id);
      const busyIndicator = busy ? (
        <RowBusyIndicator
          t={t}
          bareSessionId={bareSessionId}
          localSessionId={busy.localSessionId}
        />
      ) : undefined;
      const isPinned = isRemoteSessionPinned(
        pinnedRemoteSessionIds,
        row.orgId,
        row.id
      );
      const pinIndicator = isPinned ? (
        <HugeiconsIcon
          icon={PinIcon}
          data-icon="pin"
          size={11}
          strokeWidth={2}
          className="shrink-0 text-text-3"
          aria-label="Pinned"
        />
      ) : null;
      const trailingElement =
        pinIndicator || busyIndicator || viewerChips || commentsBadge ? (
          <span className="inline-flex items-center gap-1">
            {pinIndicator}
            {busyIndicator}
            {viewerChips}
            {commentsBadge}
          </span>
        ) : undefined;
      // Strip fork glyph(s) baked into pushed titles; the GitFork icon carries provenance.
      const displayTitle = row.title.replace(/^(?:⑂\s*)+/u, "");
      const item: NavigationMenuItem = {
        id: itemId,
        key: itemId,
        label: displayTitle,
        dataTestId: `sidebar-cloud-session-item-${bareSessionId}`,
        opensChatPanelTab: true,
        pinned: isPinned,
        // Prefer the source/agent brand used by regular sessions. Cloud
        // scope is context, not the session's icon identity.
        icon: sessionIcon,
        shortcut: relativeTime,
        trailingElement,
        disabled,
      };
      if (!disabled) {
        item.showMoreActions = true;
        // Teammate rows carry the whole (org, owner, session) tuple, so a
        // drag onto a text surface can insert the reference verbatim — no
        // local push marker exists for someone else's session to resolve
        // an org from.
        item.dragPayload = {
          path: buildCloudSessionReference(row),
          name: displayTitle,
          iconType: "session",
          dragSubtitle: row.ownerDisplayName,
        };
      }
      if (!disabled) {
        // Remote rows open/replay on plain click. Hover adds Fork plus the
        // standard overflow menu, whether this row is a leaf or thread root.
        item.rowActions = [
          {
            icon: GitForkIcon,
            label: t("cloud.orgPanel.fork"),
            onClick: () => runFork(row),
          },
          // One click on hover, matching a local row: a teammate's session is
          // pinned often enough that burying it in the overflow menu is a tax.
          {
            icon: isPinned ? PinOffIcon : PinIcon,
            label: isPinned
              ? tCommon("sessions:chat.unpinSession", "Unpin")
              : tCommon("sessions:chat.pinSession", "Pin"),
            onClick: () => toggleRemoteSessionPin(row.orgId, row.id),
          },
          {
            icon: MoreHorizontalIcon,
            label: tCommon("actions.more"),
            onClick: () => {
              dismissHoverCard();
              void popupNativeMenu({
                source: "cloud-session-row",
                buildItems: () => buildNativeMenuItems(row),
              });
            },
          },
        ];
      }
      return item;
    },
    [
      busySessionRows,
      buildNativeMenuItems,
      pinnedRemoteSessionIds,
      toggleRemoteSessionPin,
      presenceMap,
      runFork,
      seenCounts,
      selfUserId,
      t,
      tCommon,
    ]
  );

  return buildRowItem;
}
