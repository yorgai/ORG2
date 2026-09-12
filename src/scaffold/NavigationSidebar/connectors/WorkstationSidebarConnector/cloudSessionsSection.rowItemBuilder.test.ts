import { Provider } from "jotai";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";

import type { RemoteTeammateSessionMetadata } from "@src/store/collaboration/types";

import { useCloudSessionRowItemBuilder } from "./cloudSessionsSection.rowItemBuilder";

vi.mock("@src/config/agentIcons", () => ({
  resolveAgentIcon: () => (props: { size?: number }) =>
    createElement("i", {
      "data-agent-icon": "stub",
      "data-size": props.size,
    }),
}));

const ORG_ID = "11111111-1111-1111-1111-111111111111";
const OWNER_USER_ID = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa";

const remoteRow: RemoteTeammateSessionMetadata = {
  id: "remote-row-1",
  orgId: ORG_ID,
  ownerMemberId: "member-1",
  ownerUserId: OWNER_USER_ID,
  ownerDisplayName: "Alice",
  ownerIdentityKind: "human",
  sourceSessionId: "source-session-1",
  title: "Cloud-only session",
  eventsEpoch: 1,
  eventsFrozenSeq: 0,
  eventsCount: 1,
  eventsTailHash: "hash",
};

function renderCloudRowAccessory(): string {
  const Probe = () => {
    const buildRowItem = useCloudSessionRowItemBuilder({
      presenceMap: {},
      selfUserId: null,
      t: ((key: string) => key) as never,
      tCommon: ((key: string) => key) as never,
      runFork: vi.fn(),
      buildNativeMenuItems: () => [],
      busySessionRows: new Map(),
      pinnedRemoteSessionIds: new Set(),
      toggleRemoteSessionPin: vi.fn(),
    });
    const item = buildRowItem({
      row: remoteRow,
      bareSessionId: remoteRow.sourceSessionId,
      isOrphan: false,
    });
    return createElement("div", null, item.trailingElement);
  };

  return renderToStaticMarkup(
    createElement(Provider, null, createElement(Probe))
  );
}

describe("team session accessories", () => {
  it("omits the cloud icon and empty accessory wrapper", () => {
    expect(renderCloudRowAccessory()).toBe("<div></div>");
  });
});
