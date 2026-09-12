/**
 * Browser Tabs Store
 *
 * Centralized tab system for the Browser surface:
 * - Browser sessions (webview tabs)
 *
 * Browser-session tabs are shared WorkStation resources.
 * `workstationLayoutAtom` is the compatibility projection for the currently
 * presented agent workspace; its split writer routes browser-family changes
 * back to the canonical shared partition. Consequently, changing the
 * presented agent workspace changes only that workspace's active selection —
 * it never removes browser resources or makes a workspace switch look like a
 * browser-tab close.
 */
import { atom } from "jotai";

import type { PanelState } from "@src/store/workstation/tabs";
import {
  presentedWorkstationWorkspaceKeyAtom,
  removeSharedWorkstationTabAtom,
  removeSharedWorkstationTabsAtom,
  workstationLayoutAtom,
  workstationTabsStateAtom,
} from "@src/store/workstation/tabs/atoms";
import { recordRecentWorkstationTabAtom } from "@src/store/workstation/tabs/recentTabs";
import {
  closeOtherTabs as closeOtherTabsMutation,
  closeSavedTabs as closeSavedTabsMutation,
  openTab as openTabMutation,
  reorderTabs as reorderTabsMutation,
  switchTab as switchTabMutation,
} from "@src/store/workstation/tabs/tabMutations";
import type {
  WorkStationTab,
  WorkStationTabType,
} from "@src/store/workstation/tabs/types";
import { getSiteNameFromUrl } from "@src/util/url/browserDisplay";

// ============================================
// Types
// ============================================

interface BrowserSessionData {
  sessionId: string;
  url: string;
  incognito?: boolean;
  isLoading?: boolean;
}

// ============================================
// Tab ID Helpers
// ============================================

export function createBrowserSessionTabId(sessionId: string): string {
  return `browser:${sessionId}`;
}

export function isBrowserSessionTab(tabId: string): boolean {
  return tabId.startsWith("browser:");
}

export function extractSessionId(tabId: string): string {
  return tabId.replace("browser:", "");
}

// ============================================
// Display helpers
// ============================================

/**
 * Sentinel titles persisted on a browser session when the user hasn't
 * yet navigated to a page that has its own document title. Treated as
 * "no real title yet" by every display site so URL-derived fallbacks
 * win, and translated to the user's locale when shown as-is.
 *
 * IMPORTANT: these strings must stay in English on disk — they're the
 * wire / localStorage representation. The i18n layer maps them to the
 * locale-specific label at render time
 * (`common:controlTower.sidebar.newTab` /
 * `common:controlTower.sidebar.newPrivateTab`). Changing them here
 * would invalidate every persisted browser session.
 */
export const NEW_TAB_TITLE = "New Tab";
export const NEW_PRIVATE_TAB_TITLE = "New Private Tab";

/** True when `title` is one of the placeholder sentinels above. */
export function isPlaceholderBrowserSessionTitle(
  title: string | undefined
): boolean {
  return title === NEW_TAB_TITLE || title === NEW_PRIVATE_TAB_TITLE;
}

/**
 * Resolve the display title for a browser session.
 * Prefers the page title (when not a placeholder sentinel), then the URL's
 * site name, then the appropriate placeholder as a final fallback.
 *
 * The returned placeholder is still in English; callers that render to
 * the user must run it through {@link translatePlaceholderBrowserSessionTitle}
 * (or the equivalent inline check) so it picks up the user's locale.
 */
export function getBrowserSessionDisplayTitle(session: {
  title?: string;
  url?: string;
}): string {
  if (session.title && !isPlaceholderBrowserSessionTitle(session.title)) {
    return session.title;
  }
  if (session.url) {
    return getSiteNameFromUrl(session.url);
  }
  return session.title === NEW_PRIVATE_TAB_TITLE
    ? NEW_PRIVATE_TAB_TITLE
    : NEW_TAB_TITLE;
}

/**
 * Map a placeholder sentinel ("New Tab" / "New Private Tab") to its
 * locale-specific label via the supplied `t` function. Non-placeholder
 * strings pass through unchanged.
 *
 * Accepts the `t` function from any namespace; uses absolute keys
 * (`common:controlTower.sidebar.*`) so it doesn't depend on the
 * caller's active namespace.
 */
export function translatePlaceholderBrowserSessionTitle(
  title: string,
  t: (key: string) => string
): string {
  if (title === NEW_TAB_TITLE) {
    return t("common:controlTower.sidebar.newTab");
  }
  if (title === NEW_PRIVATE_TAB_TITLE) {
    return t("common:controlTower.sidebar.newPrivateTab");
  }
  return title;
}

// ============================================
// Tab Factories
// ============================================

