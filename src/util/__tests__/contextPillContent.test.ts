// @vitest-environment jsdom
import { beforeEach, expect, it, vi } from "vitest";

import {
  loadBrowserPillContent,
  waitForPendingPills,
} from "../contextPillContent";

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  storePillText: vi.fn(),
  get: vi.fn(),
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("@src/api/http/project", () => ({ projectApi: {} }));
vi.mock("@src/config/pillTokens", () => ({
  storePillText: mocks.storePillText,
  capPillText: vi.fn(),
}));
vi.mock("@src/store/workstation/tabs", () => ({ mainPaneTabsAtom: {} }));
vi.mock("@src/util/core/state/instrumentedStore", () => ({
  getInstrumentedStore: () => ({ get: mocks.get }),
}));

beforeEach(() => {
  vi.resetAllMocks();
  localStorage.clear();
  mocks.invoke.mockRejectedValue(new Error("Webview not mounted"));
});

it("loads browser mention URLs from the current workstation tab when its webview is unavailable", async () => {
  mocks.get.mockReturnValue([
    {
      type: "browser-session",
      data: { sessionId: "browser-1", url: "https://current.example" },
    },
  ]);
  loadBrowserPillContent("browser-1", "browser://mention");
  await waitForPendingPills();
  expect(mocks.invoke).toHaveBeenCalledWith("get_full_html_document", {
    label: "browser-session-browser-1",
  });
  expect(mocks.storePillText).toHaveBeenCalledWith(
    "browser://mention",
    "URL: https://current.example"
  );
});

it("does not resurrect a closed browser URL from retired global-tab records", async () => {
  localStorage.setItem(
    "orgii-global-tabs",
    JSON.stringify({
      browser: [{ id: "closed", url: "https://stale.example" }],
    })
  );
  mocks.get.mockReturnValue([]);
  loadBrowserPillContent("closed", "browser://closed");
  await waitForPendingPills();
  expect(mocks.storePillText).not.toHaveBeenCalled();
});

it("still loads page text when a browser has no recorded URL", async () => {
  mocks.get.mockReturnValue([]);
  mocks.invoke.mockResolvedValue("<body>Current page</body>");
  const textGetter = vi
    .spyOn(DOMParser.prototype, "parseFromString")
    .mockReturnValue({
      body: { innerText: "Current page" },
    } as Document);
  try {
    loadBrowserPillContent("browser-1", "browser://text");
    await waitForPendingPills();
    expect(mocks.storePillText).toHaveBeenCalledWith(
      "browser://text",
      "Current page"
    );
  } finally {
    textGetter.mockRestore();
  }
});
