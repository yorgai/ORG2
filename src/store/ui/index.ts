/**
 * UI Atoms - Barrel Export
 *
 * Pure UI state (theme, modals, navigation, etc.)
 *
 * Note: Session creator atoms live in @src/store/session
 *
 * Logical grouping:
 * - Sidebar: sidebarAtom, hoverSidebarAtom, collapseStateAtom
 * - Editor: editorSettingsAtom, fileTreeSelectionAtom, searchResultSelectionAtom
 */

// Sidebar
export * from "./sidebarAtom";
export * from "./hoverSidebarAtom";
export * from "./collapseStateAtom";
export * from "./localChannelsAtom";
export * from "./localChannelMessagesAtom";

// Editor
export * from "./editorSettingsAtom";
export * from "./fileTreeSelectionAtom";
export * from "./searchResultSelectionAtom";

// Settings
export * from "./languageAtom";

// Other UI state
export * from "./uiAtom";
export * from "./backgroundConfigAtom";
export * from "./overlayLayerAtom";
export * from "./timezoneAtom";
export * from "./notificationAtom";
export * from "./inboxAtom";
export * from "./routeToolbarAtom";
export * from "./dragDropAtom";
export * from "./todoAtom";
export * from "./addToAgentAtom";
export * from "./integrationsToolbarAtom";
export * from "./kanbanViewStateAtom";
export * from "./workManagementCreatorAtom";
export * from "./sideChatAtom";
export * from "./modelSelectorAtom";
export * from "./guideHighlightAtom";

// WorkStation / Chat / Simulator / Workspace Folders (formerly workspaceAtom barrel)
export * from "./simulatorAtom";
export * from "./overlayAtom";
export { CHAT_PANEL_SURFACE_KIND } from "@src/types/ui/chatPanel";

export * from "./chatPanel/widthAtoms";
export * from "./chatPanel/visibilityAtoms";
export * from "./chatPanel/displayPrefsAtoms";
export * from "./chatPanel/selectionAtoms";
export * from "./chatPanel/surfaceAtoms";
export * from "./chatPanel/miscAtoms";
export * from "./chatImageAtom";
export * from "./messageQueueAtom";
export * from "./sessionPaginationAtom";
export * from "./draftAtom";
export * from "./workStationLayout";
export * from "./workspace";
