import { useSetAtom } from "jotai";
import { useCallback } from "react";

import {
  openCollabOrgSpotlight,
  openGitHubIssuesImportSpotlight,
} from "@src/scaffold/GlobalSpotlight/openSpotlight";
import type { NavigationMenuItem } from "@src/scaffold/NavigationSidebar/components/NavigationMenu/config";
import {
  openChatPanelCreateTargetAtom,
  openOrganizationInChatPanelTabAtom,
  openProjectInChatPanelTabAtom,
  openWorkItemInChatPanelTabAtom,
} from "@src/store/chatPanel/chatPanelTabsAtom";
import { SESSION_SIDEBAR_PAGE_SIZE } from "@src/store/session";
import {
  CHAT_PANEL_CREATE_TARGET,
  type ChatPanelSelectedProject,
  type ChatPanelSelectedWorkItem,
} from "@src/store/ui/chatPanel/selectionAtoms";
import { STORY_ORG_SCOPE } from "@src/store/workstation/tabs";

import {
  COLLAB_ADD_ORG_MENU_ITEM_ID,
  PROJECTS_IMPORT_GITHUB_ISSUES_MENU_ITEM_ID,
  PROJECTS_NEW_PROJECT_MENU_ITEM_ID,
  PROJECTS_NEW_WORK_ITEM_MENU_ITEM_ID,
} from "../sidebarConnectorUtils";
import {
  getProjectsLinearLoadOrgId,
  getProjectsLinearOrgId,
  getProjectsLinearWorkItemId,
  getProjectsLocalOrgId,
  getProjectsProjectOverviewSlug,
  getProjectsWorkItemCreateOrgId,
  getProjectsWorkItemId,
} from "../useProjectsWorkItemMenuItems/index";

interface UseProjectsMenuItemClickParams<
  Project,
  WorkItem,
  LocalOrg extends { id: string; name: string; sync_provider?: string | null },
  LinearOrg,
  LinearWorkItem,
> {
  activateMyStationRouteForProjectTabContent: () => void;
  getProjectsLoadMoreGroupId: (id: string) => string | null;
  loadProjectsLinearOrgWorkItems: (orgId: string) => void;
  openProjectsLinearOrg: (org: LinearOrg) => void;
  openProjectsLinearWorkItem: (workItem: LinearWorkItem) => void;
  projectsLinearOrgMap: ReadonlyMap<string, LinearOrg>;
  projectsLinearWorkItemMap: ReadonlyMap<string, LinearWorkItem>;
  projectsLocalOrgMap: ReadonlyMap<string, LocalOrg>;
  projectsProjectMap: ReadonlyMap<string, Project>;
  projectsWorkItemMap: ReadonlyMap<string, WorkItem>;
  linkedSessionIds: ReadonlySet<string>;
  openLinkedSession: (item: NavigationMenuItem) => void;
  setProjectsGroupVisibleCounts: React.Dispatch<
    React.SetStateAction<Map<string, number>>
  >;
  setProjectsSelectedMenuItemId: (id: string) => void;
  toChatPanelProject: (project: Project) => ChatPanelSelectedProject;
  toChatPanelWorkItem: (workItem: WorkItem) => ChatPanelSelectedWorkItem;
}

interface TryOpenLinkedSessionFromSidebarParams {
  item: NavigationMenuItem;
  linkedSessionIds: ReadonlySet<string>;
  setProjectsSelectedMenuItemId: (id: string) => void;
  openLinkedSession: (item: NavigationMenuItem) => void;
}

export function tryOpenLinkedSessionFromSidebar({
  item,
  linkedSessionIds,
  setProjectsSelectedMenuItemId,
  openLinkedSession,
}: TryOpenLinkedSessionFromSidebarParams): boolean {
  if (!linkedSessionIds.has(item.id)) return false;
  setProjectsSelectedMenuItemId(item.key);
  openLinkedSession(item);
  return true;
}

export function useProjectsMenuItemClick<
  Project,
  WorkItem,
  LocalOrg extends { id: string; name: string; sync_provider?: string | null },
  LinearOrg,
  LinearWorkItem,
