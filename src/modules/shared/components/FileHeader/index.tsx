/**
 * FileHeader Component
 *
 * VS Code-like breadcrumb file header with dropdown navigation.
 * Click on any path segment to see files/folders in that directory.
 * Uses an icon button for diff layout and TabPill for custom / preview toggles.
 *
 * Shared across WorkStation CodeEditor, DatabaseManager, and Simulator.
 * When `repoPath` is omitted, breadcrumbs render as static path display
 * (no dropdown navigation).
 *
 * Composition:
 *   - `BreadcrumbFileHeader`  → breadcrumb + dropdown navigation.
 *   - `FileHeaderMoreMenu`    → the trailing ellipsis dropdown menu.
 *   - `FileHeaderShell`       → inline vs teleport-to-workstation wrapper.
 */
import React, {
  memo,
  useCallback,
  useContext,
  useEffect,
  useRef,
  useState,
} from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";

import Button from "@src/components/Button";
import DiffStatsBadge from "@src/components/DiffStatsBadge";
import FileTypeIcon from "@src/components/FileTypeIcon";
import Message from "@src/components/Message";
import TabPill from "@src/components/TabPill";
import { HEADER_ICON_SIZE } from "@src/config/workstation/tokens";
import type { WorkstationTabHeaderHost } from "@src/hooks/tabHost/useWorkstationTabHeader";
import { useRefreshSpin } from "@src/hooks/ui/useRefreshSpin";
import { Cancel01Icon, FileSymlinkIcon, HugeiconsIcon } from "@src/icons";
import { PANEL_HEADER_TOKENS } from "@src/modules/shared/layouts/blocks/PanelHeader/tokens";
import type { DiffViewMode } from "@src/types/git/types";
import { copyText } from "@src/util/data/clipboard";

import { DiffViewModeToggle } from "../DiffViewModeToggle";
import BreadcrumbFileHeader from "./BreadcrumbFileHeader";
import { FileHeaderMoreMenu } from "./FileHeaderMoreMenu";
import { FileHeaderShell } from "./FileHeaderShell";
import { FileHeaderToolbarContext } from "./FileHeaderToolbarContext";

const RELOAD_MENU_COOLDOWN_MS = 1200;

export type { DiffViewMode } from "@src/types/git/types";

export interface ToggleOption {
  value: string;
  icon?: React.ReactNode;
  label?: string;
  title?: string;
  disabled?: boolean;
}

