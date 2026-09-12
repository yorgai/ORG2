import type { TFunction } from "i18next";
import { useCallback } from "react";
import type { Location, NavigateFunction } from "react-router-dom";

import { ROUTES } from "@src/config/routes";
import type { useAppNavigation } from "@src/hooks/navigation/useAppNavigation";
import type { StationMode } from "@src/store/ui/simulatorAtom";

import { useSessionEntryActions } from "./sessionEntryActions";
import type { useWorkstationSidebarChatPanelAtoms } from "./sidebarConnector.chatPanelAtoms";

type ChatActions = ReturnType<typeof useWorkstationSidebarChatPanelAtoms>;
interface SidebarStationNavigationParams {
  setStationMode: ChatActions["setStationMode"];
  setStationChatVisible: ChatActions["setStationChatVisible"];
  openStartPageTab: ChatActions["openStartPageTab"];
  resetChatPanelSessionSurface: ChatActions["resetChatPanelSessionSurface"];
  setChatPanelCreateTarget: ChatActions["setChatPanelCreateTarget"];
  goToNewSession: ReturnType<typeof useAppNavigation>["goToNewSession"];
  location: Location;
  navigate: NavigateFunction;
  t: TFunction<"navigation">;
}
export function useSidebarStationNavigation({
  setStationMode,
  setStationChatVisible,
  openStartPageTab,
  resetChatPanelSessionSurface,
  setChatPanelCreateTarget,
  goToNewSession,
  location,
  navigate,
  t,
}: SidebarStationNavigationParams) {
  const activateMyStationRouteForProjectTabContent = useCallback(() => {
    const stationMode: StationMode = "my-station";
    const targetRoute = ROUTES.workStation.code.path;
    setStationMode(stationMode);
    setStationChatVisible(stationMode, true);
    if (location.pathname !== targetRoute) navigate(targetRoute);
  }, [location.pathname, navigate, setStationChatVisible, setStationMode]);

  const openNewChatTab = useCallback(() => {
    openStartPageTab({ title: t("routes.launchpad") });
  }, [openStartPageTab, t]);

  const { handleGoToNewSession } = useSessionEntryActions({
    goToNewSession,
    resetChatPanelSessionSurface,
    openNewChatTab,
    setChatPanelCreateTarget,
  });

  return {
    activateMyStationRouteForProjectTabContent,
    handleGoToNewSession,
  };
}
