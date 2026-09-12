import { describe, expect, it, vi } from "vitest";

import { tryOpenLinkedSessionFromSidebar } from "./useProjectsMenuItemClick";

describe("tryOpenLinkedSessionFromSidebar", () => {
  it("selects and opens a linked-session child row", () => {
    const setProjectsSelectedMenuItemId = vi.fn();
    const openLinkedSession = vi.fn();
    const item = {
      id: "session-1",
      key: "work-item-linked-session:work-item-1:session-1",
      label: "SDE #1",
    };

    expect(
      tryOpenLinkedSessionFromSidebar({
        item,
        linkedSessionIds: new Set(["session-1"]),
        setProjectsSelectedMenuItemId,
        openLinkedSession,
      })
    ).toBe(true);
    expect(setProjectsSelectedMenuItemId).toHaveBeenCalledWith(item.key);
    expect(openLinkedSession).toHaveBeenCalledWith(item);
  });
});