export function createBrowserSessionTab(
  sessionId: string,
  title: string,
  data: Partial<BrowserSessionData> = {}
): WorkStationTab {
  return {
    id: createBrowserSessionTabId(sessionId),
    type: "browser-session",
    title: title || NEW_TAB_TITLE,
    // Intentionally omit `icon`: SortableTab's `type === "browser-session"`
    // branch renders FaviconIcon, which prefers the URL-derived favicon over
    // the globe glyph fallback. Setting an icon name here would short-circuit
    // that branch and force a Globe regardless of the URL.
    data: {
      sessionId,
      url: data.url ?? "",
      incognito: data.incognito ?? false,
      isLoading: data.isLoading ?? false,
    },
    hasUnsavedChanges: false,
  };
}

// ============================================
// Browser-family tab classification
// ============================================

const BROWSER_TAB_TYPES: ReadonlySet<WorkStationTabType> = new Set([
  "browser-session",
]);

function isBrowserFamilyTab(tab: WorkStationTab): boolean {
  return BROWSER_TAB_TYPES.has(tab.type);
}

/**
 * Project a `PanelState` slice that contains only the browser-family
 * tabs from `mainPane`. `activeTabId` is preserved only if it points to
 * a browser-family tab; otherwise `null`.
 */
function projectBrowserSlice(mainPane: PanelState): PanelState {
  const tabs = mainPane.tabs.filter(isBrowserFamilyTab);
  const activeTabId = tabs.some((tab) => tab.id === mainPane.activeTabId)
    ? mainPane.activeTabId
    : null;
  return { tabs, activeTabId };
}

/**
 * Splice an updated browser slice back into the full `mainPane` pool.
 *
 * Strategy: replace browser-family entries in-place to preserve the
 * relative interleaving with non-browser tabs (file tabs, project tabs,
 * etc.). New browser tabs in `next.tabs` that were not in `prev.tabs`
 * are appended at the end of the pool; removed browser tabs are
 * deleted. The pane's `activeTabId` adopts `next.activeTabId` only when
 * it is a browser-family tab id — non-browser tabs retain their own
 * focus story (the user might be on a file tab while the browser slice
 * also has a session active).
 */
function applyBrowserSlice(mainPane: PanelState, next: PanelState): PanelState {
  const nextById = new Map(next.tabs.map((tab) => [tab.id, tab]));
  const result: WorkStationTab[] = [];
  let insertCursor = -1;
  for (let i = 0; i < mainPane.tabs.length; i++) {
    const existing = mainPane.tabs[i];
    if (!isBrowserFamilyTab(existing)) {
      result.push(existing);
      continue;
    }
    if (insertCursor === -1) insertCursor = result.length;
    const replacement = nextById.get(existing.id);
    if (replacement) {
      result.push(replacement);
      nextById.delete(existing.id);
    }
    // dropped browser tabs are not appended
  }

  // Append any net-new browser tabs in their `next.tabs` order at the
  // first browser-family slot (so they cluster with siblings). If there
  // were no browser tabs before, append at the end.
  const leftovers = next.tabs.filter((tab) => nextById.has(tab.id));
  if (leftovers.length) {
    if (insertCursor === -1) {
      result.push(...leftovers);
    } else {
      result.splice(insertCursor, 0, ...leftovers);
    }
  }

  // Adopt the next active id only when it points to a browser tab still
  // present in `result`; otherwise keep mainPane's activeTabId so
  // non-browser focus is unaffected by browser slice writes.
  const adoptingActive =
    next.activeTabId &&
    result.some(
      (tab) => tab.id === next.activeTabId && isBrowserFamilyTab(tab)
    );
  return {
    tabs: result,
    activeTabId: adoptingActive ? next.activeTabId : mainPane.activeTabId,
  };
}

// ============================================
// Main Atom (derived read + write)
// ============================================

/**
 * Browser tabs atom — derived view + writer over the browser-family
 * slice of `workstationLayoutAtom.mainPane`.
 */
export const browserTabsAtom = atom(
  (get): PanelState => {
    const layout = get(workstationLayoutAtom);
    return projectBrowserSlice(
      layout?.mainPane ?? { tabs: [], activeTabId: null }
    );
  },
  (
    get,
    set,
    nextOrUpdater: PanelState | ((prev: PanelState) => PanelState)
  ) => {
    const layout = get(workstationLayoutAtom);
    if (!layout) return;
    const prev = projectBrowserSlice(layout.mainPane);
    const next =
      typeof nextOrUpdater === "function"
        ? (nextOrUpdater as (s: PanelState) => PanelState)(prev)
        : nextOrUpdater;
    set(workstationLayoutAtom, {
      ...layout,
      mainPane: applyBrowserSlice(layout.mainPane, next),
    });
  }
);
browserTabsAtom.debugLabel = "browserTabsAtom";

