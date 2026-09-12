// @vitest-environment jsdom
import { EditorView, lineNumbers } from "@codemirror/view";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { dirtyDiffGutter } from "./dirtyDiff";

const mocks = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
const mounted: EditorView[] = [];
function editor() {
  const view = new EditorView({
    parent: document.body,
    doc: "new\nchanged\nsame",
    extensions: [lineNumbers(), dirtyDiffGutter({ current: "old\nsame" })],
  });
  mounted.push(view);
  return view;
}
const result = (line: number, type = "added") => ({
  markers: [{ line, type }],
});
beforeEach(() => {
  vi.useFakeTimers();
  mocks.invoke.mockReset();
});
afterEach(() => {
  mounted.splice(0).forEach((view) => view.destroy());
  document.body.replaceChildren();
  vi.useRealTimers();
});

describe("dirty diff rendering", () => {
  it("marks the number gutter and code row from the backend result", async () => {
    mocks.invoke.mockResolvedValue({
      markers: [
        { line: 1, type: "added" },
        { line: 2, type: "modified" },
        { line: 3, type: "deleted" },
      ],
    });
    const view = editor();
    await vi.advanceTimersByTimeAsync(50);
    expect(
      view.dom.querySelector(".cm-lineNumbers .cm-dirty-diff-row-added")
        ?.textContent
    ).toBe("1");
    expect(
      view.dom.querySelector(".cm-lineNumbers .cm-dirty-diff-row-modified")
        ?.textContent
    ).toBe("2");
    expect(
      view.dom.querySelector(".cm-line.cm-dirty-diff-row-added")?.textContent
    ).toBe("new");
    // A deletion anchor points to surviving text, not a deleted code row.
    expect(
      view.dom.querySelector(".cm-line.cm-dirty-diff-row-deleted")
    ).toBeNull();
    expect(view.dom.querySelector(".cm-dirty-diff-deleted")).not.toBeNull();
  });

  it("rejects a result for an obsolete document before applying fresh markers", async () => {
    let resolve!: (value: ReturnType<typeof result>) => void;
    mocks.invoke
      .mockReturnValueOnce(
        new Promise((done) => {
          resolve = done;
        })
      )
      .mockResolvedValueOnce(result(2, "modified"));
    const view = editor();
    await vi.advanceTimersByTimeAsync(50);
    view.dispatch({ changes: { from: 0, to: 3, insert: "latest" } });
    resolve(result(1));
    await vi.advanceTimersByTimeAsync(0);
    expect(view.dom.querySelector(".cm-dirty-diff-row-added")).toBeNull();
    await vi.advanceTimersByTimeAsync(150);
    expect(
      view.dom.querySelector(".cm-lineNumbers .cm-dirty-diff-row-modified")
        ?.textContent
    ).toBe("2");
    expect(mocks.invoke).toHaveBeenCalledTimes(2);
  });

  it("preserves existing markers on backend failure and stops work on close", async () => {
    mocks.invoke
      .mockResolvedValueOnce(result(1))
      .mockRejectedValueOnce(new Error("offline"));
    const view = editor();
    await vi.advanceTimersByTimeAsync(50);
    view.dispatch({ changes: { from: view.state.doc.length, insert: "!" } });
    await vi.advanceTimersByTimeAsync(150);
    expect(view.dom.querySelector(".cm-dirty-diff-row-added")).not.toBeNull();
    view.dispatch({ changes: { from: view.state.doc.length, insert: "!" } });
    view.destroy();
    mounted.pop();
    await vi.advanceTimersByTimeAsync(1000);
    expect(mocks.invoke).toHaveBeenCalledTimes(2);
  });
});