export interface FileHeaderProps {
  /** File path to display */
  filePath: string;
  /**
   * Optional content rendered at the very start of the header, before the
   * breadcrumb / title. Used by the unified Source Control tab to host
   * its Focus / All Changes pill flush against the left edge of the
   * teleported workstation tab-header strip.
   */
  leadingSlot?: React.ReactNode;
  /** Optional custom icon to render instead of FileTypeIcon */
  headerIcon?: React.ReactNode;
  /**
   * Custom title JSX replacing the breadcrumb.
   * When provided, `filePath` is only used for FileTypeIcon fallback.
   */
  titleSlot?: React.ReactNode;
  /** Whether to render FileTypeIcon from filePath (default: true) */
  useFileTypeIcon?: boolean;
  /** Root repository path for navigation */
  repoPath?: string;
  /** Optional additions count (for diffs) */
  additions?: number;
  /** Optional deletions count (for diffs) */
  deletions?: number;
  renderFileActions?: (close: () => void) => React.ReactNode;
  /** Extra actions to render on the right */
  extraActions?: React.ReactNode;
  /** Optional control rendered immediately before the trailing more menu. */
  beforeMoreMenuSlot?: React.ReactNode;
  /** For git diffs: current view mode */
  viewMode?: DiffViewMode;
  /** For git diffs: callback when view mode changes */
  onViewModeChange?: (mode: DiffViewMode) => void;
  /** Custom toggle options (overrides viewMode/onViewModeChange) */
  toggleOptions?: ToggleOption[];
  /** Current toggle value (used with toggleOptions) */
  toggleValue?: string;
  /** Callback when toggle changes (used with toggleOptions) */
  onToggleChange?: (value: string) => void;
  /** Callback when a file is selected from breadcrumb dropdown */
  onFileSelect?: (filePath: string) => void;
  /** Whether to show an action that opens this diff as the regular file. */
  showOpenFileAction?: boolean;
  /** Optional adjacent close action for dismissible file previews. */
  onClose?: () => void;
  /** Callback when reload is requested */
  onReload?: () => void;
  /** Callback when editor search is requested from the more menu. */
  onSearchRequest?: () => void;
  /** Callback when go-to-line is requested from the more menu. */
  onGoToLineRequest?: () => void;
  /** Callback when save is requested from the more menu. */
  onSave?: () => void | Promise<void>;
  /** Callback when discard is requested from the more menu. */
  onDiscard?: () => void;
  /** Relative file path copied from the more menu. */
  relativePathToCopy?: string;
  onRevealInFileManager?: () => void;
  /** Current editor line number visibility state. */
  lineNumbersEnabled?: boolean;
  /** Callback when editor line number visibility changes. */
  onLineNumbersChange?: (enabled: boolean) => void;
  /** Current editor word wrap state. */
  wordWrapEnabled?: boolean;
  /** Callback when editor word wrap changes. */
  onWordWrapChange?: (enabled: boolean) => void;
  /** Current editor minimap state. */
  minimapEnabled?: boolean;
  /** Callback when editor minimap changes. */
  onMinimapChange?: (enabled: boolean) => void;
  /** Current active-line highlight state. */
  highlightActiveLineEnabled?: boolean;
  /** Callback when active-line highlight changes. */
  onHighlightActiveLineChange?: (enabled: boolean) => void;
  /** Whether to render Git Blame toggle in the more menu. */
  showGitBlameToggle?: boolean;
  /** Current Git Blame visibility state. */
  gitBlameEnabled?: boolean;
  /** Callback when Git Blame visibility changes. */
  onGitBlameChange?: (enabled: boolean) => void;
  /** Callback when the final More settings menu action is requested. */
  onMoreSettings?: () => void;
  /** Whether the file is currently loading */
  loading?: boolean;
  /** Whether the file has unsaved changes */
  hasUnsavedChanges?: boolean;
  /** Whether the file is a markdown file */
  isMarkdownFile?: boolean;
  /** Whether markdown preview mode is active */
  isPreviewMode?: boolean;
  /** Label for the raw side of the preview toggle */
  previewSourceLabel?: string;
  /** Label for the rendered/structured side of the preview toggle */
  previewLabel?: string;
  /** Callback when preview mode is toggled */
  onTogglePreview?: () => void;
  /** When true, breadcrumbs are display-only (no click, no dropdown) */
  disableNavigation?: boolean;
  /**
   * When true, show filePath as one truncated line (no `/` segmentation).
   * Use for non-path titles (e.g. concise shell command label).
   */
  plainTitle?: boolean;
  /** Additional CSS classes for the root container */
  className?: string;
  /**
   * When set, the header is teleported into the global Workstation tab-header
   * strip for that host instead of rendering inline. Used by both My Station
   * panes (`code` / `data` / `browser` / `project`) and Agent Station's
   * simulator replay views (`simulator`) so the breadcrumb / toolbar always
   * lives in the 36px shell header rather than as a duplicate strip below
   * the tab bar.
   */
  publishToHost?: WorkstationTabHeaderHost;
  /**
   * Whether this header is the one that should claim the global slot.
   * Single-pane layouts pass `true`; reserved for cases where multiple
   * `FileHeader` instances render concurrently (e.g. a preview) and only
   * one should publish to the global 36px strip.
   */
  publishEnabled?: boolean;
}

