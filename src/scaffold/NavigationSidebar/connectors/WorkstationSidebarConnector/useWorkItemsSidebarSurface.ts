import { useCallback, useState } from "react";

import type { NavigationMenuItem } from "@src/scaffold/NavigationSidebar/components/NavigationMenu/config";

import { useProjectsWorkItemMenuItems } from "../useProjectsWorkItemMenuItems/index";
import { useRenderProjectsMenuItemWrapper } from "./menuItemWrappers";
import {
  getProjectsSectionVisibleCountKey,
  resetNewlyCollapsedSectionVisibleCounts,
} from "./sectionPagination";
import { useProjectsMenuItemClick } from "./useProjectsMenuItemClick";
import type { useSidebarStationNavigation } from "./useSidebarStationNavigation";

type StationNavigation = ReturnType<typeof useSidebarStationNavigation>;
interface WorkItemsSidebarSurfaceParams {
  enabled: boolean;
  activeProjectOrgId: Parameters<
    typeof useProjectsWorkItemMenuItems
  >[0]["selectedOrgId"];
  activateMyStationRouteForProjectTabContent: StationNavigation["activateMyStationRouteForProjectTabContent"];
  handleOpenLinkedWorkItemSession: (item: NavigationMenuItem) => void;
}

/** Always mounted: visibility only gates the existing project data source. */
export function useWorkItemsSidebarSurface({
  enabled,
  activeProjectOrgId,
  activateMyStationRouteForProjectTabContent,
  handleOpenLinkedWorkItemSession,
}: WorkItemsSidebarSurfaceParams) {
  const [projectsSelectedMenuItemId, setProjectsSelectedMenuItemId] =
    useState("");
  const [projectsGroupVisibleCounts, setProjectsGroupVisibleCounts] = useState<
    Map<string, number>
  >(new Map());
  const [projectsCollapsedSectionIds, setProjectsCollapsedSectionIds] =
    useState<Set<string>>(() => new Set());
  const {
    menuItems: projectsWorkItemMenuItems,
    projectMap: projectsProjectMap,
    workItemMap: projectsWorkItemMap,
    linearWorkItemMap: projectsLinearWorkItemMap,
    localOrgMap: projectsLocalOrgMap,
    linearOrgMap: projectsLinearOrgMap,
    loading: projectsWorkItemsLoading,
    linkedSessionIds: projectsLinkedSessionIds,
    getLoadMoreGroupId: getProjectsLoadMoreGroupId,
    loadLinearOrgWorkItems: loadProjectsLinearOrgWorkItems,
    toChatPanelProject,
    toChatPanelWorkItem,
    openLinearOrg: openProjectsLinearOrg,
    openLinearWorkItem: openProjectsLinearWorkItem,
  } = useProjectsWorkItemMenuItems({
    enabled,
    groupVisibleCounts: projectsGroupVisibleCounts,
    searchQuery: "",
    selectedOrgId: activeProjectOrgId,
  });
  const handleProjectsMenuItemClick = useProjectsMenuItemClick({
    activateMyStationRouteForProjectTabContent,
    getProjectsLoadMoreGroupId,
    loadProjectsLinearOrgWorkItems,
    openProjectsLinearOrg,
    openProjectsLinearWorkItem: openProjectsLinearWorkItem,
    projectsLinearOrgMap,
    projectsLinearWorkItemMap,
    projectsLocalOrgMap,
    projectsProjectMap,
    projectsWorkItemMap,
    linkedSessionIds: projectsLinkedSessionIds,
    openLinkedSession: handleOpenLinkedWorkItemSession,
    setProjectsGroupVisibleCounts,
    setProjectsSelectedMenuItemId,
    toChatPanelProject,
    toChatPanelWorkItem,
  });
  const handleProjectsCollapsedSectionIdsChange = useCallback(
    (nextCollapsedSectionIds: Set<string>) => {
      setProjectsGroupVisibleCounts((currentVisibleCounts) =>
        resetNewlyCollapsedSectionVisibleCounts({
          currentVisibleCounts,
          previousCollapsedSectionIds: projectsCollapsedSectionIds,
          nextCollapsedSectionIds,
          resolveVisibleCountKey: getProjectsSectionVisibleCountKey,
        })
      );
      setProjectsCollapsedSectionIds(nextCollapsedSectionIds);
    },
    [
      projectsCollapsedSectionIds,
      setProjectsCollapsedSectionIds,
      setProjectsGroupVisibleCounts,
    ]
  );

  const renderMenuItemWrapper = useRenderProjectsMenuItemWrapper({
    projectsLinearWorkItemMap,
    projectsWorkItemMap,
  });
  return {
    menuItems: projectsWorkItemMenuItems,
    loading: projectsWorkItemsLoading,
    onMenuItemClick: handleProjectsMenuItemClick,
    renderMenuItemWrapper,
    selectedMenuItemId: projectsSelectedMenuItemId,
    setSelectedMenuItemId: setProjectsSelectedMenuItemId,
    collapsedSectionIds: projectsCollapsedSectionIds,
    onCollapsedSectionIdsChange: handleProjectsCollapsedSectionIdsChange,
  };
}
