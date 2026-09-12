/**
 * SourceControlContextMenu Component
 *
 * Native OS context menu for source control files using Tauri v2 Menu API.
 * Provides git operations: stage, discard, open changes, copy path, reveal in finder.
 * For conflict files: accept ours, accept theirs.
 *
 * Uses dispatch() for actions per GUI Action System guidelines.
 */
import i18next from "i18next";
import { useEffect, useRef } from "react";

import { getShortcutAccelerator } from "@src/config/keyboard/shortcutDisplay";
import { createLogger } from "@src/hooks/logger";
import type { GitFile } from "@src/types/git/types";
import { copyText } from "@src/util/data/clipboard";
import { getFileManagerRevealLabelKey } from "@src/util/platform/fileManagerLabels";
import {
  type NativeMenuItemOptions,
  popupNativeMenu,
} from "@src/util/platform/tauri/nativeMenuPopup";

const log = createLogger("SourceControlContextMenu");

// ============================================
// Types
// ============================================

type DispatchFn = (
  actionType: string,
  payload: Record<string, unknown>,
  source: "user" | "ai" | "system"
) => Promise<unknown>;

type ConflictStrategy = "ours" | "theirs";

export function getSourceControlContextMenuActionLabels(options: {
  isDirectory: boolean;
  isStaged: boolean;
  changeCount: number;
}) {
  const { isDirectory, isStaged, changeCount } = options;
  const t = i18next.t.bind(i18next);
  const count = changeCount;

  return {
    viewChanges: t("common:sourceControl.fileMenu.viewChanges"),
    openFileInNewTab: t("common:sourceControl.fileMenu.openFileInNewTab"),
    acceptCurrent: t("common:sourceControl.fileMenu.acceptCurrent"),
    acceptIncoming: t("common:sourceControl.fileMenu.acceptIncoming"),
    stageToggle: isDirectory
      ? isStaged
        ? t("common:sourceControl.fileMenu.unstageChangesCount", { count })
        : t("common:sourceControl.fileMenu.stageChangesCount", { count })
      : isStaged
        ? t("common:sourceControl.fileMenu.unstageChanges")
        : t("common:sourceControl.fileMenu.stageChanges"),
    markResolved: isDirectory
      ? t("common:sourceControl.fileMenu.markResolvedCount", { count })
      : t("common:sourceControl.fileMenu.markResolved"),
    discard: isDirectory
      ? t("common:sourceControl.fileMenu.discardChangesCount", { count })
      : t("common:sourceControl.fileMenu.discardChanges"),
  };
}

export async function resolveConflictsForFiles(
  dispatch: DispatchFn,
  files: GitFile[],
  strategy: ConflictStrategy
) {
  await Promise.all(
    files.map((file) =>
      dispatch("git.resolveConflict", { path: file.path, strategy }, "user")
    )
  );
}

export interface SourceControlContextMenuProps {
  file: GitFile;
  files?: GitFile[];
  targetPath?: string;
  repoPath: string;
  isConflictFile: boolean;
  isDirectory?: boolean;
  dispatch: DispatchFn;
  onSelect?: (fileId: string) => void;
  onStageToggle?: (fileId: string, stage: boolean) => Promise<void>;
  onDiscard?: (fileId: string) => Promise<void>;
  onDiscardFiles?: (fileIds: string[]) => Promise<void>;
  onStageResolved?: (fileId: string) => Promise<void>;
  onClose: () => void;
}

// Module-level ref for menu callbacks (Tauri menu actions run outside React)
const contextMenuRef: { current: SourceControlContextMenuProps | null } = {
  current: null,
};

// ============================================
// Component
// ============================================