/** Global browser resources, independent of which workspace currently shows them. */
export const sharedBrowserTabsAtom = atom((get): WorkStationTab[] =>
  get(workstationTabsStateAtom).shared.tabs.filter(isBrowserFamilyTab)
);
sharedBrowserTabsAtom.debugLabel = "sharedBrowserTabsAtom";

// ============================================
// Derived Atoms
// ============================================

/**
 * Active tab in browser
 */
export const activeBrowserTabAtom = atom((get) => {
  const state = get(browserTabsAtom);
  return state.tabs.find((tab) => tab.id === state.activeTabId) ?? null;
});
activeBrowserTabAtom.debugLabel = "activeBrowserTabAtom";

// ============================================
// Action Atoms (for convenience)
// ============================================

/**
 * Open a tab (or switch to it if exists)
 */
export const openBrowserTabAtom = atom(
  null,
  (get, set, tab: WorkStationTab) => {
    const state = get(browserTabsAtom);
    set(browserTabsAtom, openTabMutation(state, tab));
  }
);

export const removeBrowserResourceTabAtom = atom(
  null,
  (_get, set, tabId: string) => {
    set(removeSharedWorkstationTabAtom, tabId);
  }
);

/**
 * Close a browser tab in the current workspace. The live BrowserContext owner
 * observes the disappearance and then removes the global resource explicitly.
 */
export const closeBrowserTabAtom = atom(null, (get, set, tabId: string) => {
  const tab = get(browserTabsAtom).tabs.find(
    (candidate) => candidate.id === tabId
  );
  set(removeBrowserResourceTabAtom, tabId);
  if (tab) {
    set(recordRecentWorkstationTabAtom, {
      workspace: get(presentedWorkstationWorkspaceKeyAtom),
      tab,
    });
  }
});

/**
 * Switch to a tab
 */
export const switchBrowserTabAtom = atom(null, (get, set, tabId: string) => {
  const state = get(browserTabsAtom);
  set(browserTabsAtom, switchTabMutation(state, tabId));
});

/**
 * Reorder tabs
 */
export const reorderBrowserTabsAtom = atom(
  null,
  (
    get,
    set,
    { startIndex, endIndex }: { startIndex: number; endIndex: number }
  ) => {
    const state = get(browserTabsAtom);
    set(browserTabsAtom, reorderTabsMutation(state, startIndex, endIndex));
  }
);

/**
 * Close other tabs
 */
export const closeOtherBrowserTabsAtom = atom(
  null,
  (get, set, tabId: string) => {
    const state = get(browserTabsAtom);
    const next = closeOtherTabsMutation(state, tabId);
    const nextIds = new Set(next.tabs.map((tab) => tab.id));
    set(
      removeSharedWorkstationTabsAtom,
      state.tabs.filter((tab) => !nextIds.has(tab.id)).map((tab) => tab.id)
    );
    for (const tab of state.tabs) {
      if (!nextIds.has(tab.id)) {
        set(recordRecentWorkstationTabAtom, {
          workspace: get(presentedWorkstationWorkspaceKeyAtom),
          tab,
        });
      }
    }
  }
);

/**
 * Close saved tabs
 */
export const closeSavedBrowserTabsAtom = atom(null, (get, set) => {
  const state = get(browserTabsAtom);
  const next = closeSavedTabsMutation(state);
  const nextIds = new Set(next.tabs.map((tab) => tab.id));
  set(
    removeSharedWorkstationTabsAtom,
    state.tabs.filter((tab) => !nextIds.has(tab.id)).map((tab) => tab.id)
  );
  for (const tab of state.tabs) {
    if (!nextIds.has(tab.id)) {
      set(recordRecentWorkstationTabAtom, {
        workspace: get(presentedWorkstationWorkspaceKeyAtom),
        tab,
      });
    }
  }
});

/**
 * Update tab data (e.g., update URL for browser session)
 */
export const updateBrowserTabDataAtom = atom(
  null,
  (
    get,
    set,
    { tabId, data }: { tabId: string; data: Partial<Record<string, unknown>> }
  ) => {
    const state = get(browserTabsAtom);
    set(browserTabsAtom, {
      ...state,
      tabs: state.tabs.map((tab) =>
        tab.id === tabId ? { ...tab, data: { ...tab.data, ...data } } : tab
      ),
    });
  }
);

/**
 * Update tab title
 */
export const updateBrowserTabTitleAtom = atom(
  null,
  (get, set, { tabId, title }: { tabId: string; title: string }) => {
    const state = get(browserTabsAtom);
    set(browserTabsAtom, {
      ...state,
      tabs: state.tabs.map((tab) =>
        tab.id === tabId ? { ...tab, title } : tab
      ),
    });
  }
);
