// @vitest-environment jsdom
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import WeeklyQuotaHistoryPanel from "./WeeklyQuotaHistoryPanel";

const mocks = vi.hoisted(() => ({
  state: {
    accounts: [] as unknown[],
    loading: false,
    error: false,
    observedAt: 10000,
    refresh: vi.fn(),
  },
}));
vi.mock("@src/hooks/keyVault/useWeeklyQuotaHistory", () => ({
  useWeeklyQuotaHistory: () => mocks.state,
}));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (_key: string, opts: { defaultValue?: string }) => opts?.defaultValue,
    i18n: { language: "en" },
  }),
}));
vi.mock("./RuntimeSectionHeader", () => ({
  RuntimeSectionHeader: ({
    title,
    children,
  }: {
    title: string;
    children: unknown;
  }) => createElement("div", null, title, children as never),
  RuntimeRefreshButton: ({ onRefresh }: { onRefresh: () => void }) =>
    createElement("button", { onClick: onRefresh }, "Read history"),
}));
vi.mock("@src/components/Chart", () => ({
  CHART_AXIS_TICK: {},
  CHART_GRID_STROKE: "",
  CHART_MARGIN: {},
  CHART_TOOLTIP: {},
}));
vi.mock("@src/components/Select", () => ({
  default: ({
    options,
    onChange,
  }: {
    options: { value: string; label: string }[];
    onChange: (v: string) => void;
  }) =>
    createElement(
      "select",
      {
        onChange: (event: { target: { value: string } }) =>
          onChange(event.target.value),
      },
      options.map((o) =>
        createElement("option", { value: o.value, key: o.value }, o.label)
      )
    ),
}));
vi.mock("recharts", () => ({
  ResponsiveContainer: ({ children }: { children: unknown }) => children,
  BarChart: ({ children }: { children: unknown }) =>
    createElement("div", { "data-testid": "quota-chart" }, children as never),
  Bar: ({ children }: { children: unknown }) => children,
  Cell: ({ className }: { className: string }) =>
    createElement("span", { "data-testid": "quota-bar", className }),
  CartesianGrid: () => null,
  XAxis: () => null,
  YAxis: () => null,
  Tooltip: () => null,
}));
beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
});
const roots: ReturnType<typeof createRoot>[] = [];
afterEach(async () => {
  await act(async () => roots.splice(0).forEach((root) => root.unmount()));
  mocks.state.accounts = [];
  mocks.state.error = false;
  vi.unstubAllGlobals();
});
async function mount() {
  const element = document.createElement("div");
  const root = createRoot(element);
  roots.push(root);
  await act(async () => root.render(createElement(WeeklyQuotaHistoryPanel)));
  return element;
}
const account = (index: number) => ({
  keyId: String(index),
  name: `Account ${index}`,
  provider: "codex",
  status: "ok",
  points: [{ capturedAt: 10000, remainingPercent: 80, resetAt: null }],
});
describe("WeeklyQuotaHistoryPanel", () => {
  it("mounts only the selected account chart for a 128-account history", async () => {
    mocks.state.accounts = Array.from({ length: 128 }, (_, i) => account(i));
    const element = await mount();
    expect(
      element.querySelectorAll('[data-testid="quota-chart"]')
    ).toHaveLength(1);
    const select = element.querySelector("select")!;
    await act(async () => {
      select.value = "127";
      select.dispatchEvent(new Event("change", { bubbles: true }));
    });
    expect(
      element.querySelectorAll('[data-testid="quota-chart"]')
    ).toHaveLength(1);
    expect(element.textContent).toContain("Account 127");
  });
  it("browses retained weeks locally and resets the range on account change", async () => {
    const current = account(0);
    current.points.unshift({
      capturedAt: 10000 - 8 * 86400,
      remainingPercent: 20,
      resetAt: null,
    });
    mocks.state.accounts = [current, account(1)];
    mocks.state.refresh.mockClear();
    const element = await mount();
    const previous = () =>
      element.querySelector<HTMLButtonElement>(
        'button[aria-label="Previous week"]'
      )!;
    const next = () =>
      element.querySelector<HTMLButtonElement>(
        'button[aria-label="Next week"]'
      )!;
    expect(next().disabled).toBe(true);
    expect(previous().disabled).toBe(false);
    await act(async () => previous().click());
    expect(element.textContent).toContain("20%");
    expect(previous().disabled).toBe(true);
    expect(next().disabled).toBe(false);
    await act(async () => next().click());
    expect(element.textContent).toContain("80%");
    expect(next().disabled).toBe(true);
    await act(async () => previous().click());
    const select = element.querySelector("select")!;
    await act(async () => {
      select.value = "1";
      select.dispatchEvent(new Event("change", { bubbles: true }));
    });
    expect(element.textContent).toContain("80%");
    expect(next().disabled).toBe(true);
    expect(previous().disabled).toBe(true);
    expect(mocks.state.refresh).not.toHaveBeenCalled();
  });
  it("uses the shared quota colors for remaining percentages", async () => {
    mocks.state.accounts = [
      {
        ...account(0),
        points: [0, 9, 10, 49, 50, 100].map((remainingPercent, index) => ({
          capturedAt: 10000 - (5 - index) * 3600,
          remainingPercent,
          resetAt: null,
        })),
      },
    ];
    const element = await mount();
    expect(
      Array.from(
        element.querySelectorAll('[data-testid="quota-bar"]'),
        (bar) => bar.className
      )
    ).toEqual([
      "text-danger-6",
      "text-danger-6",
      "text-warning-6",
      "text-warning-6",
      "text-success-6",
      "text-success-6",
    ]);
  });
  it("does not mistake a failed load for a disconnected account list", async () => {
    mocks.state.error = true;
    const element = await mount();
    expect(element.textContent).toContain("unavailable");
    expect(element.textContent).not.toContain("Connect a Claude Code");
  });
});
