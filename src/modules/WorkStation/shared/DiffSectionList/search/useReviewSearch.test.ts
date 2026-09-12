// @vitest-environment jsdom
import { act, createElement, useRef } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { CURRENT_SHORTCUT_PLATFORM } from "@src/config/keyboard/shortcutBindings";

import type { ReviewSearchFile } from "./reviewSearchTypes";
import { useReviewSearch } from "./useReviewSearch";

const mocks = vi.hoisted(() => ({ card: vi.fn(), navigate: vi.fn() }));
vi.mock("@src/components/FindCard", () => ({
  default: (props: unknown) => {
    mocks.card(props);
    return null;
  },
}));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));
const workers: FakeWorker[] = [];
class FakeWorker {
  onmessage: ((event: MessageEvent) => void) | null = null;
  onerror: (() => void) | null = null;
  postMessage = vi.fn();
  terminate = vi.fn();
  constructor() {
    workers.push(this);
  }
  reply() {
    const request = this.postMessage.mock.calls.at(-1)![0];
    this.onmessage?.({
      data: {
        id: request.id,
        matches: [{ path: "b", side: "old", from: 0, to: 3 }],
      },
    } as MessageEvent);
  }
}
const files = [
  { path: "a", oldContent: "old", newContent: "new" },
  { path: "b", oldContent: "old", newContent: "new" },
];
let loadFile: ((path: string) => Promise<ReviewSearchFile | null>) | undefined;
function Harness() {
  const ref = useRef<HTMLDivElement>(null);
  const search = useReviewSearch({
    enabled: true,
    loadFile,
    files,
    containerRef: ref,
    onNavigate: mocks.navigate,
  });
  // eslint-disable-next-line react-hooks/refs -- createElement forwards the ref to React without reading it
  return createElement("div", { ref }, createElement("input"), search.card);
}
let root: ReturnType<typeof createRoot>, host: HTMLDivElement;
const env = globalThis as typeof globalThis & {
  IS_REACT_ACT_ENVIRONMENT?: boolean;
};
let previous: boolean | undefined;
const card = () => mocks.card.mock.calls.at(-1)![0];
const advance = async (ms: number) => {
  await act(async () => vi.advanceTimersByTimeAsync(ms));
};
beforeEach(() => {
  previous = env.IS_REACT_ACT_ENVIRONMENT;
  env.IS_REACT_ACT_ENVIRONMENT = true;
  vi.useFakeTimers();
  vi.clearAllMocks();
  workers.length = 0;
  loadFile = undefined;
  vi.stubGlobal("Worker", FakeWorker);
  vi.spyOn(HTMLElement.prototype, "getClientRects").mockReturnValue([
    {},
  ] as unknown as DOMRectList);
  vi.spyOn(document, "visibilityState", "get").mockReturnValue("visible");
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  act(() => root.render(createElement(Harness)));
  const input = host.querySelector("input")!;
  input.focus();
  act(() =>
    input.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "f",
        bubbles: true,
        cancelable: true,
        metaKey: CURRENT_SHORTCUT_PLATFORM === "mac",
        ctrlKey: CURRENT_SHORTCUT_PLATFORM !== "mac",
      })
    )
  );
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  vi.useRealTimers();
  env.IS_REACT_ACT_ENVIRONMENT = previous;
});
describe("review search lifecycle", () => {
  it("reports rejected worker dispatch and releases the scan", async () => {
    let resolveRead!: (file: ReviewSearchFile) => void;
    loadFile = () =>
      new Promise<ReviewSearchFile>((resolve) => {
        resolveRead = resolve;
      });
    act(() => root.render(createElement(Harness)));
    act(() => card().search.setQuery("old"));
    await advance(500);
    workers[0].postMessage.mockImplementation(() => {
      throw new Error("worker dispatch failed");
    });
    await act(async () => resolveRead(files[0]));
    expect(card().statusText).toBe("status.error");
    expect(card().search.isSearching).toBe(false);
    expect(workers[0].terminate).toHaveBeenCalledOnce();
    expect(mocks.navigate).not.toHaveBeenCalled();
  });
  it("rejects a prior file read after visibility restarts the worker", async () => {
    const reads: ((file: ReviewSearchFile) => void)[] = [];
    loadFile = vi.fn(
      () => new Promise<ReviewSearchFile>((resolve) => reads.push(resolve))
    );
    act(() => root.render(createElement(Harness)));
    act(() => card().search.setQuery("old"));
    await advance(500);
    vi.spyOn(document, "visibilityState", "get").mockReturnValue("hidden");
    act(() => document.dispatchEvent(new Event("visibilitychange")));
    vi.spyOn(document, "visibilityState", "get").mockReturnValue("visible");
    act(() => document.dispatchEvent(new Event("visibilitychange")));
    await advance(500);
    expect(reads).toHaveLength(2);
    await act(async () => reads[0](files[0]));
    expect(workers[1].postMessage).not.toHaveBeenCalled();
    await act(async () => reads[1](files[0]));
    expect(workers[1].postMessage).toHaveBeenCalledTimes(2);
  });
  it("loads collapsed review files serially and stops loading after close", async () => {
    const loader = vi.fn(async (path: string) => ({
      path,
      oldContent: "old",
      newContent: "new",
    }));
    loadFile = loader;
    act(() => root.render(createElement(Harness)));
    act(() => card().search.setQuery("old"));
    await advance(500);
    expect(loader).toHaveBeenCalledTimes(1);
    expect(loader).toHaveBeenCalledWith("a");
    await act(async () => workers[0].reply());
    expect(loader).toHaveBeenCalledTimes(2);
    expect(loader).toHaveBeenLastCalledWith("b");
    act(() => card().search.closeSearch());
    await advance(1000);
    expect(workers[0].terminate).toHaveBeenCalled();
    expect(mocks.navigate).not.toHaveBeenCalled();
  });
  it("starts no idle work, debounces, cancels superseded work and rejects stale replies", async () => {
    expect(workers).toHaveLength(0);
    act(() => card().search.setQuery("old"));
    await advance(499);
    expect(workers).toHaveLength(0);
    await advance(1);
    expect(workers).toHaveLength(1);
    act(() => card().search.setQuery("new"));
    expect(workers[0].terminate).toHaveBeenCalled();
    act(() => workers[0].reply());
    expect(mocks.navigate).not.toHaveBeenCalled();
    await advance(500);
    act(() => workers[1].reply());
    expect(mocks.navigate).toHaveBeenCalledOnce();
    expect(card().search.resultCount).toBe(1);
  });
  it("always searches all review files without a scope switch and supports immediate Enter flush", async () => {
    act(() => card().search.setQuery("old"));
    act(() => card().search.nextResult());
    await advance(0);
    expect(workers[0].postMessage.mock.calls.at(-1)![0].path).toBeUndefined();
    expect(workers[0].postMessage.mock.calls[0][0].files).toEqual(files);
    expect(card().scopeControls).toBe(false);
    expect(card().targetName).toBe("actions.review");
  });
  it("releases work on close and pauses while hidden", async () => {
    act(() => card().search.setQuery("old"));
    await advance(500);
    vi.spyOn(document, "visibilityState", "get").mockReturnValue("hidden");
    act(() => document.dispatchEvent(new Event("visibilitychange")));
    expect(workers[0].terminate).toHaveBeenCalled();
    await advance(1000);
    expect(workers).toHaveLength(1);
    vi.spyOn(document, "visibilityState", "get").mockReturnValue("visible");
    act(() => document.dispatchEvent(new Event("visibilitychange")));
    await advance(500);
    expect(workers).toHaveLength(2);
    act(() => card().search.closeSearch());
    expect(workers[1].terminate).toHaveBeenCalled();
    await advance(1000);
    expect(workers).toHaveLength(2);
  });
});