>({
  activateMyStationRouteForProjectTabContent,
  getProjectsLoadMoreGroupId,
  loadProjectsLinearOrgWorkItems,
  openProjectsLinearOrg,
  openProjectsLinearWorkItem,
  projectsLinearOrgMap,
  projectsLinearWorkItemMap,
  projectsLocalOrgMap,
  projectsProjectMap,
  projectsWorkItemMap,
  linkedSessionIds,
  openLinkedSession,
  setProjectsGroupVisibleCounts,
  setProjectsSelectedMenuItemId,
  toChatPanelProject,
  toChatPanelWorkItem,
}: UseProjectsMenuItemClickParams<
  Project,
  WorkItem,
  LocalOrg,
  LinearOrg,
  LinearWorkItem
>): (key: string, item: NavigationMenuItem) => void {
  // Detail surfaces (org hub / project / work item) open as dedicated chat-pane
  // tabs. Creator actions target the singleton Launchpad instead.
  const openWorkItemTab = useSetAtom(openWorkItemInChatPanelTabAtom);
  const openProjectTab = useSetAtom(openProjectInChatPanelTabAtom);
  const openOrganizationTab = useSetAtom(openOrganizationInChatPanelTabAtom);
  const openCreateTarget = useSetAtom(openChatPanelCreateTargetAtom);
  return useCallback(
    (_key: string, item: NavigationMenuItem) => {
      if (item.id === COLLAB_ADD_ORG_MENU_ITEM_ID) {
        openCollabOrgSpotlight();
        return;
      }

      if (item.id === PROJECTS_NEW_PROJECT_MENU_ITEM_ID) {
        openCreateTarget({
          target: CHAT_PANEL_CREATE_TARGET.PROJECT,
        });
        return;
      }

      if (item.id === PROJECTS_IMPORT_GITHUB_ISSUES_MENU_ITEM_ID) {
        openGitHubIssuesImportSpotlight();
        return;
      }

      if (item.id === PROJECTS_NEW_WORK_ITEM_MENU_ITEM_ID) {
        openCreateTarget({ target: CHAT_PANEL_CREATE_TARGET.WORK_ITEM });
        return;
      }

      if (
        tryOpenLinkedSessionFromSidebar({
          item,
          linkedSessionIds,
          setProjectsSelectedMenuItemId,
          openLinkedSession,
        })
      )
        return;

      const localOrgId = getProjectsLocalOrgId(item.id);
      if (localOrgId) {
        const localOrg = projectsLocalOrgMap.get(localOrgId);
        if (!localOrg) return;
        activateMyStationRouteForProjectTabContent();
        setProjectsSelectedMenuItemId(item.id);
        openOrganizationTab({
          organization: {
            kind: "local",
            projectOrg: {
              orgId: localOrg.id,
              orgName: localOrg.name,
              orgScope: STORY_ORG_SCOPE.PROJECT_ORG,
              orgSyncProvider: localOrg.sync_provider,
            },
          },
          title: localOrg.name,
        });
        return;
      }

      const linearOrgId = getProjectsLinearOrgId(item.id);
      if (linearOrgId) {
        const linearOrg = projectsLinearOrgMap.get(linearOrgId);
        if (!linearOrg) return;
        activateMyStationRouteForProjectTabContent();
        setProjectsSelectedMenuItemId(item.id);
        openProjectsLinearOrg(linearOrg);
        return;
      }

      const createWorkItemOrgId = getProjectsWorkItemCreateOrgId(item.id);
      if (createWorkItemOrgId) {
        // The row is org-scoped, so the creation surface must carry the org:
        // NEW_WORK_ITEM without `createProjectContext` writes standalone
        // items under personal-org (see createWorkItemFromDraft).
        openCreateTarget({
          target: CHAT_PANEL_CREATE_TARGET.WORK_ITEM,
          createProjectContext: { orgId: createWorkItemOrgId },
        });
        return;
      }

      const linearLoadOrgId = getProjectsLinearLoadOrgId(item.id);
      if (linearLoadOrgId) {
        loadProjectsLinearOrgWorkItems(linearLoadOrgId);
        return;
      }

      const loadMoreGroupId = getProjectsLoadMoreGroupId(item.id);
      if (loadMoreGroupId) {
        setProjectsGroupVisibleCounts((previousCounts) => {
          const nextCounts = new Map(previousCounts);
          const current =
            nextCounts.get(loadMoreGroupId) ?? SESSION_SIDEBAR_PAGE_SIZE;
          nextCounts.set(loadMoreGroupId, current + SESSION_SIDEBAR_PAGE_SIZE);
          return nextCounts;
        });
        return;
      }

      const projectOverviewSlug = getProjectsProjectOverviewSlug(item.id);
      if (projectOverviewSlug) {
        const project = projectsProjectMap.get(projectOverviewSlug);
        if (!project) return;
        activateMyStationRouteForProjectTabContent();
        setProjectsSelectedMenuItemId(item.id);
        openProjectTab(toChatPanelProject(project));
        return;
      }

      const linearWorkItemId = getProjectsLinearWorkItemId(item.id);
      if (linearWorkItemId) {
        const linearWorkItem = projectsLinearWorkItemMap.get(linearWorkItemId);
        if (!linearWorkItem) return;
        activateMyStationRouteForProjectTabContent();
        setProjectsSelectedMenuItemId(item.id);
        openProjectsLinearWorkItem(linearWorkItem);
        return;
      }

      const workItemId = getProjectsWorkItemId(item.id);
      if (!workItemId) return;
      const workItem = projectsWorkItemMap.get(workItemId);
      if (!workItem) return;
      const chatPanelWorkItem = toChatPanelWorkItem(workItem);
      activateMyStationRouteForProjectTabContent();
      setProjectsSelectedMenuItemId(item.id);
      openWorkItemTab(chatPanelWorkItem);
    },
    [
      activateMyStationRouteForProjectTabContent,
      getProjectsLoadMoreGroupId,
      loadProjectsLinearOrgWorkItems,
      linkedSessionIds,
      openCreateTarget,
      openOrganizationTab,
      openProjectTab,
      openProjectsLinearOrg,
      openProjectsLinearWorkItem,
      openLinkedSession,
      openWorkItemTab,
      projectsLinearOrgMap,
      projectsLinearWorkItemMap,
      projectsLocalOrgMap,
      projectsProjectMap,
      projectsWorkItemMap,
      setProjectsGroupVisibleCounts,
      setProjectsSelectedMenuItemId,
      toChatPanelProject,
      toChatPanelWorkItem,
    ]
  );
}
