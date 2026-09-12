// @vitest-environment jsdom
import { act, createElement, useEffect } from "react";
import { type Root, createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { NavigationMenuItem } from "@src/scaffold/NavigationSidebar/components/NavigationMenu/config";
import { SESSION_SIDEBAR_PAGE_SIZE } from "@src/store/session";
import { CHAT_PANEL_CREATE_TARGET } from "@src/store/ui/chatPanel/selectionAtoms";
import { STORY_ORG_SCOPE } from "@src/store/workstation/tabs";

import { PROJECTS_NEW_WORK_ITEM_MENU_ITEM_ID } from "../sidebarConnectorUtils";
import { LOAD_MORE_GROUP_PREFIX } from "../types";
import * as ids from "../useProjectsWorkItemMenuItems/idHelpers";
import type { UseProjectsWorkItemMenuItemsParams } from "../useProjectsWorkItemMenuItems/types";
import { useWorkItemsSidebarSurface } from "./useWorkItemsSidebarSurface";

const mocks = vi.hoisted(() => ({
  model: vi.fn(),
  openWorkItem: vi.fn(),
  openProject: vi.fn(),
  openOrganization: vi.fn(),
  openCreator: vi.fn(),
  wrap: vi.fn(),
  mounted: vi.fn(),
  unmounted: vi.fn(),
}));
vi.mock("../useProjectsWorkItemMenuItems/index", async () => ({
  ...(await import("../useProjectsWorkItemMenuItems/idHelpers")),
  useProjectsWorkItemMenuItems: (
    params: UseProjectsWorkItemMenuItemsParams
  ) => {
    useEffect(() => {
      mocks.mounted();
      return () => mocks.unmounted();
    }, []);
    return mocks.model(params);
  },
}));
vi.mock("./menuItemWrappers", () => ({
  useRenderProjectsMenuItemWrapper: () => mocks.wrap,
}));
vi.mock("@src/store/chatPanel/chatPanelTabsAtom", async () => {
  const { atom } = await import("jotai");
  return {
    openWorkItemInChatPanelTabAtom: atom(null, (_get, _set, value: unknown) =>
      mocks.openWorkItem(value)
    ),
    openProjectInChatPanelTabAtom: atom(null, (_get, _set, value: unknown) =>
      mocks.openProject(value)
    ),
    openOrganizationInChatPanelTabAtom: atom(
      null,
      (_get, _set, value: unknown) => mocks.openOrganization(value)
    ),
    openChatPanelCreateTargetAtom: atom(null, (_get, _set, value: unknown) =>
      mocks.openCreator(value)
    ),
  };
});

const row = (id: string, label = id): NavigationMenuItem => ({
  id,
  key: id,
  label,
});
const groupA = "projects-work-items:org:org-a";
const groupB = "projects-work-items:org:org-b";
const project = { slug: "project-a" };
const workItem = { id: "work-a" };
const linearOrg = { id: "linear-a" };
const linearWorkItem = { id: "linear-work-a" };
const localOrg = { id: "org-a", name: "Team A", sync_provider: "local" };
const menuItems = [
  row("separator-projects"),
  row(ids.getWorkItemMenuItemId("work-a"), "Original work item"),
];

describe("persistent work-item sidebar surface", () => {
  let root: Root;
  let container: HTMLDivElement;
  let surface: ReturnType<typeof useWorkItemsSidebarSurface>;
  const activateDetail = vi.fn();
  const reset = vi.fn();
  const openLinkedSession = vi.fn();
  const loadLinear = vi.fn();
  const openLinearOrg = vi.fn();
  const openLinearWorkItem = vi.fn();

  function Probe({ enabled, orgId }: { enabled: boolean; orgId: string }) {
    const value = useWorkItemsSidebarSurface({
      enabled,
      activeProjectOrgId: orgId,
      activateMyStationRouteForProjectTabContent: activateDetail,
      handleOpenLinkedWorkItemSession: openLinkedSession,
    });
    useEffect(() => {
      surface = value;
    });
    return null;
  }
  const render = (enabled = true, orgId = "org-a") =>
    act(() => root.render(createElement(Probe, { enabled, orgId })));
  const click = (item: NavigationMenuItem) =>
    act(() => surface.onMenuItemClick(item.key, item));
  const latestQuery = () =>
    mocks.model.mock.lastCall![0] as UseProjectsWorkItemMenuItemsParams;

  beforeEach(() => {
    Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
    vi.clearAllMocks();
    mocks.model.mockReturnValue({
      menuItems,
      loading: false,
      projectMap: new Map([["project-a", project]]),
      workItemMap: new Map([["work-a", workItem]]),
      linearOrgMap: new Map([["linear-a", linearOrg]]),
      linearWorkItemMap: new Map([["linear-work-a", linearWorkItem]]),
      localOrgMap: new Map([["org-a", localOrg]]),
      linkedSessionIds: new Set(["linked-session"]),
      getLoadMoreGroupId: ids.isProjectsWorkItemLoadMoreId,
      loadLinearOrgWorkItems: loadLinear,
      toChatPanelProject: () => ({ slug: "project-a", name: "Project A" }),
      toChatPanelWorkItem: () => ({ id: "work-a", title: "Work A" }),
      openLinearOrg,
      openLinearWorkItem,
    });
    container = document.createElement("div");
    root = createRoot(container);
  });
  afterEach(() => {
    act(() => root.unmount());
    Reflect.deleteProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT");
  });

  it("retains selection, per-group pagination and collapse state across view and scope switches", () => {
    render();
    click(row(`${LOAD_MORE_GROUP_PREFIX}${groupA}`));
    click(row(`${LOAD_MORE_GROUP_PREFIX}${groupB}`));
    click(row(ids.getWorkItemMenuItemId("work-a")));
    expect(latestQuery().groupVisibleCounts.get(groupA)).toBe(
      SESSION_SIDEBAR_PAGE_SIZE * 2
    );
    expect(surface.menuItems).toBe(menuItems);
    expect(surface.renderMenuItemWrapper).toBe(mocks.wrap);
    act(() => surface.onCollapsedSectionIdsChange(new Set([groupA])));
    expect(latestQuery().groupVisibleCounts.has(groupA)).toBe(false);
    expect(latestQuery().groupVisibleCounts.get(groupB)).toBe(
      SESSION_SIDEBAR_PAGE_SIZE * 2
    );
    render(false, "org-b");
    expect(latestQuery()).toMatchObject({
      enabled: false,
      selectedOrgId: "org-b",
      searchQuery: "",
    });
    expect(surface.selectedMenuItemId).toBe(
      ids.getWorkItemMenuItemId("work-a")
    );
    expect(surface.collapsedSectionIds).toEqual(new Set([groupA]));
    render(true);
    expect(latestQuery().groupVisibleCounts.get(groupB)).toBe(
      SESSION_SIDEBAR_PAGE_SIZE * 2
    );
    expect(mocks.mounted).toHaveBeenCalledTimes(1);
    expect(mocks.unmounted).not.toHaveBeenCalled();
  });

  it("dispatches local/project/work-item and Linear rows through their existing owners", () => {
    render();
    click(row(ids.getLocalOrgMenuItemId("org-a")));
    expect(mocks.openOrganization).toHaveBeenCalledWith({
      organization: {
        kind: "local",
        projectOrg: {
          orgId: "org-a",
          orgName: "Team A",
          orgScope: STORY_ORG_SCOPE.PROJECT_ORG,
          orgSyncProvider: "local",
        },
      },
      title: "Team A",
    });
    click(row(ids.getProjectOverviewMenuItemId("project-a")));
    expect(mocks.openProject).toHaveBeenCalledWith({
      slug: "project-a",
      name: "Project A",
    });
    click(row(ids.getWorkItemMenuItemId("work-a")));
    expect(mocks.openWorkItem).toHaveBeenCalledWith({
      id: "work-a",
      title: "Work A",
    });
    click(row(ids.getLinearOrgMenuItemId("linear-a")));
    expect(openLinearOrg).toHaveBeenCalledWith(linearOrg);
    click(row(ids.getLinearWorkItemMenuItemId("linear-work-a")));
    expect(openLinearWorkItem).toHaveBeenCalledWith(linearWorkItem);
    click(row("projects-linear-load:linear-a"));
    expect(loadLinear).toHaveBeenCalledWith("linear-a");
    expect(activateDetail).toHaveBeenCalledTimes(5);
    expect(reset).not.toHaveBeenCalled();
  });

  it("keeps linked-session row keys and creator organization context", () => {
    render();
    const linked = {
      ...row("linked-session"),
      key: "work-item-linked-session:work-a:linked-session",
    };
    click(linked);
    expect(openLinkedSession).toHaveBeenCalledWith(linked);
    expect(surface.selectedMenuItemId).toBe(linked.key);
    expect(activateDetail).not.toHaveBeenCalled();
    click(row(`${PROJECTS_NEW_WORK_ITEM_MENU_ITEM_ID}:org-a`));
    expect(mocks.openCreator).toHaveBeenCalledWith({
      target: CHAT_PANEL_CREATE_TARGET.WORK_ITEM,
      createProjectContext: { orgId: "org-a" },
    });
    expect(reset).not.toHaveBeenCalled();
    expect(surface.selectedMenuItemId).toBe(linked.key);
  });
});
