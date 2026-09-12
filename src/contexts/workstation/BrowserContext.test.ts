// @vitest-environment jsdom
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { BrowserProvider, useBrowserContext } from "./BrowserContext";

beforeEach(() => {
  localStorage.clear();
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
});
afterEach(() => {
  localStorage.clear();
  vi.unstubAllGlobals();
});

function SessionCreator() {
  const browser = useBrowserContext();
  return createElement(
    "button",
    {
      onClick: () => browser.handleAddSession("https://example.com"),
    },
    "Open"
  );
}

it("creates and persists new sessions without timestamped browsing history", async () => {
  const persist = vi.spyOn(Storage.prototype, "setItem");
  const container = document.createElement("div");
  const root = createRoot(container);
  try {
    await act(async () =>
      root.render(
        createElement(BrowserProvider, null, createElement(SessionCreator))
      )
    );
    await act(async () => container.querySelector("button")?.click());
    const saved = JSON.parse(
      localStorage.getItem("browser-explorer-sessions") ?? "null"
    );
    expect(saved.sessions).toHaveLength(1);
    expect(saved.sessions[0]).not.toHaveProperty("historyEntries");
    expect(saved.sessions[0].history).toEqual(["https://example.com"]);
    expect(saved.sessions[0].historyIndex).toBe(0);
  } finally {
    await act(async () => root.unmount());
    const keys = persist.mock.calls.map(([key]) => key);
    persist.mockRestore();
    expect(keys).not.toContain("orgii-global-tabs");
  }
});

it("does not migrate away existing stored history entries", async () => {
  const oldEntries = [
    { url: "https://old.example", title: "Old", visitedAt: 1 },
  ];
  localStorage.setItem(
    "browser-explorer-sessions",
    JSON.stringify({
      sessions: [
        {
          id: "old",
          url: "https://old.example",
          title: "Old",
          history: ["https://old.example"],
          historyIndex: 0,
          historyEntries: oldEntries,
          isLoading: false,
          error: null,
        },
      ],
      activeSessionId: "old",
    })
  );
  const root = createRoot(document.createElement("div"));
  try {
    await act(async () =>
      root.render(
        createElement(BrowserProvider, null, createElement(SessionCreator))
      )
    );
    const saved = JSON.parse(
      localStorage.getItem("browser-explorer-sessions") ?? "null"
    );
    expect(saved.sessions[0].historyEntries).toEqual(oldEntries);
  } finally {
    await act(async () => root.unmount());
  }
});