export default function SourceControlContextMenu(
  props: SourceControlContextMenuProps
) {
  const { onClose } = props;
  const hasShownMenu = useRef(false);

  useEffect(() => {
    contextMenuRef.current = props;
    return () => {
      contextMenuRef.current = null;
    };
  }, [props]);

  useEffect(() => {
    if (hasShownMenu.current) return;
    hasShownMenu.current = true;

    async function showNativeMenu() {
      try {
        await popupNativeMenu({
          source: "source-control",
          buildItems: () => {
            const ctx = contextMenuRef.current;
            if (!ctx) return [];

            const { file, isConflictFile, isDirectory } = ctx;
            const t = i18next.t.bind(i18next);
            const files = ctx.files ?? [file];
            const labels = getSourceControlContextMenuActionLabels({
              isDirectory: !!isDirectory,
              isStaged: file.staged,
              changeCount: files.length,
            });

            const items: NativeMenuItemOptions[] = [];

            if (!isDirectory) {
              // --- Open Changes (diff view) ---
              items.push({
                text: labels.viewChanges,
                action: () => {
                  const ref = contextMenuRef.current;
                  if (ref?.onSelect) {
                    ref.onSelect(ref.file.id);
                  }
                },
              });

              // --- Open File ---
              items.push({
                text: labels.openFileInNewTab,
                action: () => {
                  const ref = contextMenuRef.current;
                  if (ref) {
                    const absPath = ref.repoPath
                      ? `${ref.repoPath}/${ref.file.path}`
                      : ref.file.path;
                    ref.dispatch(
                      "file.openAtLine",
                      { path: absPath, line: 1 },
                      "user"
                    );
                  }
                },
              });

              // --- Separator ---
              items.push({ item: "Separator" });
            }

            // --- Stage / Unstage ---
            if (!isConflictFile) {
              items.push({
                text: labels.stageToggle,
                action: async () => {
                  const ref = contextMenuRef.current;
                  if (ref?.onStageToggle) {
                    const files = ref.files ?? [ref.file];
                    await Promise.all(
                      files.map((file) =>
                        ref.onStageToggle?.(file.id, !ref.file.staged)
                      )
                    );
                  }
                },
              });
            }

            // --- Stage Resolved (conflict files) ---
            if (isConflictFile) {
              items.push({
                text: labels.markResolved,
                action: async () => {
                  const ref = contextMenuRef.current;
                  if (ref?.onStageResolved) {
                    const files = ref.files ?? [ref.file];
                    await Promise.all(
                      files.map((file) => ref.onStageResolved?.(file.id))
                    );
                  }
                },
              });
            }

            // --- Discard Changes ---
            items.push({
              text: labels.discard,
              action: async () => {
                const ref = contextMenuRef.current;
                if (ref?.onDiscardFiles && ref.files) {
                  await ref.onDiscardFiles(ref.files.map((file) => file.id));
                } else if (ref?.onDiscard) {
                  await ref.onDiscard(ref.file.id);
                }
              },
            });

            // --- Conflict resolution options ---
            if (isConflictFile) {
              items.push({ item: "Separator" });

              items.push({
                text: labels.acceptCurrent,
                action: async () => {
                  const ref = contextMenuRef.current;
                  if (ref) {
                    const files = ref.files ?? [ref.file];
                    await resolveConflictsForFiles(ref.dispatch, files, "ours");
                  }
                },
              });

              items.push({
                text: labels.acceptIncoming,
                action: async () => {
                  const ref = contextMenuRef.current;
                  if (ref) {
                    const files = ref.files ?? [ref.file];
                    await resolveConflictsForFiles(
                      ref.dispatch,
                      files,
                      "theirs"
                    );
                  }
                },
              });
            }

            // --- Separator ---
            items.push({ item: "Separator" });

            // --- Copy Path ---
            items.push({
              text: t("common:actions.copyPath"),
              accelerator: getShortcutAccelerator("file_menu_copy_path"),
              action: async () => {
                const ref = contextMenuRef.current;
                if (ref) {
                  const targetPath = ref.targetPath ?? ref.file.path;
                  const absPath = ref.repoPath
                    ? `${ref.repoPath}/${targetPath}`
                    : targetPath;
                  await copyText(absPath);
                }
              },
            });

            // --- Copy Relative Path ---
            items.push({
              text: t("common:actions.copyRelativePath"),
              accelerator: getShortcutAccelerator(
                "file_menu_copy_relative_path"
              ),
              action: async () => {
                const ref = contextMenuRef.current;
                if (ref) {
                  await copyText(ref.targetPath ?? ref.file.path);
                }
              },
            });

            // --- Separator ---
            items.push({ item: "Separator" });

            // --- Reveal in OS file manager ---
            items.push({
              text: t(getFileManagerRevealLabelKey()),
              action: () => {
                const ref = contextMenuRef.current;
                if (ref) {
                  const targetPath = ref.targetPath ?? ref.file.path;
                  const absPath = ref.repoPath
                    ? `${ref.repoPath}/${targetPath}`
                    : targetPath;
                  ref.dispatch(
                    "file.revealInFinder",
                    { path: absPath },
                    "user"
                  );
                }
              },
            });

            return items;
          },
        });
      } catch (error) {
        log.error("[SourceControlContextMenu] Failed to show menu:", error);
      } finally {
        // Always close so the parent resets showContextMenu → allows re-open
        onClose();
      }
    }

    void showNativeMenu();
  }, [onClose]);

  // Native menu renders nothing in React
  return null;
}
