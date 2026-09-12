import { Provider } from "jotai";
import React from "react";
import { renderToString } from "react-dom/server";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { ROUTES } from "@src/config/routes";
import {
  activeSessionIdAtom,
  workstationActiveSessionIdAtom,
} from "@src/store/session";
import { chatPanelMaximizedAtom } from "@src/store/ui/chatPanel/surfaceAtoms";
import { stationModeAtom } from "@src/store/ui/simulatorAtom";
import { enterWorkstationRouteAtom } from "@src/store/workstation/routeEntryAtom";
import { workstationLayoutAtom } from "@src/store/workstation/tabs";
import { createInstrumentedStore } from "@src/util/core/state/instrumentedStore";

import { useAppNavigation } from "./useAppNavigation";

const mocks = vi.hoisted(() => ({ navigate: vi.fn() }));

vi.mock("react-router-dom", async (importOriginal) => ({
  ...(await importOriginal<typeof import("react-router-dom")>()),
  useNavigate: () => mocks.navigate,
}));

describe("new session navigation", () => {
  beforeEach(() => {
    localStorage.clear();
    mocks.navigate.mockReset();
  });

  it.each([
    { mode: "my-station" as const, maximized: true },
    { mode: "my-station" as const, maximized: false },
    { mode: "agent-station" as const, maximized: true },
    { mode: "agent-station" as const, maximized: false },
  ])(
    "preserves $mode layout with maximized=$maximized",
    ({ mode, maximized }) => {
      const store = createInstrumentedStore();
      store.set(stationModeAtom, mode);
      store.set(chatPanelMaximizedAtom, maximized);
      store.set(activeSessionIdAtom, "previous-session");
      store.set(workstationActiveSessionIdAtom, "previous-session");
      const layout = store.get(workstationLayoutAtom);
      // Exercise the actual route-entry writer, which previously reopened My Station.
      mocks.navigate.mockImplementation((path: string) => {
        store.set(enterWorkstationRouteAtom, path.split("?")[0]);
      });

      let navigation: ReturnType<typeof useAppNavigation> | undefined;
      function HookProbe(): null {
        // eslint-disable-next-line react-hooks/globals -- synchronous server-rendered hook probe
        navigation = useAppNavigation();
        return null;
      }
      renderToString(
        React.createElement(Provider, { store }, React.createElement(HookProbe))
      );
      expect(navigation).toBeDefined();

      for (const options of [
        undefined,
        { projectId: "project-1", workflowId: "workflow-1" },
      ]) {
        navigation!.goToNewSession(options);
        expect(mocks.navigate).toHaveBeenLastCalledWith(
          options
            ? `${ROUTES.workStation.base.path}?projectId=project-1&workflowId=workflow-1`
            : ROUTES.workStation.base.path
        );
        expect(store.get(chatPanelMaximizedAtom)).toBe(maximized);
        expect(store.get(stationModeAtom)).toBe(mode);
        expect(store.get(workstationLayoutAtom)).toEqual(layout);
        expect(store.get(activeSessionIdAtom)).toBeNull();
        expect(store.get(workstationActiveSessionIdAtom)).toBeNull();
      }
    }
  );
});
