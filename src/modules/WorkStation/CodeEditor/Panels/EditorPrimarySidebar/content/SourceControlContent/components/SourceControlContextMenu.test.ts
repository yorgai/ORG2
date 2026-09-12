// @vitest-environment jsdom
import i18next from "i18next";
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { beforeEach, describe, expect, it, vi } from "vitest";

import deCommon from "@src/i18n/locales/de/common.json";
import enCommon from "@src/i18n/locales/en/common.json";
import esCommon from "@src/i18n/locales/es/common.json";
import frCommon from "@src/i18n/locales/fr/common.json";
import jaCommon from "@src/i18n/locales/ja/common.json";
import koCommon from "@src/i18n/locales/ko/common.json";
import plCommon from "@src/i18n/locales/pl/common.json";
import ptCommon from "@src/i18n/locales/pt/common.json";
import ruCommon from "@src/i18n/locales/ru/common.json";
import trCommon from "@src/i18n/locales/tr/common.json";
import viCommon from "@src/i18n/locales/vi/common.json";
import zhHantCommon from "@src/i18n/locales/zh-Hant/common.json";
import zhCommon from "@src/i18n/locales/zh/common.json";
import type { GitFile } from "@src/types/git/types";
import { popupNativeMenu } from "@src/util/platform/tauri/nativeMenuPopup";

import SourceControlContextMenu, {
  getSourceControlContextMenuActionLabels,
  resolveConflictsForFiles,
} from "./SourceControlContextMenu";

vi.mock("@src/util/platform/tauri/nativeMenuPopup", () => ({
  popupNativeMenu: vi.fn(),
}));

const resources = {
  de: { common: deCommon },
  en: { common: enCommon },
  es: { common: esCommon },
  fr: { common: frCommon },
  ja: { common: jaCommon },
  ko: { common: koCommon },
  pl: { common: plCommon },
  pt: { common: ptCommon },
  ru: { common: ruCommon },
  tr: { common: trCommon },
  vi: { common: viCommon },
  zh: { common: zhCommon },
  "zh-Hant": { common: zhHantCommon },
};

beforeEach(async () => {
  await i18next.init({
    lng: "en",
    fallbackLng: false,
    resources,
    defaultNS: "common",
    initImmediate: false,
  });
});

function gitFile(path: string): GitFile {
  return {
    id: path,
    path,
    status: "modified",
    staged: false,
    additions: 0,
    deletions: 0,
  };
}

describe("getSourceControlContextMenuActionLabels", () => {
  it("keeps existing single-file labels", () => {
    expect(
      getSourceControlContextMenuActionLabels({
        isDirectory: false,
        isStaged: false,
        changeCount: 1,
      })
    ).toMatchObject({
      stageToggle: "Stage Changes",
      markResolved: "Mark as Resolved (Stage)",
      discard: "Discard Changes",
    });
  });

  it("shows singular folder change counts", () => {
    expect(
      getSourceControlContextMenuActionLabels({
        isDirectory: true,
        isStaged: false,
        changeCount: 1,
      })
    ).toMatchObject({
      stageToggle: "Stage Changes (1)",
      markResolved: "Mark as Resolved (1)",
      discard: "Discard Changes (1)",
    });
  });

  it("shows plural folder change counts", () => {
    expect(
      getSourceControlContextMenuActionLabels({
        isDirectory: true,
        isStaged: true,
        changeCount: 3,
      })
    ).toMatchObject({
      stageToggle: "Unstage Changes (3)",
      markResolved: "Mark as Resolved (3)",
      discard: "Discard Changes (3)",
    });
  });
});

describe("resolveConflictsForFiles", () => {
  it("dispatches conflict resolution for every file in a folder", async () => {
    const dispatch = vi.fn().mockResolvedValue(undefined);

    await resolveConflictsForFiles(
      dispatch,
      [gitFile("src/a.ts"), gitFile("src/b.ts")],
      "ours"
    );

    expect(dispatch).toHaveBeenCalledTimes(2);
    expect(dispatch).toHaveBeenNthCalledWith(
      1,
      "git.resolveConflict",
      { path: "src/a.ts", strategy: "ours" },
      "user"
    );
    expect(dispatch).toHaveBeenNthCalledWith(
      2,
      "git.resolveConflict",
      { path: "src/b.ts", strategy: "ours" },
      "user"
    );
  });
});

describe("localized native source-control menu", () => {
  it.each(Object.keys(resources))(
    "resolves every file action in %s without fallback",
    async (language) => {
      await i18next.changeLanguage(language);
      for (const isDirectory of [false, true]) {
        for (const isStaged of [false, true]) {
          const labels = getSourceControlContextMenuActionLabels({
            isDirectory,
            isStaged,
            changeCount: 3,
          });
          for (const label of Object.values(labels)) {
            expect(label).toBeTruthy();
            expect(label).not.toContain("sourceControl.fileMenu");
            expect(label).not.toContain("{{count}}");
          }
        }
      }
    }
  );

  it("builds Chinese open actions with distinct destinations and preserves their dispatch", async () => {
    await i18next.changeLanguage("zh");
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    const container = document.createElement("div");
    const root = createRoot(container);
    const onSelect = vi.fn();
    const dispatch = vi.fn().mockResolvedValue(undefined);
    let items: Awaited<
      ReturnType<Parameters<typeof popupNativeMenu>[0]["buildItems"]>
    > = [];
    vi.mocked(popupNativeMenu).mockImplementation(async (options) => {
      items = await options.buildItems();
      return { status: "closed" };
    });
    try {
      await act(async () =>
        root.render(
          createElement(SourceControlContextMenu, {
            file: gitFile("src/a.ts"),
            repoPath: "/repo",
            isConflictFile: false,
            onSelect,
            dispatch,
            onClose: vi.fn(),
          })
        )
      );
      const actions = items.filter((item) => "text" in item);
      expect(actions.map((item) => item.text)).toEqual([
        "查看更改",
        "在新标签页中打开文件",
        "暂存更改",
        "放弃更改",
        i18next.t("actions.copyPath"),
        i18next.t("actions.copyRelativePath"),
        expect.any(String),
      ]);
      expect(actions[0]).toHaveProperty("action");
      if ("action" in actions[0]) await actions[0].action?.("view-changes");
      expect(onSelect).toHaveBeenCalledWith("src/a.ts");
      expect(dispatch).not.toHaveBeenCalled();
      expect(actions[1]).toHaveProperty("action");
      if ("action" in actions[1]) await actions[1].action?.("open-file");
      expect(dispatch).toHaveBeenCalledWith(
        "file.openAtLine",
        { path: "/repo/src/a.ts", line: 1 },
        "user"
      );
    } finally {
      act(() => root.unmount());
      vi.unstubAllGlobals();
    }
  });
});
