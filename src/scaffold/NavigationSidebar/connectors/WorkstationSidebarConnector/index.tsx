import { useAtomValue, useSetAtom } from "jotai";
import React, { useCallback, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useLocation, useNavigate } from "react-router-dom";

import Message from "@src/components/Message";
import { ROUTES } from "@src/config/routes";
import { useAppNavigation } from "@src/hooks/navigation/useAppNavigation";
import { useSessionView } from "@src/hooks/ui/tabs/useSessionView";
import { teamInboxUnreadCountAtom } from "@src/modules/MainApp/TeamInbox/store";
import { useTeamInboxDataSource } from "@src/modules/MainApp/TeamInbox/useTeamInboxDataSource";
import { openAgentSessionSearchSpotlight } from "@src/scaffold/GlobalSpotlight/openSpotlight";
import {
  activeSessionCreatorDraftIdAtom,
  deleteSessionCreatorDraftAtom,
  promoteActiveSessionCreatorDraftAtom,
  sessionCreatorDraftListAtom,
  sessionLoadingAtom,
  sessionPaginationAtom,
  sessionsAtom,
  visitedSessionsAtom,
  workstationActiveSessionIdAtom,
} from "@src/store/session";
import {
  clearSessionSidebarRevealAtom,
  sessionSidebarRevealRequestAtom,
  sidebarCollapsedAtom,
} from "@src/store/ui/sidebarAtom";

import { SidebarBottomBar } from "../../blocks";
import SidebarSettingsMenuButton from "../../blocks/SidebarSettingsMenuButton";
import NavigationSidebar from "../../variants/NavigationSidebar";
import SidebarAccountButton from "../SidebarAccountButton";
import { NEW_SESSION_MENU_ITEM_ID } from "../sidebarConnectorUtils";
import type { SidebarTabDisposition } from "../sidebarTabNavigation";
import { useSessionMenuItems } from "../useSessionMenuItems/index";
import { DEFAULT_COLLAPSED_SECTION_IDS } from "../workstationSidebarData";
import { SessionSidebarViewSwitcher } from "./SessionSidebarViewSwitcher";
import { SidebarDialogs } from "./SidebarDialogs";
import { openNewChatFromSidebar } from "./sessionEntryActions";
import { useWorkstationSidebarBottomActions } from "./sidebarConnector.bottomActions";
import { useWorkstationSidebarChatPanelAtoms } from "./sidebarConnector.chatPanelAtoms";
import { useWorkstationSidebarChrome } from "./sidebarConnector.chrome";
import { useWorkstationSidebarCloudMenuData } from "./sidebarConnector.cloudMenuData";
import { buildWorkstationSidebarLabels } from "./sidebarConnector.labels";
import { useWorkstationSidebarPinnedAndRevealData } from "./sidebarConnector.pinnedAndRevealData";
import { useWorkstationSidebarRevealNavigationEffects } from "./sidebarConnector.revealNavigationEffects";
import { useWorkstationSidebarRevealRequestState } from "./sidebarConnector.revealRequestState";
import { useWorkstationSidebarScopeAndPagination } from "./sidebarConnector.scopeAndPagination";
import { useWorkstationSidebarSelectionAndCollapse } from "./sidebarConnector.selectionAndCollapse";
import { useWorkstationSidebarSessionInteractionHandlers } from "./sidebarConnector.sessionInteractionHandlers";
import { useSidebarSessionRefreshAction } from "./sidebarSessionRefresh";
import type { SessionSidebarView } from "./types";
import { useMobileSidebarSessions } from "./useMobileSidebarSessions";
import { useSessionSidebarOrdering } from "./useSessionSidebarOrdering";
import { useSessionSidebarRowActions } from "./useSessionSidebarRowActions";
import { useSidebarStationNavigation } from "./useSidebarStationNavigation";
import { useWorkItemsSidebarSurface } from "./useWorkItemsSidebarSurface";
import { useWorkspaceGroupActions } from "./useWorkspaceGroupActions";

/**
 * Owns organization scope, cross-surface reveal/selection, and shared sidebar chrome.
 * Work-item state/actions, channel scope composition, session row actions/dialogs,
 * workflows have dedicated owners. Every controller remains mounted
 * with this connector; switching views only changes the existing visibility gates.
 */