export const FileHeader: React.FC<FileHeaderProps> = memo(
  ({
    filePath,
    leadingSlot,
    headerIcon,
    titleSlot,
    useFileTypeIcon = true,
    repoPath,
    additions,
    deletions,
    extraActions,
    renderFileActions,
    beforeMoreMenuSlot,
    viewMode,
    onViewModeChange,
    toggleOptions,
    toggleValue,
    onToggleChange,
    onFileSelect,
    showOpenFileAction = false,
    onClose,
    onReload,
    onSearchRequest,
    onGoToLineRequest,
    onSave,
    onDiscard,
    relativePathToCopy,
    onRevealInFileManager,
    lineNumbersEnabled = true,
    onLineNumbersChange,
    wordWrapEnabled = false,
    onWordWrapChange,
    minimapEnabled = false,
    onMinimapChange,
    highlightActiveLineEnabled = true,
    onHighlightActiveLineChange,
    showGitBlameToggle = false,
    gitBlameEnabled = false,
    onGitBlameChange,
    onMoreSettings,
    loading,
    hasUnsavedChanges = false,
    isMarkdownFile,
    isPreviewMode,
    previewSourceLabel,
    previewLabel,
    onTogglePreview,
    disableNavigation,
    plainTitle,
    className,
    publishToHost,
    publishEnabled = true,
  }) => {
    const { t } = useTranslation();
    const reloadSpinPersistenceKey = `${repoPath ?? "no-repo"}::${filePath}`;
    const { spinClass: reloadSpinClass, handleClick: handleReloadClick } =
      useRefreshSpin(
        onReload ?? (() => {}),
        loading ?? false,
        reloadSpinPersistenceKey
      );
    const toolbarTarget = useContext(FileHeaderToolbarContext);
    const [moreMenuVisible, setMoreMenuVisible] = useState(false);
    const [reloadMenuCoolingDown, setReloadMenuCoolingDown] = useState(false);
    const reloadMenuCooldownTimerRef = useRef<ReturnType<
      typeof setTimeout
    > | null>(null);
    useEffect(() => {
      return () => {
        if (reloadMenuCooldownTimerRef.current) {
          clearTimeout(reloadMenuCooldownTimerRef.current);
        }
      };
    }, []);

    const handleOpenFileClick = useCallback(() => {
      const targetPath = filePath.startsWith("/")
        ? filePath
        : repoPath
          ? `${repoPath}/${filePath}`
          : filePath;
      onFileSelect?.(targetPath);
    }, [filePath, onFileSelect, repoPath]);
    const handleCloseClick = useCallback(() => {
      onClose?.();
    }, [onClose]);
    const handleReloadMenuClick = useCallback(() => {
      if (loading || reloadMenuCoolingDown) return;

      setReloadMenuCoolingDown(true);
      if (reloadMenuCooldownTimerRef.current) {
        clearTimeout(reloadMenuCooldownTimerRef.current);
      }
      reloadMenuCooldownTimerRef.current = setTimeout(() => {
        setReloadMenuCoolingDown(false);
        reloadMenuCooldownTimerRef.current = null;
      }, RELOAD_MENU_COOLDOWN_MS);

      handleReloadClick();
      setMoreMenuVisible(false);
    }, [handleReloadClick, loading, reloadMenuCoolingDown]);
    const handleSearchMenuClick = useCallback(() => {
      onSearchRequest?.();
      setMoreMenuVisible(false);
    }, [onSearchRequest]);
    const handleGoToLineMenuClick = useCallback(() => {
      onGoToLineRequest?.();
      setMoreMenuVisible(false);
    }, [onGoToLineRequest]);
    const handleSaveMenuClick = useCallback(() => {
      if (!hasUnsavedChanges || loading) return;

      void onSave?.();
      setMoreMenuVisible(false);
    }, [hasUnsavedChanges, loading, onSave]);
    const handleDiscardMenuClick = useCallback(() => {
      if (!hasUnsavedChanges || loading) return;

      onDiscard?.();
      setMoreMenuVisible(false);
    }, [hasUnsavedChanges, loading, onDiscard]);
    const handleCopyRelativePathMenuClick = useCallback(async () => {
      if (!relativePathToCopy) return;

      try {
        await copyText(relativePathToCopy);
        Message.success(t("common:status.copied"));
      } catch {
        Message.error(t("common:errors.failedToCopy"));
      }
      setMoreMenuVisible(false);
    }, [relativePathToCopy, t]);
    const handleRevealInFileManagerMenuClick = useCallback(() => {
      onRevealInFileManager?.();
      setMoreMenuVisible(false);
    }, [onRevealInFileManager]);
    const handleLineNumbersChange = useCallback(
      (enabled: boolean) => {
        onLineNumbersChange?.(enabled);
      },
      [onLineNumbersChange]
    );
    const handleWordWrapChange = useCallback(
      (enabled: boolean) => {
        onWordWrapChange?.(enabled);
      },
      [onWordWrapChange]
    );
    const handleMinimapChange = useCallback(
      (enabled: boolean) => {
        onMinimapChange?.(enabled);
      },
      [onMinimapChange]
    );
    const handleHighlightActiveLineChange = useCallback(
      (enabled: boolean) => {
        onHighlightActiveLineChange?.(enabled);
      },
      [onHighlightActiveLineChange]
    );
    const handleGitBlameChange = useCallback(
      (enabled: boolean) => {
        onGitBlameChange?.(enabled);
      },
      [onGitBlameChange]
    );
    const handleMoreSettingsMenuClick = useCallback(() => {
      onMoreSettings?.();
      setMoreMenuVisible(false);
    }, [onMoreSettings]);

    const hasStats = additions !== undefined || deletions !== undefined;
    const showViewModeToggle = viewMode && onViewModeChange;
    const showCustomToggle = toggleOptions && toggleValue && onToggleChange;
    const showReloadButton = !!onReload;
    const showSearchAction = !!onSearchRequest;
    const showGoToLineAction = !!onGoToLineRequest;
    const showSaveAction = !!onSave;
    const showDiscardAction = !!onDiscard;
    const showCopyRelativePathAction = !!relativePathToCopy;
    const showRevealInFileManagerAction = !!onRevealInFileManager;
    const showLineNumbersToggle = !!onLineNumbersChange;
    const showWordWrapToggle = !!onWordWrapChange;
    const showMinimapToggle = !!onMinimapChange;
    const showHighlightActiveLineToggle = !!onHighlightActiveLineChange;
    const showMoreSettingsAction = !!onMoreSettings;
    const showMoreMenu =
      !!renderFileActions ||
      showReloadButton ||
      showSearchAction ||
      showGoToLineAction ||
      showSaveAction ||
      showDiscardAction ||
      showCopyRelativePathAction ||
      showRevealInFileManagerAction ||
      showLineNumbersToggle ||
      showWordWrapToggle ||
      showMinimapToggle ||
      showHighlightActiveLineToggle ||
      showGitBlameToggle ||
      showMoreSettingsAction;
    const showPreviewButton = isMarkdownFile && onTogglePreview && !hasStats;
    const showAnyTabSwitch =
      showViewModeToggle || showCustomToggle || showPreviewButton;
    const showCloseAction = !!onClose;
    const showHeaderActionButtons =
      showMoreMenu || showOpenFileAction || showCloseAction;
    const breadcrumbLastSegmentIcon = headerIcon ? (
      headerIcon
    ) : useFileTypeIcon && filePath ? (
      <FileTypeIcon
        fileName={filePath}
        size="small"
        className="shrink-0 text-text-2"
      />
    ) : null;
    const hasRightControls =
      (hasStats && (additions! > 0 || deletions! > 0)) ||
      showViewModeToggle ||
      showCustomToggle ||
      showPreviewButton ||
      !!beforeMoreMenuSlot ||
      showMoreMenu ||
      showOpenFileAction ||
      showCloseAction ||
      !!extraActions;

    const viewModeToggle = showViewModeToggle ? (
      <DiffViewModeToggle
        viewMode={viewMode}
        onChange={onViewModeChange}
        t={t}
      />
    ) : null;
    const moreMenu = showMoreMenu ? (
      <FileHeaderMoreMenu
        renderFileActions={renderFileActions}
        showReloadButton={showReloadButton}
        showSearchAction={showSearchAction}
        showGoToLineAction={showGoToLineAction}
        showSaveAction={showSaveAction}
        showDiscardAction={showDiscardAction}
        showCopyRelativePathAction={showCopyRelativePathAction}
        showRevealInFileManagerAction={showRevealInFileManagerAction}
        showLineNumbersToggle={showLineNumbersToggle}
        showWordWrapToggle={showWordWrapToggle}
        showMinimapToggle={showMinimapToggle}
        showHighlightActiveLineToggle={showHighlightActiveLineToggle}
        showGitBlameToggle={showGitBlameToggle}
        showMoreSettingsAction={showMoreSettingsAction}
        lineNumbersEnabled={lineNumbersEnabled}
        wordWrapEnabled={wordWrapEnabled}
        minimapEnabled={minimapEnabled}
        highlightActiveLineEnabled={highlightActiveLineEnabled}
        gitBlameEnabled={gitBlameEnabled}
        loading={!!loading}
        hasUnsavedChanges={hasUnsavedChanges}
        reloadSpinClass={reloadSpinClass}
        reloadMenuCoolingDown={reloadMenuCoolingDown}
        menuVisible={moreMenuVisible}
        setMenuVisible={setMoreMenuVisible}
        onSaveClick={handleSaveMenuClick}
        onDiscardClick={handleDiscardMenuClick}
        onSearchClick={handleSearchMenuClick}
        onGoToLineClick={handleGoToLineMenuClick}
        onCopyRelativePathClick={handleCopyRelativePathMenuClick}
        onRevealInFileManagerClick={handleRevealInFileManagerMenuClick}
        onReloadClick={handleReloadMenuClick}
        onLineNumbersChange={handleLineNumbersChange}
        onWordWrapChange={handleWordWrapChange}
        onMinimapChange={handleMinimapChange}
        onHighlightActiveLineChange={handleHighlightActiveLineChange}
        onGitBlameChange={handleGitBlameChange}
        onMoreSettingsClick={handleMoreSettingsMenuClick}
      />
    ) : null;

    const headerInner = (
      <>
        {toolbarTarget && createPortal(moreMenu, toolbarTarget)}
        {/* Optional leading content (rendered before the breadcrumb) */}
        {leadingSlot && (
          <div className="flex shrink-0 items-center">{leadingSlot}</div>
        )}

        {/* Breadcrumb Navigation / Custom Title */}
        {titleSlot ? (
          <div className="flex min-w-0 flex-1 items-center gap-1.5 overflow-x-hidden">
            {breadcrumbLastSegmentIcon && (
              <span className="inline-flex shrink-0 items-center text-text-2">
                {breadcrumbLastSegmentIcon}
              </span>
            )}
            {titleSlot}
          </div>
        ) : (
          <BreadcrumbFileHeader
            filePath={filePath}
            repoPath={repoPath}
            lastSegmentIcon={breadcrumbLastSegmentIcon}
            onFileSelect={onFileSelect}
            disableNavigation={disableNavigation}
            plainTitle={plainTitle}
          />
        )}

        {/* Right-side controls */}
        {hasRightControls && (
          <div className="ml-auto flex shrink-0 items-center gap-px">
            {/* Stats (for diffs) */}
            {hasStats && (
              <DiffStatsBadge additions={additions} deletions={deletions} />
            )}

            {/* Separator before tab pills */}
            {((showViewModeToggle && !toolbarTarget) ||
              showCustomToggle ||
              showPreviewButton) &&
              hasStats &&
              (additions! > 0 || deletions! > 0) && (
                <div
                  className={`${PANEL_HEADER_TOKENS.verticalSeparator} mx-1.5`}
                  role="separator"
                  aria-hidden
                />
              )}
            {/* View Mode Toggle (for diffs) */}
            {showViewModeToggle && !toolbarTarget && viewModeToggle}

            {/* Custom Toggle — TabPill pill (matches source control / preview) */}
            {showCustomToggle && (
              <div className="flex h-7 shrink-0 items-center">
                <TabPill
                  activeTab={toggleValue}
                  tabs={toggleOptions.map((option) => ({
                    key: option.value,
                    label: option.label ?? option.value,
                    icon: option.icon,
                    disabled: option.disabled,
                  }))}
                  onChange={onToggleChange}
                  variant="pill"
                  color="fill"
                  fillWidth={false}
                  size="small"
                  iconOnly={toggleOptions.every((opt) => !opt.label)}
                  className={
                    toggleOptions.every((opt) => !opt.label)
                      ? "[&_button]:h-7 [&_button]:w-7 [&_button]:min-w-7 [&_button]:p-0"
                      : undefined
                  }
                />
              </div>
            )}

            {/* Markdown Preview Toggle — TabPill pill (source control / preview style) */}
            {showPreviewButton && (
              <div className="flex h-7 shrink-0 items-center">
                <TabPill
                  activeTab={isPreviewMode ? "preview" : "source"}
                  tabs={[
                    {
                      key: "source",
                      label: previewSourceLabel ?? t("common:common.raw"),
                    },
                    {
                      key: "preview",
                      label: previewLabel ?? t("common:common.preview"),
                    },
                  ]}
                  onChange={(key) => {
                    if (key === "preview" && !isPreviewMode)
                      onTogglePreview?.();
                    if (key === "source" && isPreviewMode) onTogglePreview?.();
                  }}
                  variant="pill"
                  color="fill"
                  fillWidth={false}
                  size="small"
                />
              </div>
            )}

            {/* Vertical separator: tab switches | other buttons */}
            {showAnyTabSwitch &&
              !toolbarTarget &&
              (showHeaderActionButtons || extraActions) && (
                <div
                  className={`${PANEL_HEADER_TOKENS.verticalSeparator} mx-1.5`}
                  role="separator"
                  aria-hidden
                />
              )}

            {(showHeaderActionButtons || beforeMoreMenuSlot) && (
              <span className="flex items-center gap-px">
                {beforeMoreMenuSlot}

                {/* More actions */}
                {showMoreMenu && !toolbarTarget && moreMenu}

                {showOpenFileAction && onFileSelect && (
                  <Button
                    htmlType="button"
                    variant="tertiary"
                    size="small"
                    iconOnly
                    onClick={handleOpenFileClick}
                    title={t("tooltips.openFile")}
                    className="shrink-0"
                    icon={
                      <HugeiconsIcon
                        icon={FileSymlinkIcon}
                        data-icon="file-symlink"
                        size={HEADER_ICON_SIZE.sm}
                        strokeWidth={1.75}
                      />
                    }
                  />
                )}

                {showCloseAction && (
                  <Button
                    htmlType="button"
                    variant="tertiary"
                    size="small"
                    iconOnly
                    onClick={handleCloseClick}
                    title={t("common:actions.close")}
                    aria-label={t("common:actions.close")}
                    className="shrink-0"
                    icon={
                      <HugeiconsIcon
                        icon={Cancel01Icon}
                        data-icon="x"
                        size={HEADER_ICON_SIZE.sm}
                        strokeWidth={1.75}
                      />
                    }
                  />
                )}
              </span>
            )}
            {/* Extra actions */}
            {extraActions}
          </div>
        )}
      </>
    );

    return (
      <FileHeaderShell
        className={className}
        publishToHost={publishToHost}
        publishEnabled={publishEnabled}
      >
        {headerInner}
      </FileHeaderShell>
    );
  }
);

FileHeader.displayName = "FileHeader";

export default FileHeader;
