// @vitest-environment jsdom
import { Provider, createStore } from "jotai";
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";

import { FileHeaderToolbarContext } from "./FileHeaderToolbarContext";
import { FileHeader } from "./index";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));
vi.mock("./BreadcrumbFileHeader", () => ({
  default: () => createElement("span", null, "file breadcrumb"),
}));
vi.mock("./FileHeaderMoreMenu", () => ({
  FileHeaderMoreMenu: ({ onSearchClick }: { onSearchClick: () => void }) =>
    createElement("button", { onClick: onSearchClick }, "file menu"),
}));

it("moves the live file menu into its host toolbar and releases it on unmount", () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  const container = document.createElement("div");
  const toolbar = document.createElement("div");
  document.body.append(container, toolbar);
  const root = createRoot(container);
  const onSearch = vi.fn();
  const store = createStore();
  const render = (target: HTMLElement | null) =>
    act(() =>
      root.render(
        createElement(
          Provider,
          { store },
          createElement(
            FileHeaderToolbarContext.Provider,
            { value: target },
            createElement(FileHeader, {
              filePath: "src/index.ts",
              useFileTypeIcon: false,
              viewMode: "split",
              onViewModeChange: vi.fn(),
              onSearchRequest: onSearch,
            })
          )
        )
      )
    );
  try {
    render(toolbar);
    expect(container.textContent).toContain("file breadcrumb");
    expect(container.textContent).not.toContain("file menu");
    expect(
      container.querySelector('[aria-label="workstation.switchToUnifiedDiff"]')
    ).toBeNull();
    act(() => toolbar.querySelector("button")!.click());
    expect(onSearch).toHaveBeenCalledOnce();
    render(null);
    expect(toolbar.childElementCount).toBe(0);
    expect(container.textContent).toContain("file menu");
    expect(
      container.querySelector('[aria-label="workstation.switchToUnifiedDiff"]')
    ).not.toBeNull();
    render(toolbar);
  } finally {
    act(() => root.unmount());
    expect(toolbar.childElementCount).toBe(0);
    container.remove();
    toolbar.remove();
    vi.unstubAllGlobals();
  }
});
