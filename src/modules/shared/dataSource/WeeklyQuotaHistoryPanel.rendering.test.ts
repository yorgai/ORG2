// @vitest-environment jsdom
import { type ReactElement, act, cloneElement, createElement } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";

import WeeklyQuotaHistoryPanel from "./WeeklyQuotaHistoryPanel";

const now = 1_788_970_200;
vi.mock("@src/hooks/keyVault/useWeeklyQuotaHistory", () => ({
  useWeeklyQuotaHistory: () => ({
    accounts: [
      {
        keyId: "codex",
        name: "OpenAI",
        provider: "codex",
        status: "ok",
        samplingEnabled: false,
        points: [100, 95, 87].map((remainingPercent, index) => ({
          capturedAt: now - [10, 7, 1][index] * 3600,
          remainingPercent,
          resetAt: null,
        })),
      },
    ],
    observedAt: now + 3 * 3600,
    loading: false,
    error: false,
    refresh: vi.fn(),
  }),
}));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (_key: string, options: { defaultValue: string }) =>
      options.defaultValue,
    i18n: { language: "zh" },
  }),
}));
vi.mock("./RuntimeSectionHeader", () => ({
  RuntimeSectionHeader: () => null,
  RuntimeRefreshButton: () => null,
}));
vi.mock("@src/components/Select", () => ({ default: () => null }));
// Give the real chart a measurable viewport without mocking its SVG renderer.
vi.mock("recharts", async (importOriginal) => ({
  ...(await importOriginal<typeof import("recharts")>()),
  ResponsiveContainer: ({ children }: { children: ReactElement }) =>
    cloneElement(children, { width: 900, height: 160 } as object),
}));
let root: ReturnType<typeof createRoot>;
afterEach(async () => {
  await act(async () => root?.unmount());
  vi.unstubAllGlobals();
});
it("renders visible bars for sparse recent samples on the seven-day time axis", async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  const container = document.createElement("div");
  root = createRoot(container);
  await act(async () => root.render(createElement(WeeklyQuotaHistoryPanel)));
  expect(container.querySelector('[aria-live="polite"]')?.textContent).toMatch(
    /^Sep \d+ – Sep \d+$/
  );
  expect(container.textContent).not.toMatch(/[月日]/);
  expect(container.textContent).toMatch(/87% · Sep \d+, \d{2}:\d{2}/);
  expect(container.textContent).not.toContain("OpenAI · codex");
  const notices = Array.from(container.querySelectorAll('[role="status"]'));
  expect(
    notices.some((notice) => notice.textContent?.includes("over two hours old"))
  ).toBe(false);
  expect(
    notices.some((notice) => notice.textContent?.includes("Sampling paused"))
  ).toBe(true);
  expect(notices.every((notice) => notice.tagName !== "P")).toBe(true);
  const bars = container.querySelectorAll(".recharts-bar-rectangle path");
  expect(bars).toHaveLength(3);
  for (const bar of bars) {
    expect(Number(bar.getAttribute("width"))).toBeGreaterThanOrEqual(1);
    expect(Number(bar.getAttribute("height"))).toBeGreaterThan(0);
    expect(Number(bar.getAttribute("x"))).toBeGreaterThanOrEqual(40);
    expect(Number(bar.getAttribute("x"))).toBeLessThan(900);
  }
});