export const WorkstationSidebarConnector: React.FC = () => {
  const { t } = useTranslation("navigation");
  const { t: tProjects } = useTranslation("projects");
  const { t: tSessions } = useTranslation("sessions");
  const { t: tCommonRaw } = useTranslation();
  const tCommon = useCallback(
    (key: string, defaultValue?: string) => tCommonRaw(key, { defaultValue }),
    [tCommonRaw]
  );
  const location = useLocation();
  const navigate = useNavigate();
  const sessions = useAtomValue(sessionsAtom);
  useTeamInboxDataSource();
  const teamInboxUnreadCount = useAtomValue(teamInboxUnreadCountAtom);
  const sessionsLoading = useAtomValue(sessionLoadingAtom);
  const sessionPagination = useAtomValue(sessionPaginationAtom);
  const sessionSidebarRevealRequest = useAtomValue(
    sessionSidebarRevealRequestAtom
  );
  const clearSessionSidebarReveal = useSetAtom(clearSessionSidebarRevealAtom);
  const setSidebarCollapsed = useSetAtom(sidebarCollapsedAtom);
  const visitedSessions = useAtomValue(visitedSessionsAtom);
  const sessionCreatorDrafts = useAtomValue(sessionCreatorDraftListAtom);
  const activeSessionCreatorDraftId = useAtomValue(
    activeSessionCreatorDraftIdAtom
  );
  const promoteActiveSessionCreatorDraft = useSetAtom(
    promoteActiveSessionCreatorDraftAtom
  );
  const deleteSessionCreatorDraft = useSetAtom(deleteSessionCreatorDraftAtom);
  const { refreshSpinClass, handleRefreshSessions } =
    useSidebarSessionRefreshAction();

  const {
    chatPanelContentMode,
    chatPanelCreateTarget,
    chatPanelSelectedWorkItem,
    chatPanelSelectedProject,
    setChatPanelCreateTarget,
    resetChatPanelSessionSurface,
    setStationChatVisible,
    setStationMode,
    activeWorkManagementSection,
    workManagementProjectsView,
    setWorkManagementProjectsView,
    openWorkManagementTab,
    openOrganizationTab,
    openSessionInNewChatTab,
    openSessionInWorkstation,
    openSessionInNewWindow,
    openOrReplaceSessionInChatPanelTab,
    activateChatPanelTab,
    openStartPageTab,
    openRuntimeTab,
    openTeamInboxTab,
    closeAndDestroyChatPanelTab,
    closeOtherThanActiveChatPanelTabs,
  } = useWorkstationSidebarChatPanelAtoms();

  const { openSession } = useSessionView();
  const activeSessionId = useAtomValue(workstationActiveSessionIdAtom) ?? "";
  const { goToNewSession, navigateTo } = useAppNavigation();
  const [activeViewKey, setActiveViewKey] =
    useState<SessionSidebarView>("sessions");
  const workItemsContentVisible = activeViewKey === "work-items";
  const channelSidebarVisible = activeViewKey === "channels";

  const {
    sortedSessions,
    activeCloudOrgId,
    activeOrgId,
    activeProjectOrgId,
    cloudSessionFilter,
    cloudTaggedSessionIds,
    handleCloudSessionFilterChange,
    manageableCloudOrg,
    manageableLocalOrg,
    orgSelectorLoading,
    orgSelectorOptions,
    personalHiddenCloudTaggedIds,
    sessionFilterOrgIds,
    setSelectedOrgId,
    repoPathToName,
    groupByMode,
    setGroupByMode,
    groupVisibleCount,
    setGroupVisibleCount,
    includeExternal,
    setIncludeExternal,
    cloudMyPaginationScopeKey,
    cloudMySessionsVisibleCount,
    setCloudMyPagination,
    resetCloudMyPagination,
    cloudSignedInAvatarUrl,
    cloudSignedInIdentity,
    handleCloudSignIn,
  } = useWorkstationSidebarScopeAndPagination({ sessions });

  const [groupVisibleCounts, setGroupVisibleCounts] = useState<
    Map<string, number>
  >(new Map());
  const [expandedSubagentParentIds, setExpandedSubagentParentIds] = useState<
    Set<string>
  >(() => new Set());
  const [collapsedSectionIds, setCollapsedSectionIds] = useState<Set<string>>(
    () => new Set(DEFAULT_COLLAPSED_SECTION_IDS)
  );

  const { activeSessionSidebarRevealRequest, revealedSessionIds } =
    useWorkstationSidebarRevealRequestState({
      sessionSidebarRevealRequest,
      activeSessionId,
      clearSessionSidebarReveal,
    });

  const {
    untitledSession,
    newSessionLabel,
    pinFolderLabel,
    unpinFolderLabel,
    createProjectLabel,
    createWorkItemLabel,
    runtimeLabel,
    teamInboxLabel,
    importGithubIssuesLabel,
    addOrgLabel,
    manageOrgLabel,
    moreActionsLabel,
    pinWorkspaceLabel,
    unpinWorkspaceLabel,
    hideWorkspaceLabel,
    unhideWorkspaceLabel,
    revealWorkspaceLabel,
    workspaceUnavailableTitle,
    workspaceUnavailableMessage,
  } = buildWorkstationSidebarLabels({ t, tProjects, tSessions, tCommon });

  // Same entry point as the sidebar's own "+ New session", so a workspace
  // header `+` lands the user on the identical surface — it only pre-seeds
  // the creator's source with that workspace first.
  const openNewSessionFromSidebar = useCallback(() => {
    openNewChatFromSidebar({
      goToNewSession,
      resetChatPanelSessionSurface,
      openNewChatTab: () => openStartPageTab({ title: t("routes.launchpad") }),
      setChatPanelCreateTarget,
    });
  }, [
    goToNewSession,
    resetChatPanelSessionSurface,
    openStartPageTab,
    setChatPanelCreateTarget,
    t,
  ]);

  const workspaceGroupActions = useWorkspaceGroupActions({
    createSessionLabel: newSessionLabel,
    moreActionsLabel,
    pinLabel: pinWorkspaceLabel,
    unpinLabel: unpinWorkspaceLabel,
    hideLabel: hideWorkspaceLabel,
    unhideLabel: unhideWorkspaceLabel,
    revealLabel: revealWorkspaceLabel,
    unavailableTitle: workspaceUnavailableTitle,
    unavailableMessage: workspaceUnavailableMessage,
    openNewSession: openNewSessionFromSidebar,
    setCollapsedSectionIds,
  });

  const openCloudSessionAtDestination = useCallback(
    (
      destination: SidebarTabDisposition | "my-station" | "new-window",
      options: { sessionId: string; title: string }
    ) => {
      if (destination === "new-window") {
        void openSessionInNewWindow(options).catch((error) => {
          Message.error(error instanceof Error ? error.message : String(error));
        });
        return;
      }

      setStationMode("my-station");
      setStationChatVisible("my-station", true);
      if (location.pathname !== ROUTES.workStation.code.path) {
        navigate(ROUTES.workStation.code.path);
      }

      if (destination === "new-tab") {
        resetChatPanelSessionSurface();
        openSessionInNewChatTab({
          sessionId: options.sessionId,
          sessionName: options.title,
        });
        return;
      }

      if (destination === "default" || destination === "replace-all") {
        resetChatPanelSessionSurface();
        openOrReplaceSessionInChatPanelTab({
          sessionId: options.sessionId,
          sessionName: options.title,
        });
        if (destination === "replace-all") {
          void closeOtherThanActiveChatPanelTabs();
        }
        return;
      }

      openSessionInWorkstation({
        sessionId: options.sessionId,
        title: options.title,
      });
    },
    [
      location.pathname,
      navigate,
      resetChatPanelSessionSurface,
      openSessionInNewChatTab,
      openSessionInNewWindow,
      openSessionInWorkstation,
      openOrReplaceSessionInChatPanelTab,
      closeOtherThanActiveChatPanelTabs,
      setStationChatVisible,
      setStationMode,
    ]
  );

  const {
    cloudMenuItems,
    cloudSessionMenuItems,
    channelMenuItems,
    selectedCloudMenuItemId,
    handleCloudSessionItemClick,
    resetCloudTeamPagination,
    buildCloudRemoteItemMenuItems,
    cloudMemberFilterDropdown,
    cloudRemoteRowMap,
    cloudRemoteViewerMap,
    sessionListExcludedIds,
    cloudScopedExtraSessionIds,
    cloudChannelsDialogs,
    localChannelsDialogs,
  } = useWorkstationSidebarCloudMenuData({
    activeCloudOrgId,
    sessions,
    cloudSessionFilter,
    activeSessionId,
    cloudMySessionsVisibleCount,
    groupVisibleCount,
    revealedCloudOrgId: activeSessionSidebarRevealRequest?.cloudOrgId,
    revealedSidebarItemId: activeSessionSidebarRevealRequest?.sidebarItemId,
    openSessionAtDestination: openCloudSessionAtDestination,
    handleCloudSessionFilterChange,
    personalHiddenCloudTaggedIds,
    cloudTaggedSessionIds,
  });

  const {
    menuItems,
    sessionMap,
    subagentParentIds,
    isLoadMoreId,
    getLoadMoreGroupId,
  } = useSessionMenuItems({
    sortedSessions,
    visitedSessions,
    repoPathToName,
    groupByMode,
    untitledSession,
    selectedOrgIds: sessionFilterOrgIds,
    extraSessionIds: cloudScopedExtraSessionIds,
    excludedSessionIds: sessionListExcludedIds,
    includeExternal,
    groupVisibleCounts,
    defaultGroupVisibleCount: groupVisibleCount,
    showAllLoadedGroupSessions: Boolean(activeCloudOrgId),
    expandedSubagentParentIds,
    revealedSessionIds,
    workspaceGroupActions,
  });

  const {
    rename,
    activeChatPanelTab,
    highlightedSessionId,
    pinnedMenuItems,
    sessionSidebarMenuItems,
    loadedCloudMySessionRowCount,
    revealCandidateMenuItems,
  } = useWorkstationSidebarPinnedAndRevealData({
    activeSessionId,
    cloudMenuItems,
    menuItems,
    sessionCreatorDrafts,
    activeViewKey,
    sessionSearchLabel: t("sidebar.search.sessions"),
    sessionRefreshLabel: tCommon("actions.refresh"),
    sessionRefreshIconClassName: refreshSpinClass,
    onSessionSearch: openAgentSessionSearchSpotlight,
    onSessionRefresh: handleRefreshSessions,
    createProjectLabel,
    createWorkItemLabel,
    importGithubIssuesLabel,
    newSessionLabel,
    runtimeLabel,
    teamInboxLabel,
    teamInboxUnreadCount,
    t,
    tSessions,
  });

  const { activateMyStationRouteForProjectTabContent, handleGoToNewSession } =
    useSidebarStationNavigation({
      setStationMode,
      setStationChatVisible,
      openStartPageTab,
      resetChatPanelSessionSurface,
      setChatPanelCreateTarget,
      goToNewSession,
      location,
      navigate,
      t,
    });

  const {
    handleDeleteSession,
    handleExportMarkdown,
    handleMenuItemClick,
    handleTogglePin,
    handleOpenInNewTab,
    handleOpenInMyStation,
    handleOpenInNewWindow,
    handleOpenLinkedWorkItemSession,
    handleToggleSubagentExpansion,
  } = useWorkstationSidebarSessionInteractionHandlers({
    handleCloudSessionItemClick,
    cloudMySessionsVisibleCount,
    cloudMyPaginationScopeKey,
    setCloudMyPagination,
    loadedCloudMySessionRowCount,
    sessionPagination,
    activeSessionId,
    sessionMap,
    isLoadMoreId,
    getLoadMoreGroupId,
    sessionRouteLabel: t("routes.session"),
    handleGoToNewSession,
    navigateTo,
    openSession,
    promoteActiveSessionCreatorDraft,
    groupByMode,
    defaultGroupVisibleCount: groupVisibleCount,
    setGroupVisibleCounts,
    tCommon,
    activateChatPanelTab,
    openOrReplaceSessionInChatPanelTab,
    closeAndDestroyChatPanelTab,
    activateMyStationRouteForProjectTabContent,
    resetChatPanelSessionSurface,
    openSessionInNewChatTab,
    openSessionInWorkstation,
    openSessionInNewWindow,
    setExpandedSubagentParentIds,
  });

  const {
    moveToOrg,
    cloudSyncLevel,
    cloudShare,
    handleMenuItemContextMenu,
    menuItems: sessionMenuItems,
  } = useSessionSidebarRowActions({
    sessionMap,
    rename,
    handleDeleteSession,
    deleteSessionCreatorDraft,
    handleOpenDraftInNewTab: (item) =>
      handleMenuItemClick(item.key, item, "new-tab"),
    handleExportMarkdown,
    handleOpenInNewTab,
    handleOpenInNewWindow,
    handleOpenInMyStation,
    handleTogglePin,
    handleToggleSubagentExpansion,
    buildCloudRemoteItemMenuItems,
    t,
    tCommon,
    expandedSubagentParentIds,
    pinFolderLabel,
    unpinFolderLabel,
    subagentParentIds,
    cloudSessionMenuItems,
    sessionSidebarMenuItems,
    cloudMySessionsVisibleCount,
  });

  useMobileSidebarSessions({
    scope: activeOrgId,
    loading: sessionsLoading || orgSelectorLoading,
    items: sessionMenuItems,
    sessionMap,
    repoPathToName,
  });

  const workItems = useWorkItemsSidebarSurface({
    enabled: workItemsContentVisible,
    activeProjectOrgId,
    activateMyStationRouteForProjectTabContent,
    handleOpenLinkedWorkItemSession,
  });
  const { selectedMenuItemId, handleSessionCollapsedSectionIdsChange } =
    useWorkstationSidebarSelectionAndCollapse({
      activeSessionCreatorDraftId,
      highlightedSessionId,
      activeViewKey,
      activeChatPanelTabType: activeChatPanelTab?.type ?? null,
      chatPanelContentMode,
      chatPanelCreateTarget,
      chatPanelSelectedProject,
      chatPanelSelectedWorkItem,
      projectsSelectedMenuItemId: workItems.selectedMenuItemId,
      sessionCreatorDrafts,
      activeWorkManagementSection,
      workManagementProjectsView,
      setGroupVisibleCounts,
      collapsedSectionIds,
      groupByMode,
      resetCloudTeamPagination,
      resetCloudMyPagination,
      setCollapsedSectionIds,
    });

  const sidebarMenuItems = workItemsContentVisible
    ? workItems.menuItems
    : channelSidebarVisible
      ? channelMenuItems
      : sessionMenuItems;
  const sidebarScrollLayout = useMemo(() => {
    if (activeViewKey !== "sessions") {
      return { pinnedMenuItems, menuItems: sidebarMenuItems };
    }
    return {
      pinnedMenuItems: pinnedMenuItems.filter(
        (item) => item.id === NEW_SESSION_MENU_ITEM_ID
      ),
      menuItems: [
        ...pinnedMenuItems.filter(
          (item) => item.id !== NEW_SESSION_MENU_ITEM_ID
        ),
        ...sidebarMenuItems,
      ],
    };
  }, [activeViewKey, pinnedMenuItems, sidebarMenuItems]);
  const resolvedCollapsedSectionIds = workItemsContentVisible
    ? workItems.collapsedSectionIds
    : collapsedSectionIds;
  const resolvedOnCollapsedSectionIdsChange = workItemsContentVisible
    ? workItems.onCollapsedSectionIdsChange
    : handleSessionCollapsedSectionIdsChange;

  useWorkstationSidebarRevealNavigationEffects({
    sessionSidebarRevealRequest,
    setSidebarCollapsed,
    setActiveViewKey,
    setSelectedOrgId,
    setExpandedSubagentParentIds,
    activeSessionSidebarRevealRequest,
    revealCandidateMenuItems,
    setCollapsedSectionIds,
  });

  const {
    sidebarOrgSelector,
    resolvedMenuItemClick,
    resolvedMenuItemContextMenu,
    resolvedRenderMenuItemWrapper,
  } = useWorkstationSidebarChrome({
    activeOrgId,
    orgSelectorOptions,
    orgSelectorLoading,
    addOrgLabel,
    cloudSignedIn: cloudSignedInIdentity !== null,
    manageOrgLabel,
    handleCloudSignIn,
    activeViewKey,
    handleMenuItemContextMenu,
    activateMyStationRouteForProjectTabContent,
    t,
    setSelectedOrgId,
    activeCloudOrgId,
    manageableCloudOrg,
    manageableLocalOrg,
    openOrganizationTab,
    sessionMap,
    cloudRemoteRowMap,
    cloudRemoteViewerMap,
    renderProjectsMenuItemWrapper: workItems.renderMenuItemWrapper,
    tSessions,
    setWorkManagementProjectsView,
    openWorkManagementTab,
    openRuntimeTab,
    runtimeLabel,
    openTeamInboxTab,
    activateChatPanelTab,
    handleMenuItemClick,
    handleProjectsMenuItemClick: workItems.onMenuItemClick,
    handleOpenInNewTab,
    closeOtherThanActiveChatPanelTabs,
    tCommon,
  });

  const { isLoading, sidebarBottomRightActions, resolvedSelectedMenuItemId } =
    useWorkstationSidebarBottomActions({
      sidebarMenuItems,
      resolvedOnCollapsedSectionIdsChange,
      sessions,
      activeViewKey,
      projectsWorkItemsLoading: workItems.loading,
      projectsSidebarMenuItems: workItems.menuItems,
      sessionsLoading,
      handleRefreshSessions,
      openRuntimeTab,
      runtimeLabel,
      groupByMode,
      groupVisibleCount,
      includeExternal,
      setGroupByMode,
      setGroupVisibleCount,
      setIncludeExternal,
      setGroupVisibleCounts,
      resetCloudTeamPagination,
      resetCloudMyPagination,
      selectedCloudMenuItemId,
      selectedMenuItemId,
      activeSessionId,
      collapsedSectionIds,
      pinnedMenuItems,
    });

  const ordering = useSessionSidebarOrdering({
    enabled: activeViewKey === "sessions",
    items: sidebarMenuItems,
    sessionMap,
    onTogglePin: handleTogglePin,
  });
  const wrapOrderedRow = ordering.wrap;
  const renderOrderedMenuItem = useCallback(
    (
      item: Parameters<NonNullable<typeof resolvedRenderMenuItemWrapper>>[0],
      node: React.ReactElement
    ) =>
      wrapOrderedRow(
        item,
        resolvedRenderMenuItemWrapper
          ? resolvedRenderMenuItemWrapper(item, node)
          : node
      ),
    [wrapOrderedRow, resolvedRenderMenuItemWrapper]
  );

  return (
    <>
      <NavigationSidebar
        menuItems={sidebarScrollLayout.menuItems}
        pinnedMenuItems={sidebarScrollLayout.pinnedMenuItems}
        selectedKey={resolvedSelectedMenuItemId}
        onMenuItemClick={resolvedMenuItemClick}
        onMenuItemContextMenu={resolvedMenuItemContextMenu}
        renderMenuItemWrapper={renderOrderedMenuItem}
        topBarFollowingContent={
          <div className="shrink-0 px-3 pt-1">{sidebarOrgSelector}</div>
        }
        preListContent={
          <SessionSidebarViewSwitcher
            activeKey={activeViewKey}
            onChange={setActiveViewKey}
          />
        }
        listTopPadding={activeViewKey === "sessions" ? "row" : true}
        bottomContent={
          <>
            {ordering.unpinDropZone}
            <SidebarBottomBar
              leftContent={
                <SidebarSettingsMenuButton
                  onSignIn={
                    cloudSignedInIdentity === null
                      ? handleCloudSignIn
                      : undefined
                  }
                  renderTrigger={({ isOpen, onClick }) => (
                    <SidebarAccountButton
                      identity={cloudSignedInIdentity}
                      avatarUrl={cloudSignedInAvatarUrl}
                      menuOpen={isOpen}
                      onClick={onClick}
                    />
                  )}
                />
              }
              rightActions={sidebarBottomRightActions}
            />
          </>
        }
        isLoading={isLoading}
        collapsibleSections
        collapsedSectionIds={resolvedCollapsedSectionIds}
        onCollapsedSectionsChange={resolvedOnCollapsedSectionIdsChange}
        revealMenuItemRequest={
          activeSessionSidebarRevealRequest
            ? {
                key:
                  activeSessionSidebarRevealRequest.sidebarItemId ??
                  activeSessionSidebarRevealRequest.sessionId,
                requestId: activeSessionSidebarRevealRequest.requestId,
              }
            : undefined
        }
      />
      {ordering.insertionLine}
      <SidebarDialogs
        cloudChannelsDialogs={cloudChannelsDialogs}
        localChannelsDialogs={localChannelsDialogs}
        cloudMemberFilterDropdown={cloudMemberFilterDropdown}
        cloudShare={cloudShare}
        cloudSyncLevel={cloudSyncLevel}
        moveToOrg={moveToOrg}
        rename={rename}
        sessionMap={sessionMap}
      />
    </>
  );
};
