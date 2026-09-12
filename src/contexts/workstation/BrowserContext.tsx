/**
 * Browser Context
 *
 * Provides browser session state management across Browser page and BrowserExtraSidebar
 *
 * Performance optimizations:
 * - Uses startTransition for non-urgent state updates to avoid blocking UI
 * - Defers cascading state updates with queueMicrotask
 */
import React, {
  createContext,
  startTransition,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
} from "react";
import { v4 as uuidv4 } from "uuid";

import type { BrowserSession } from "@src/engines/BrowserCore/types";
import {
  NEW_PRIVATE_TAB_TITLE,
  NEW_TAB_TITLE,
} from "@src/store/workstation/browser/tabs";

interface BrowserContextValue {
  sessions: BrowserSession[];
  activeSessionId: string;
  filterValue: string;
  setFilterValue: (value: string) => void;
  handleSessionClick: (sessionId: string) => void;
  handleAddSession: (url?: string, incognito?: boolean) => string;
  handleCloseSession: (sessionId: string) => void;
  updateSession: (sessionId: string, updates: Partial<BrowserSession>) => void;
  /** Force save sessions to localStorage (call when switching away from browser) */
  forceSave: () => void;
}

const BrowserContext = createContext<BrowserContextValue | null>(null);

// Helper function to extract title from URL
const getTitleFromUrl = (url: string): string => {
  if (!url) return NEW_TAB_TITLE;
  try {
    const urlObj = new URL(url);
    return urlObj.hostname || NEW_TAB_TITLE;
  } catch {
    return NEW_TAB_TITLE;
  }
};

// Browser sessions are the durable source for live browser resources. The
// WorkStation `browserTabsAtom` is a shared-resource projection synchronized
// from this state; it must never be used as a second persistence owner.
const BROWSER_SESSIONS_STORAGE_KEY = "browser-explorer-sessions";

// Load sessions from localStorage
const loadFromStorage = (): {
  sessions: BrowserSession[];
  activeSessionId: string;
} | null => {
  try {
    const stored = localStorage.getItem(BROWSER_SESSIONS_STORAGE_KEY);
    const parsed = stored ? JSON.parse(stored) : null;
    const sessions = Array.isArray(parsed?.sessions) ? parsed.sessions : [];
    const activeSessionId =
      typeof parsed?.activeSessionId === "string" ? parsed.activeSessionId : "";
    if (sessions.length > 0) {
      const validActiveSessionId = sessions.some(
        (session: BrowserSession) => session.id === activeSessionId
      )
        ? activeSessionId
        : (sessions[0]?.id ?? "");
      return { sessions, activeSessionId: validActiveSessionId };
    }
  } catch {
    return null;
  }
  return null;
};

// Save sessions to localStorage
const saveToStorage = (sessions: BrowserSession[], activeSessionId: string) => {
  try {
    localStorage.setItem(
      BROWSER_SESSIONS_STORAGE_KEY,
      JSON.stringify({ sessions, activeSessionId })
    );
  } catch {
    // Ignore storage errors
  }
};

// Default initial state for browser tab - starts empty like CodeEditor
const getDefaultState = (): {
  sessions: BrowserSession[];
  activeSessionId: string;
  filterValue: string;
} => {
  // Try to load from localStorage first
  const stored = loadFromStorage();
  if (stored) {
    return { ...stored, filterValue: "" };
  }

  // No default session - user clicks + to create tabs
  return {
    sessions: [],
    activeSessionId: "",
    filterValue: "",
  };
};

export const BrowserProvider: React.FC<{ children: React.ReactNode }> = ({
  children,
}) => {
  const [sessions, setSessions] = useState<BrowserSession[]>(
    () => getDefaultState().sessions
  );
  const [activeSessionId, setActiveSessionId] = useState<string>(
    () => getDefaultState().activeSessionId
  );
  const [filterValue, setFilterValue] = useState<string>("");

  // Ensure active session exists (or is empty if no sessions)
  useEffect(() => {
    const activeSessionExists = sessions.some(
      (session) => session.id === activeSessionId
    );
    if (!activeSessionExists) {
      // Use startTransition to avoid blocking UI during correction
      startTransition(() => {
        setActiveSessionId(sessions.length > 0 ? sessions[0].id : "");
      });
    }
  }, [sessions, activeSessionId]);

  // Persist state to localStorage
  useEffect(() => {
    if (sessions.length > 0) {
      saveToStorage(sessions, activeSessionId);
    } else {
      // Clear storage when all sessions are closed
      localStorage.removeItem(BROWSER_SESSIONS_STORAGE_KEY);
    }
  }, [sessions, activeSessionId]);

  // Add a new session
  const handleAddSession = useCallback((url?: string, incognito = false) => {
    const newSessionId = uuidv4();
    const newSession: BrowserSession = {
      id: newSessionId,
      title: url
        ? getTitleFromUrl(url)
        : incognito
          ? NEW_PRIVATE_TAB_TITLE
          : NEW_TAB_TITLE,
      url: url || "",
      history: url ? [url] : [],
      historyIndex: url ? 0 : -1,
      isLoading: false,
      error: null,
      incognito,
    };

    // Keep session list + active id in the same update. Deferring only setSessions
    // (e.g. via startTransition) while setting activeSessionId eagerly lets the
    // "ensure active session exists" effect run with the new id before the new
    // row exists and resets focus to sessions[0].
    setSessions((prev) => [...prev, newSession]);
    setActiveSessionId(newSessionId);
    return newSessionId;
  }, []);

  // Switch to a session
  const handleSessionClick = useCallback((sessionId: string) => {
    setActiveSessionId(sessionId);
  }, []);

  // Close a session
  const handleCloseSession = useCallback(
    (sessionId: string) => {
      // Use startTransition to avoid blocking UI during state update
      startTransition(() => {
        setSessions((prev) => {
          const filtered = prev.filter((session) => session.id !== sessionId);

          // If closing the active session, activate the first remaining session (or clear if none)
          if (sessionId === activeSessionId) {
            if (filtered.length > 0) {
              setActiveSessionId(filtered[0].id);
            } else {
              setActiveSessionId("");
            }
          }

          return filtered;
        });
      });
    },
    [activeSessionId]
  );

  // Update a specific session
  const updateSession = useCallback(
    (sessionId: string, updates: Partial<BrowserSession>) => {
      // Use startTransition for non-urgent updates (like URL/title changes)
      startTransition(() => {
        setSessions((prev) => {
          return prev.map((session) =>
            session.id === sessionId ? { ...session, ...updates } : session
          );
        });
      });
    },
    []
  );

  // Force save to localStorage (for when switching away from browser mode)
  const forceSave = useCallback(() => {
    saveToStorage(sessions, activeSessionId);
  }, [sessions, activeSessionId]);

  const value = useMemo<BrowserContextValue>(
    () => ({
      sessions,
      activeSessionId,
      filterValue,
      setFilterValue,
      handleSessionClick,
      handleAddSession,
      handleCloseSession,
      updateSession,
      forceSave,
    }),
    [
      sessions,
      activeSessionId,
      filterValue,
      handleSessionClick,
      handleAddSession,
      handleCloseSession,
      updateSession,
      forceSave,
    ]
  );

  return (
    <BrowserContext.Provider value={value}>{children}</BrowserContext.Provider>
  );
};

export const useBrowserContext = () => {
  const context = useContext(BrowserContext);
  if (!context) {
    throw new Error("useBrowserContext must be used within BrowserProvider");
  }
  return context;
};

// Optional version that doesn't throw - for GlobalTabsSidebar
export const useBrowserContextOptional = () => {
  return useContext(BrowserContext);
};
