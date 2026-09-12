/**
 * SessionsTab Component
 *
 * Displays a list of browser sessions using TreeRowBase for consistent styling.
 * Single-line display with favicon and page title.
 */
import type { BrowserSession } from "@/src/engines/BrowserCore/types";
import React, { memo, useCallback, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { FaviconIcon } from "@src/components/FaviconIcon";
import Input from "@src/components/Input";
import { ToolbarTooltip } from "@src/components/KeyboardShortcut/ToolbarTooltip";
import { Placeholder } from "@src/components/Placeholder";
import { TreeRowBase, type TreeRowNode } from "@src/components/TreeRow";
import { HEADER_BUTTON } from "@src/config/workstation/tokens";
import {
  Cancel01Icon,
  FilterIcon,
  HugeiconsIcon,
  Loading03Icon,
} from "@src/icons";
import {
  NEW_TAB_TITLE,
  isPlaceholderBrowserSessionTitle,
  translatePlaceholderBrowserSessionTitle,
} from "@src/store/workstation/browser/tabs";
import { getSiteNameFromUrl } from "@src/util/url/browserDisplay";

// ============================================
// Types
// ============================================

export interface SessionsTabProps {
  /** List of browser sessions */
  sessions: BrowserSession[];
  /** Currently active session ID */
  activeSessionId: string | null;
  /** Callback to select a session */
  onSelectSession: (sessionId: string) => void;
  /** Callback to close a session */
  onCloseSession: (sessionId: string) => void;
  /** Whether filter input is visible */
  showFilter?: boolean;
}

// ============================================
// Helpers
// ============================================

/**
 * Get display title from session - prefer title, fallback to site name,
 * then the appropriate placeholder sentinel (still in English; callers
 * translate via `translatePlaceholderBrowserSessionTitle`).
 */
function getDisplayTitle(session: BrowserSession): string {
  if (session.title && !isPlaceholderBrowserSessionTitle(session.title)) {
    return session.title;
  }
  if (session.url) {
    return getSiteNameFromUrl(session.url);
  }
  return session.title || NEW_TAB_TITLE;
}

// ============================================
// Session Item Component
// ============================================

interface SessionItemProps {
  session: BrowserSession;
  isActive: boolean;
  onSelect: () => void;
  onClose: (event: React.MouseEvent) => void;
}

const SessionItem: React.FC<SessionItemProps> = memo(
  ({ session, isActive, onSelect, onClose }) => {
    const { t } = useTranslation();
    const rawDisplayTitle = getDisplayTitle(session);
    const displayTitle = translatePlaceholderBrowserSessionTitle(
      rawDisplayTitle,
      t
    );

    const icon = (
      <FaviconIcon
        url={session.url}
        isIncognito={false}
        isSelected={isActive}
      />
    );

    const node: TreeRowNode = {
      id: session.id,
      name: displayTitle,
      path: session.url || "",
      type: "file",
      icon,
    };

    return (
      <TreeRowBase
        node={node}
        depth={0}
        isSelected={isActive}
        onClick={onSelect}
      >
        {/* Loading indicator */}
        {session.isLoading && (
          <HugeiconsIcon
            icon={Loading03Icon}
            data-icon="loader-2"
            size={16}
            strokeWidth={1.75}
            className="shrink-0 animate-spin text-primary-6"
          />
        )}

        {/* Close button (on hover) */}
        <ToolbarTooltip label={t("tooltips.closeSession")}>
          <button
            type="button"
            className={`group/close ${HEADER_BUTTON.danger} hidden shrink-0 group-focus-within/item:flex group-hover/item:flex`}
            onClick={onClose}
            aria-label={t("tooltips.closeSession")}
          >
            <HugeiconsIcon
              icon={Cancel01Icon}
              data-icon="x"
              size={14}
              strokeWidth={1.75}
            />
          </button>
        </ToolbarTooltip>
      </TreeRowBase>
    );
  }
);

SessionItem.displayName = "SessionItem";

// ============================================
// Main Component
// ============================================

export const SessionsTab: React.FC<SessionsTabProps> = memo(
  ({
    sessions,
    activeSessionId,
    onSelectSession,
    onCloseSession,
    showFilter = false,
  }) => {
    const handleCloseSession = useCallback(
      (event: React.MouseEvent, sessionId: string) => {
        event.stopPropagation();
        onCloseSession(sessionId);
      },
      [onCloseSession]
    );

    const { t } = useTranslation();
    const [filterQuery, setFilterQuery] = useState("");

    const displayedSessions = useMemo(() => {
      const normalizedQuery = filterQuery.trim().toLowerCase();
      if (!normalizedQuery) return sessions;

      return sessions.filter((session) => {
        const rawDisplayTitle = getDisplayTitle(session);
        const title = translatePlaceholderBrowserSessionTitle(
          rawDisplayTitle,
          t
        ).toLowerCase();
        const url = (session.url ?? "").toLowerCase();
        return title.includes(normalizedQuery) || url.includes(normalizedQuery);
      });
    }, [sessions, filterQuery, t]);

    return (
      <div className="flex h-full flex-col">
        {showFilter && (
          <div className="shrink-0 px-3 pb-2">
            <Input
              prefix={
                <HugeiconsIcon
                  icon={FilterIcon}
                  data-icon="filter-icon"
                  size={14}
                  strokeWidth={1.75}
                />
              }
              placeholder={t("placeholders.filterByUrl")}
              value={filterQuery}
              onChange={setFilterQuery}
              size="small"
              className="input-pane-surface"
              autoComplete="off"
              autoCorrect="off"
              autoCapitalize="off"
              spellCheck={false}
            />
          </div>
        )}

        <div className="scrollbar-hide min-h-0 flex-1 overflow-y-auto">
          {sessions.length === 0 ? (
            <Placeholder
              variant="empty"
              placement="sidebar"
              title={t("placeholders.noTabs")}
              fillParentHeight
            />
          ) : displayedSessions.length === 0 ? (
            <Placeholder
              variant="no-results"
              placement="sidebar"
              title={t("placeholders.noMatchingResults")}
              fillParentHeight
            />
          ) : (
            displayedSessions.map((session) => (
              <SessionItem
                key={session.id}
                session={session}
                isActive={session.id === activeSessionId}
                onSelect={() => onSelectSession(session.id)}
                onClose={(event) => handleCloseSession(event, session.id)}
              />
            ))
          )}
        </div>
      </div>
    );
  }
);

SessionsTab.displayName = "SessionsTab";

export default SessionsTab;
