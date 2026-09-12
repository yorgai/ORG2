import React, {
  memo,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import {
  type StateSnapshot,
  Virtuoso,
  type VirtuosoHandle,
} from "react-virtuoso";

import { Placeholder } from "@src/components/Placeholder";
import type { DiffViewMode } from "@src/types/git/types";

import DiffFileSection from "../DiffFileSection";
import type { DiffFileSectionData } from "../DiffFileSection";
import { getDefaultDiffSectionExpanded } from "./expansion";
import type {
  ReviewSearchFile,
  ReviewSearchMatch,
} from "./search/reviewSearchTypes";
import { useReviewSearch } from "./search/useReviewSearch";
import {
  type DiffSectionListViewState,
  type RememberedExpansion,
  createRestoredExpansions,
  pruneRestoredExpansions,
  seedRestoredExpansion,
  snapshotExpansions,
} from "./viewState";

export type { DiffSectionListViewState } from "./viewState";

export interface DiffSectionListItem<TFile extends DiffFileSectionData> {
  key: string;
  file: TFile;
}

interface DiffSectionListProps<TFile extends DiffFileSectionData> {
  sections: Array<DiffSectionListItem<TFile>>;
  enableReviewSearch?: boolean;
  reviewSearchFiles?: readonly ReviewSearchFile[];
  loadReviewFile?: (path: string) => Promise<ReviewSearchFile | null>;
  viewMode: DiffViewMode;
  loading?: boolean;
  emptyTitle: string;
  emptySubtitle?: string;
  repoPath?: string;
  collapseThreshold?: number;
  /** Start collapsible sections closed regardless of list size. */
  defaultCollapsed?: boolean;
  collapseSignal?: number;
  getSectionRef?: (path: string) => React.RefObject<HTMLDivElement | null>;
  focusedPath?: string | null;
  focusedNonce?: number;
  onFileSelect?: (path: string) => void;
  onRequestContent?: (file: TFile) => void;
  onExpansionChange?: (file: TFile, expanded: boolean) => void;
  sectionKeySuffix?: (section: DiffSectionListItem<TFile>) => string | number;
  showBottomBorder?: boolean;
  hideLastBottomBorder?: boolean;
  /** Show the original path after renamed files in each section header. */
  showRenamePath?: boolean;
  /** When true, each section renders a flat FileHeader instead of the collapsible chevron button. */
  flat?: boolean;
  /** Use the compact header gutter for panes with their own left divider/chrome. */
  compactHeaderGutter?: boolean;
  /** When true, removes the bottom scroll padding (for contexts that have no bottom panel). */
  hideBottomPadding?: boolean;
  /**
   * View state saved by a previous mount of this list (expansion overrides,
   * scroll offset, handled focus nonce). Read once at mount; the list is
   * rebuilt from it instead of being kept alive hidden.
   */
  viewState?: DiffSectionListViewState | null;
  /**
   * Receives a fresh snapshot whenever an override changes and, on unmount,
   * with the final scroll offset. Callers persist it per tab.
   */
  onViewStateChange?: (viewState: DiffSectionListViewState) => void;
}

const DEFAULT_COLLAPSE_THRESHOLD = 10;

function DiffListFooter() {
  return <div className="h-[100px]" aria-hidden />;
}

const DIFF_LIST_COMPONENTS = { Footer: DiffListFooter };

function DiffSectionListInner<TFile extends DiffFileSectionData>({
  sections,
  enableReviewSearch = false,
  reviewSearchFiles,
  loadReviewFile,
  viewMode,
  loading = false,
  emptyTitle,
  emptySubtitle,
  repoPath,
  collapseThreshold = DEFAULT_COLLAPSE_THRESHOLD,
  defaultCollapsed = false,
  collapseSignal = 0,
  getSectionRef,
  focusedPath,
  focusedNonce = 0,
  onFileSelect,
  onRequestContent,
  onExpansionChange,
  sectionKeySuffix,
  showBottomBorder,
  hideLastBottomBorder = false,
  showRenamePath = false,
  flat = false,
  compactHeaderGutter = false,
  hideBottomPadding = false,
  viewState,
  onViewStateChange,
}: DiffSectionListProps<TFile>) {
  const virtuosoRef = useRef<VirtuosoHandle>(null);
  const searchRootRef = useRef<HTMLDivElement>(null);
  const reviewFiles = useMemo(
    () =>
      sections.map(({ file }) => ({
        path: file.path,
        oldContent: file.oldContent,
        newContent: file.newContent,
        isBinary: file.isBinary,
        isUnavailable: file.isUnavailable,
      })),
    [sections]
  );
  const navigateSearch = useCallback(
    (match: ReviewSearchMatch) => {
      const index = sections.findIndex(({ file }) => file.path === match.path);
      if (index >= 0)
        virtuosoRef.current?.scrollToIndex({
          index,
          align: "start",
          behavior: "auto",
        });
    },
    [sections]
  );
  const reviewSearch = useReviewSearch({
    enabled: enableReviewSearch,
    files: reviewSearchFiles ?? reviewFiles,
    loadFile: loadReviewFile,
    containerRef: searchRootRef,
    onNavigate: navigateSearch,
  });

  const rememberedExpansionsRef = useRef(
    new Map<string, RememberedExpansion>()
  );

  // ── Restorable view state ──────────────────────────────────────────────
  // Everything below is read once at mount: the list is rebuilt from the
  // snapshot a previous mount saved (see `viewState.ts`), not kept alive.
  const restoredExpansionsRef = useRef(createRestoredExpansions(viewState));
  const [restoredScroll] = useState<StateSnapshot | undefined>(
    () => viewState?.scroll ?? undefined
  );
  const lastScrollRef = useRef<StateSnapshot | null>(viewState?.scroll ?? null);
  const handledFocusNonceRef = useRef<number | null>(
    viewState?.focusNonce ?? null
  );
  const scrollerRef = useRef<HTMLElement | null>(null);
  const onViewStateChangeRef = useRef(onViewStateChange);
  useLayoutEffect(() => {
    onViewStateChangeRef.current = onViewStateChange;
  });

  const keyedSections = useMemo(
    () =>
      sections.map((section) => {
        const suffix = sectionKeySuffix?.(section) ?? "";
        return {
          section,
          renderKey: `${section.key}-${suffix}`,
        };
      }),
    [sectionKeySuffix, sections]
  );

  const keyedSectionsSignature = keyedSections
    .map(({ renderKey }) => renderKey)
    .join("\0");

  // Row → live expansion signal, rebuilt lazily per snapshot so a snapshot
  // costs O(rows) rather than O(rows × overrides). Kept in a ref (updated in
  // a layout effect, ahead of any unmount cleanup) so `emitViewState` stays
  // stable for the handlers and the unmount snapshot.
  const signalForRef = useRef<() => (renderKey: string) => number | undefined>(
    () => () => undefined
  );
  useLayoutEffect(() => {
    signalForRef.current = () => {
      const signals = new Map<string, number>();
      for (const { section, renderKey } of keyedSections) {
        const isFocused = focusedPath === section.file.path;
        signals.set(renderKey, collapseSignal + (isFocused ? focusedNonce : 0));
      }
      return (renderKey: string) => signals.get(renderKey);
    };
  }, [collapseSignal, focusedNonce, focusedPath, keyedSections]);

  const emitViewState = useCallback(() => {
    const listener = onViewStateChangeRef.current;
    if (!listener) return;
    listener({
      expanded: snapshotExpansions(
        rememberedExpansionsRef.current,
        signalForRef.current()
      ),
      scroll: lastScrollRef.current,
      focusNonce: handledFocusNonceRef.current,
    });
  }, []);

  // Expansion state belongs to this mounted list, not to recycled rows. Prune
  // removed files and discard all remembered overrides on a collapse signal so
  // virtual row unmount/remount does not either lose or leak state.
  useEffect(() => {
    const validKeys = new Set(keyedSections.map(({ renderKey }) => renderKey));
    const rememberedExpansions = rememberedExpansionsRef.current;
    for (const key of rememberedExpansions.keys()) {
      if (!validKeys.has(key)) rememberedExpansions.delete(key);
    }
    pruneRestoredExpansions(restoredExpansionsRef.current, validKeys);
  }, [keyedSections, keyedSectionsSignature]);

  // Only a *change* of the collapse signal invalidates overrides. Running on
  // mount too would wipe the overrides just restored from `viewState`.
  const previousCollapseSignalRef = useRef(collapseSignal);
  useEffect(() => {
    if (previousCollapseSignalRef.current === collapseSignal) return;
    previousCollapseSignalRef.current = collapseSignal;
    rememberedExpansionsRef.current.clear();
    restoredExpansionsRef.current.clear();
  }, [collapseSignal]);

  const handleExpansionChange = useCallback(
    (
      renderKey: string,
      file: TFile,
      expansionSignal: number,
      expanded: boolean
    ) => {
      rememberedExpansionsRef.current.set(renderKey, {
        signal: expansionSignal,
        expanded,
      });
      onExpansionChange?.(file, expanded);
      emitViewState();
    },
    [emitViewState, onExpansionChange]
  );

  // Snapshot the scroll offset (plus measured sizes) each time scrolling
  // settles. This is an event callback, so reading the handle here is safe,
  // and it works no matter when Virtuoso mounted — the list often renders a
  // placeholder first and only mounts Virtuoso once sections arrive, so a
  // handle captured at this component's mount would be null.
  const handleIsScrolling = useCallback(
    (scrolling: boolean) => {
      if (scrolling) return;
      virtuosoRef.current?.getState((snapshot) => {
        lastScrollRef.current = snapshot;
        emitViewState();
      });
    },
    [emitViewState]
  );

  const handleScrollerRef = useCallback(
    (element: HTMLElement | Window | null) => {
      scrollerRef.current = element instanceof HTMLElement ? element : null;
    },
    []
  );

  // Final snapshot when the list goes away. Layout cleanup runs while the
  // scroller is still attached, so a scroll that had not settled yet is
  // captured from the element (keeping the last measured sizes). A mount
  // the user never scrolled keeps the previously saved offset — the restored
  // scroll may not have been applied yet, and a fresh `0` would clobber it.
  useLayoutEffect(() => {
    return () => {
      const scroller = scrollerRef.current;
      if (scroller && scroller.isConnected && scroller.scrollTop > 0) {
        lastScrollRef.current = {
          ranges: lastScrollRef.current?.ranges ?? [],
          scrollTop: scroller.scrollTop,
        };
      }
      emitViewState();
    };
  }, [emitViewState]);

  useEffect(() => {
    if (!focusedPath) return;
    // A remount for the nonce that already scrolled restores the saved
    // offset instead of jumping back to the focused row.
    if (handledFocusNonceRef.current === focusedNonce) return;
    const focusedIndex = keyedSections.findIndex(
      ({ section }) => section.file.path === focusedPath
    );
    if (focusedIndex < 0) return;

    virtuosoRef.current?.scrollToIndex({
      index: focusedIndex,
      align: "start",
      behavior: "auto",
    });

    const frame = window.requestAnimationFrame(() => {
      const externalRef = getSectionRef?.(focusedPath);
      if (externalRef?.current) {
        externalRef.current.scrollIntoView({
          block: "start",
          behavior: "auto",
        });
        return;
      }
    });

    return () => window.cancelAnimationFrame(frame);
  }, [focusedPath, focusedNonce, getSectionRef, keyedSections]);

  // Once the focus scroll for this nonce has run, later remounts for the
  // same nonce must not repeat it. Recorded separately from the effect above
  // so section-list changes within a mount keep re-targeting as before.
  useEffect(() => {
    if (!focusedPath) return;
    if (handledFocusNonceRef.current === focusedNonce) return;
    const hasFocusedRow = keyedSections.some(
      ({ section }) => section.file.path === focusedPath
    );
    if (!hasFocusedRow) return;
    handledFocusNonceRef.current = focusedNonce;
    emitViewState();
  }, [emitViewState, focusedNonce, focusedPath, keyedSections]);

  if (loading && sections.length === 0) {
    return (
      <Placeholder
        variant="loading"
        placement="detail-panel"
        fillParentHeight
      />
    );
  }

  if (sections.length === 0) {
    return (
      <Placeholder
        variant="empty"
        placement="detail-panel"
        title={emptyTitle}
        subtitle={emptySubtitle}
        fillParentHeight
      />
    );
  }

  return (
    <div
      ref={searchRootRef}
      className="relative flex h-full min-h-0 flex-col overflow-hidden"
    >
      {reviewSearch.card}
      <div className="min-h-0 flex-1 overflow-hidden">
        <Virtuoso
          ref={virtuosoRef}
          className="scrollbar-hide h-full"
          data={keyedSections}
          computeItemKey={(_index, item) => item.renderKey}
          overscan={600}
          restoreStateFrom={restoredScroll}
          isScrolling={handleIsScrolling}
          scrollerRef={handleScrollerRef}
          {...(hideBottomPadding ? {} : { components: DIFF_LIST_COMPONENTS })}
          itemContent={(index, { section, renderKey }) => {
            const isFocused = focusedPath === section.file.path;
            const expansionSignal =
              collapseSignal + (isFocused ? focusedNonce : 0);
            const rememberedExpansion =
              rememberedExpansionsRef.current.get(renderKey);
            const expandedOverride =
              rememberedExpansion?.signal === expansionSignal
                ? rememberedExpansion.expanded
                : seedRestoredExpansion(
                    rememberedExpansionsRef.current,
                    restoredExpansionsRef.current,
                    renderKey,
                    expansionSignal
                  );

            return (
              <DiffFileSection
                file={section.file}
                reviewSearch={
                  enableReviewSearch
                    ? {
                        query: reviewSearch.appliedQuery,
                        match:
                          reviewSearch.match?.path === section.file.path
                            ? reviewSearch.match
                            : null,
                      }
                    : undefined
                }
                viewMode={viewMode}
                defaultExpanded={
                  (reviewSearch.match?.path === section.file.path
                    ? true
                    : expandedOverride) ??
                  getDefaultDiffSectionExpanded({
                    flat,
                    isFocused,
                    collapseSignal,
                    defaultCollapsed,
                    sectionCount: sections.length,
                    collapseThreshold,
                  })
                }
                expansionSignal={expansionSignal}
                repoPath={repoPath}
                sectionRef={getSectionRef?.(section.file.path)}
                dataPath={section.file.path}
                onFileSelect={onFileSelect}
                onRequestContent={
                  onRequestContent
                    ? () => onRequestContent(section.file)
                    : undefined
                }
                onExpansionChange={(expanded) =>
                  handleExpansionChange(
                    renderKey,
                    section.file,
                    expansionSignal,
                    expanded
                  )
                }
                showBottomBorder={
                  hideLastBottomBorder && index === sections.length - 1
                    ? false
                    : showBottomBorder
                }
                showRenamePath={showRenamePath}
                flat={flat}
                compactHeaderGutter={compactHeaderGutter}
                noBottomPadding={hideBottomPadding}
              />
            );
          }}
        />
      </div>
    </div>
  );
}

const DiffSectionList = memo(
  DiffSectionListInner
) as typeof DiffSectionListInner;

export default DiffSectionList;
