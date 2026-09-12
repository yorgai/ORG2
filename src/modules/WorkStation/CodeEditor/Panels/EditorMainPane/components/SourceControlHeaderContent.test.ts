import type { TFunction } from "i18next";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";

import type { WorkStationTab } from "@src/store/workstation/tabs";

import { SourceControlHeaderContent } from "./SourceControlHeaderContent";

vi.mock("@src/components/Button", () => ({
  default: ({
    title,
    disabled,
    "aria-label": label,
  }: {
    title?: string;
    disabled?: boolean;
    "aria-label"?: string;
  }) =>
    createElement("button", {
      "data-title": title,
      "aria-label": label,
      disabled,
    }),
}));

vi.mock("@src/components/TabPill", () => ({
  default: ({
    activeTab,
    tabs,
  }: {
    activeTab: string;
    tabs: Array<{ key: string }>;
  }) =>
    createElement("div", {
      "data-active-tab": activeTab,
      "data-tabs": tabs.map((tab) => tab.key).join(","),
    }),
}));

const t = ((key: string) => key) as TFunction;

function sourceControlTab(mode: "focus" | "all-changes"): WorkStationTab {
  return {
    id: "source-control:changes",
    type: "source-control",
    title: "Review",
    data: {
      mode,
      staged: false,
      fileCount: 1,
      focusPath: null,
      historySelection: null,
    },
  } as WorkStationTab;
}

vi.mock("./SourceControlDiffSettingsMenu", () => ({
  SourceControlDiffSettingsMenu: () =>
    createElement("button", { "data-menu": "diff-settings" }),
}));

function renderHeader(
  mode: "focus" | "all-changes",
  focusPath: string | null = null,
  navigationTotal = focusPath ? 1 : 0
): string {
  const tab = sourceControlTab(mode);
  tab.data.focusPath = focusPath;
  return renderToStaticMarkup(
    createElement(SourceControlHeaderContent, {
      activeTab: tab,
      sourceControlFilterMode: "uncommitted",
      showSourceControlModePill: true,
      gitReviewNavigationTotal: navigationTotal,
      selectedIssue: null,
      sourceControlRefreshSpinClass: undefined,
      diffViewMode: "split",
      t,
      onDiffViewModeChange: vi.fn(),
      onModeChange: vi.fn(),
      onOpenHistoryInNewTab: vi.fn(),
      onReviewPrevFile: vi.fn(),
      onReviewNextFile: vi.fn(),
      onCollapseAll: vi.fn(),
      onRefresh: vi.fn(),
    })
  );
}

describe("SourceControlHeaderContent diff view controls", () => {
  it("shows the shared unified/split control in All Changes", () => {
    const markup = renderHeader("all-changes");

    expect(markup).toContain('aria-label="workstation.switchToUnifiedDiff"');
    expect(markup).not.toContain('data-tabs="unified,split"');
  });

  it("places focused diff controls after navigation and its separator", () => {
    const markup = renderHeader("focus", "src/index.ts");
    const next = markup.indexOf('aria-label="common:actions.reviewNextFile"');
    const separator = markup.indexOf('role="separator"', next);
    const split = markup.indexOf(
      'aria-label="workstation.switchToUnifiedDiff"'
    );
    expect(markup).not.toContain("disabled");
    expect(next).toBeGreaterThan(-1);
    expect(separator).toBeGreaterThan(next);
    expect(split).toBeGreaterThan(separator);
    expect(markup.slice(split)).not.toContain('role="separator"');
  });

  it("keeps aggregate split and menu adjacent after collapse controls", () => {
    const markup = renderHeader("all-changes");
    const collapse = markup.indexOf('data-title="actions.collapseAll"');
    const separator = markup.indexOf('role="separator"', collapse);
    const split = markup.indexOf(
      'aria-label="workstation.switchToUnifiedDiff"'
    );
    const menu = markup.indexOf('data-menu="diff-settings"');
    expect(separator).toBeGreaterThan(collapse);
    expect(split).toBeGreaterThan(separator);
    expect(menu).toBeGreaterThan(split);
    expect(markup.slice(split, menu)).not.toContain('role="separator"');
  });

  it.each([0, 3])(
    "keeps empty Focus arrows disabled and its menu visible with %i review files",
    (total) => {
      const markup = renderHeader("focus", null, total);
      for (const action of ["reviewPreviousFile", "reviewNextFile"]) {
        expect(markup).toContain(
          `aria-label="common:actions.${action}" disabled=""`
        );
      }
      expect(markup).toContain('data-menu="diff-settings"');
      expect(markup.indexOf('role="separator"')).toBeLessThan(
        markup.indexOf('data-menu="diff-settings"')
      );
    }
  );

  it("disables navigation when a selected file has no review sequence", () => {
    const markup = renderHeader("focus", "src/index.ts", 0);
    expect(markup).toContain(
      'aria-label="common:actions.reviewNextFile" disabled=""'
    );
    expect(markup).not.toContain('data-menu="diff-settings"');
  });

  it("keeps the aggregate diff control out of empty Focus mode", () => {
    const markup = renderHeader("focus");

    expect(markup).not.toContain("workstation.switchToUnifiedDiff");
    expect(markup).toContain('data-tabs="focus,all-changes"');
  });
});
