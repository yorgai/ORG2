/**
 * useAppNavigation Hook
 *
 * Unified navigation system for application routes.
 * Route is the single source of truth - all navigation goes through React Router.
 *
 * Features:
 * - Generic route navigation with lazy-route preloading
 * - Session route navigation
 *
 * Usage:
 * ```tsx
 * const { navigateTo, goToSettings, goToNewSession } = useAppNavigation();
 *
 * // Navigate to Settings
 * goToSettings();
 *
 * // Navigate to session creator (clears active session)
 * goToNewSession(); // or goToNewSession({ projectId, workflowId })
 * ```
 *
 * Architecture:
 * - Uses React Router's navigate() for all navigation
 * - The router owns which shell is mounted for each route branch
 *
 * Created: 2026-02-01
 */
import { useSetAtom } from "jotai";
import { useCallback } from "react";
import { useNavigate } from "react-router-dom";

import {
  type ExternalSkillsetsTab,
  type IntegrationsCategorySegment,
  type SettingsPathOptions,
  buildExternalSkillsetsPath,
  buildIntegrationsPath,
  buildSettingsPath,
} from "@src/config/mainAppPaths";
import { ROUTES } from "@src/config/routes";
import { clearSessionAtom } from "@src/engines/SessionCore/core/atoms";
import { preloadRouteByPath } from "@src/router/lazy/preload";
import {
  activeSessionIdAtom,
  promoteActiveSessionCreatorDraftAtom,
  selectSessionCreatorDraftAtom,
  startNewSessionCreatorDraftAtom,
  workstationActiveSessionIdAtom,
} from "@src/store/session";
import { resetChatPanelSessionSurfaceAtom } from "@src/store/ui/chatPanel/surfaceAtoms";

// ============================================
// Types
// ============================================

/** Navigation options */
export interface NavigateOptions {
  /** Replace current history entry instead of pushing */
  replace?: boolean;
  /** Navigation state to pass to the route */
  state?: Record<string, unknown>;
}

/** Optional query params when opening the empty session workspace */
export interface GoToNewSessionOptions {
  projectId?: string;
  workflowId?: string;
  draftId?: string;
  preserveActiveDraft?: boolean;
}

// ============================================
// Hook Implementation
// ============================================

export interface UseAppNavigationReturn {
  // Generic navigation
  navigateTo: (path: string, options?: NavigateOptions) => void;

  // Convenience methods
  goToSettings: (options?: SettingsPathOptions) => void;
  goToIntegrations: (options?: {
    category?: IntegrationsCategorySegment;
    modelsTab?: string;
    skillsetTab?: ExternalSkillsetsTab;
  }) => void;
  goToNewSession: (options?: GoToNewSessionOptions) => void;
}

export function useAppNavigation(): UseAppNavigationReturn {
  const navigate = useNavigate();
  // Session lifecycle atoms (for goToNewSession)
  const dispatchClearSession = useSetAtom(clearSessionAtom);
  const setActiveSessionId = useSetAtom(activeSessionIdAtom);
  const setWorkstationActiveSessionId = useSetAtom(
    workstationActiveSessionIdAtom
  );
  const resetChatPanelSessionSurface = useSetAtom(
    resetChatPanelSessionSurfaceAtom
  );
  const startNewSessionCreatorDraft = useSetAtom(
    startNewSessionCreatorDraftAtom
  );
  const selectSessionCreatorDraft = useSetAtom(selectSessionCreatorDraftAtom);
  const promoteActiveSessionCreatorDraft = useSetAtom(
    promoteActiveSessionCreatorDraftAtom
  );

  // ========================================
  // Core Navigation Functions
  // ========================================

  /**
   * Generic navigation - just navigates, no tab management
   */
  const navigateTo = useCallback(
    (path: string, options?: NavigateOptions) => {
      promoteActiveSessionCreatorDraft();
      preloadRouteByPath(path);
      navigate(path, {
        replace: options?.replace,
        state: options?.state,
      });
    },
    [navigate, promoteActiveSessionCreatorDraft]
  );

  // ========================================
  // Convenience Methods
  // ========================================

  const goToSettings = useCallback(
    (options?: SettingsPathOptions) => {
      navigateTo(buildSettingsPath(options));
    },
    [navigateTo]
  );

  const goToIntegrations = useCallback(
    (options?: {
      category?: IntegrationsCategorySegment;
      modelsTab?: string;
      skillsetTab?: ExternalSkillsetsTab;
    }) => {
      const category = options?.category ?? "externalSkillsets";

      if (category === "externalSkillsets") {
        const built = buildExternalSkillsetsPath({ tab: options?.skillsetTab });
        const [pathname, existingSearch = ""] = built.split("?");
        const search = new URLSearchParams(existingSearch);
        if (options?.modelsTab) search.set("modelsTab", options.modelsTab);
        const query = search.toString();
        navigateTo(query ? `${pathname}?${query}` : pathname);
        return;
      }

      const basePath = buildIntegrationsPath({ category });
      const search = new URLSearchParams();
      if (options?.modelsTab) search.set("modelsTab", options.modelsTab);
      const query = search.toString();
      const path = query ? `${basePath}?${query}` : basePath;
      navigateTo(path);
    },
    [navigateTo]
  );

  const goToNewSession = useCallback(
    (options?: GoToNewSessionOptions) => {
      dispatchClearSession();
      resetChatPanelSessionSurface();
      // Starting a session changes chat identity, not the WorkStation layout.
      setActiveSessionId(null);
      setWorkstationActiveSessionId(null);

      if (!options?.preserveActiveDraft) {
        promoteActiveSessionCreatorDraft();
      }

      if (options?.draftId) {
        selectSessionCreatorDraft(options.draftId);
      } else if (!options?.preserveActiveDraft) {
        startNewSessionCreatorDraft();
      }

      const params = new URLSearchParams();
      if (options?.projectId) {
        params.set("projectId", options.projectId);
      }
      if (options?.workflowId) {
        params.set("workflowId", options.workflowId);
      }
      const query = params.toString();
      // Host-specific routes reopen My Station on entry. Session creation must
      // preserve the current station and the user's chat-only/split layout.
      const path = query
        ? `${ROUTES.workStation.base.path}?${query}`
        : ROUTES.workStation.base.path;
      navigate(path);
    },
    [
      dispatchClearSession,
      resetChatPanelSessionSurface,
      setActiveSessionId,
      setWorkstationActiveSessionId,
      promoteActiveSessionCreatorDraft,
      selectSessionCreatorDraft,
      startNewSessionCreatorDraft,
      navigate,
    ]
  );

  // ========================================
  // Return
  // ========================================

  return {
    // Core navigation
    navigateTo,

    // Convenience methods
    goToSettings,
    goToIntegrations,
    goToNewSession,
  };
}

export default useAppNavigation;
