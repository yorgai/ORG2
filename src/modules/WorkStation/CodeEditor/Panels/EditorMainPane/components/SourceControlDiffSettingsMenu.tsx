import { useAtom, useAtomValue } from "jotai";
import React, { useState } from "react";

import { FileHeaderMoreMenu } from "@src/modules/shared/components/FileHeader/FileHeaderMoreMenu";
import {
  activeStatusBarCallbacksAtom,
  editorHighlightActiveLineAtom,
  editorLineNumbersAtom,
  editorWordWrapAtom,
} from "@src/store/ui";

const noop = () => {};

/** Aggregate diffs expose shared editor preferences without single-file actions. */
export function SourceControlDiffSettingsMenu() {
  const [menuVisible, setMenuVisible] = useState(false);
  const [lineNumbers, setLineNumbers] = useAtom(editorLineNumbersAtom);
  const [wordWrap, setWordWrap] = useAtom(editorWordWrapAtom);
  const [highlightActiveLine, setHighlightActiveLine] = useAtom(
    editorHighlightActiveLineAtom
  );
  const { onOpenSettings } = useAtomValue(activeStatusBarCallbacksAtom);
  return (
    <FileHeaderMoreMenu
      showReloadButton={false}
      showSearchAction={false}
      showGoToLineAction={false}
      showSaveAction={false}
      showDiscardAction={false}
      showCopyRelativePathAction={false}
      showRevealInFileManagerAction={false}
      showLineNumbersToggle
      showWordWrapToggle
      showMinimapToggle={false}
      showHighlightActiveLineToggle
      showGitBlameToggle={false}
      showMoreSettingsAction={!!onOpenSettings}
      lineNumbersEnabled={lineNumbers !== "off"}
      wordWrapEnabled={wordWrap}
      minimapEnabled={false}
      highlightActiveLineEnabled={highlightActiveLine}
      gitBlameEnabled={false}
      loading={false}
      hasUnsavedChanges={false}
      reloadSpinClass={undefined}
      reloadMenuCoolingDown={false}
      menuVisible={menuVisible}
      setMenuVisible={setMenuVisible}
      onSaveClick={noop}
      onDiscardClick={noop}
      onSearchClick={noop}
      onGoToLineClick={noop}
      onCopyRelativePathClick={noop}
      onRevealInFileManagerClick={noop}
      onReloadClick={noop}
      onLineNumbersChange={(enabled) => setLineNumbers(enabled ? "on" : "off")}
      onWordWrapChange={setWordWrap}
      onMinimapChange={noop}
      onHighlightActiveLineChange={setHighlightActiveLine}
      onGitBlameChange={noop}
      onMoreSettingsClick={() => {
        onOpenSettings?.();
        setMenuVisible(false);
      }}
    />
  );
}
