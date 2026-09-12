import React, { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import {
  type OrgtrackSessionFinalDiff,
  getOrgtrackSessionFinalDiffs,
} from "@src/api/tauri/lineage";
import { rpc } from "@src/api/tauri/rpc";
import type { SessionProvenanceRecentSignal } from "@src/api/tauri/rpc/schemas/agentOrgs";
import FileTypeIcon from "@src/components/FileTypeIcon";
import type { IconProvider } from "@src/components/ModelIcon";
import SettingsTable, {
  SETTINGS_TABLE_CELL,
  SETTINGS_TABLE_COL,
  type SettingsTableColumn,
} from "@src/components/SettingsTable";
import Tag, { type TagProps } from "@src/components/Tag";
import { parseUnifiedDiffToOldNew } from "@src/engines/SessionCore/rendering/props/extractorShared";
import { CodeMirrorDiff } from "@src/features/CodeMirror/Diff";
import { useMountedCleanup } from "@src/hooks/lifecycle/useMounted";
import { useSessionView } from "@src/hooks/ui/tabs/useSessionView";
import {
  SECTION_GAP_CLASSES,
  SECTION_SUBHEADING_CLASSES,
} from "@src/modules/shared/layouts/SectionLayout";
import {
  CollapsibleSection,
  InlineInfoCard,
} from "@src/modules/shared/layouts/blocks";
import { formatRelativeElapsedShort } from "@src/util/data/formatters/date";

import { RuntimeRefreshButton } from "./RuntimeSectionHeader";
import SessionProvenanceSourceIcon from "./SessionProvenanceSourceIcon";
import { tildePath } from "./sourcePath";

// Map the persisted interaction `source` string to a display label + icon.
const SIGNAL_SOURCE_META: Record<
  string,
  { label: string; iconId: IconProvider }
> = {
  claude_code: { label: "Claude Code", iconId: "claude_code" },
  codex_app: { label: "Codex", iconId: "codex" },
  cursor_ide: { label: "Cursor", iconId: "cursor" },
  qwen_code: { label: "Qwen Code", iconId: "qwen_code" },
  droid: { label: "Droid", iconId: "droid" },
  trae: { label: "Trae", iconId: "trae" },
  opencode: { label: "OpenCode", iconId: "opencode" },
  windsurf: { label: "Windsurf", iconId: "windsurf" },
  kimi: { label: "Kimi", iconId: "kimi" },
  antigravity: { label: "Antigravity", iconId: "antigravity" },
  zcode: { label: "ZCode", iconId: "zcode" },
};

const ACTION_TAG_COLOR: Record<string, TagProps["color"]> = {
  read: "default",
  write: "processing",
  create: "success",
  delete: "danger",
  rename: "warning",
  search: "default",
};

// Max render width (px) for the path columns; content past this is truncated.
const PATH_COL_MAX_PX = 300;
const WORKSPACE_COL_MAX_PX = 200;
const RECENT_SIGNALS_LIMIT = 50;
const RECENT_SIGNALS_PAGE_SIZE = 10;
const RECENT_SIGNALS_PAGE_SIZE_OPTIONS = [10, 25, 50];

/**
 * Collapse the middle of a long path to `/.../`, keeping the leading segment and
 * the trailing file (plus its parent dir when it fits) so both ends stay
 * readable. Paths within `maxChars` are returned unchanged.
 */
function middleTruncatePath(path: string, maxChars: number): string {
  if (path.length <= maxChars) return path;
  const segments = path.split("/").filter(Boolean);
  if (segments.length <= 2) {
    const keep = Math.max(6, maxChars - 5);
    const head = Math.ceil(keep / 2);
    return `${path.slice(0, head)}/.../${path.slice(path.length - (keep - head))}`;
  }
  const first = segments[0];
  const last = segments[segments.length - 1];
  const withParent = `${first}/.../${segments.slice(-2).join("/")}`;
  if (withParent.length <= maxChars) return withParent;
  const fileOnly = `${first}/.../${last}`;
  if (fileOnly.length <= maxChars) return fileOnly;
  const budget = Math.max(6, maxChars - first.length - 5);
  return `${first}/.../${last.slice(last.length - budget)}`;
}

// Only mutating actions can carry a patch; reads/searches never do.
const EDIT_ACTIONS = new Set(["write", "create", "delete", "rename"]);

// Signal `filePath` is repo-relative; a final-diff `filePath` may be absolute or
// workspace-relative. Match on normalized trailing path segments.
function diffMatchesFile(diffPath: string, signalPath: string): boolean {
  const a = diffPath.replace(/\\/g, "/");
  const b = signalPath.replace(/\\/g, "/");
  return a === b || a.endsWith(`/${b}`) || b.endsWith(`/${a}`);
}

type SignalDiffState =
  | { status: "loading" }
  | { status: "empty" }
  | {
      status: "ready";
      oldValue: string;
      newValue: string;
      oldStartLine?: number;
      newStartLine?: number;
    };

/**
 * Expanded-row content for one edit signal. Lazily loads the session's final
 * diffs (deduped per session) and renders the matching file's patch. Session
 * provenance stores metadata only, so a patch exists solely for sources whose
 * transcript has been imported/reconciled (native + importable CLIs) — hence the
 * graceful empty state for hook-only sources and not-yet-imported sessions.
 */
const SignalDiffCard: React.FC<{
  signal: SessionProvenanceRecentSignal;
  fetchDiffs: (
    source: string,
    sessionId: string
  ) => Promise<OrgtrackSessionFinalDiff[]>;
}> = ({ signal, fetchDiffs }) => {
  const { t } = useTranslation("integrations");
  const [state, setState] = useState<SignalDiffState>({ status: "loading" });

  useEffect(() => {
    // Each expanded row mounts a fresh card (rows are content-keyed), so the
    // initial "loading" state already holds — no synchronous reset needed.
    let cancelled = false;
    fetchDiffs(signal.source, signal.sessionId)
      .then((diffs) => {
        if (cancelled) return;
        const match = diffs.find((diff) =>
          diffMatchesFile(diff.filePath, signal.filePath)
        );
        if (match?.diff) {
          const parsed = parseUnifiedDiffToOldNew(match.diff);
          setState({
            status: "ready",
            oldValue: parsed.oldValue,
            newValue: parsed.newValue,
            oldStartLine: parsed.oldStartLine,
            newStartLine: parsed.newStartLine,
          });
        } else if (
          match &&
          (match.oldContent != null || match.newContent != null)
        ) {
          setState({
            status: "ready",
            oldValue: match.oldContent ?? "",
            newValue: match.newContent ?? "",
          });
        } else {
          setState({ status: "empty" });
        }
      })
      .catch(() => {
        if (!cancelled) setState({ status: "empty" });
      });
    return () => {
      cancelled = true;
    };
  }, [signal.source, signal.sessionId, signal.filePath, fetchDiffs]);

  return (
    <InlineInfoCard dataTestId="session-provenance-signal-diff">
      {state.status === "loading" ? (
        <p className="text-[12px] text-text-3">
          {t("agentOrgs.sessionProvenance.signals.diffLoading", {
            defaultValue: "Loading patch…",
          })}
        </p>
      ) : state.status === "empty" ? (
        <p className="text-[12px] leading-relaxed text-text-3">
          {t("agentOrgs.sessionProvenance.signals.diffEmpty", {
            defaultValue:
              "No patch captured. Provenance records file changes as metadata; a diff appears only once this session's edits are imported.",
          })}
        </p>
      ) : (
        <div className="max-h-[360px] overflow-auto">
          <CodeMirrorDiff
            oldValue={state.oldValue}
            newValue={state.newValue}
            filePath={signal.filePath}
            viewMode="unified"
            readOnly
            autoHeight
            oldStartLine={state.oldStartLine}
            newStartLine={state.newStartLine}
          />
        </div>
      )}
    </InlineInfoCard>
  );
};

const SessionProvenanceRecentSignalsTable: React.FC = () => {
  const { t } = useTranslation("integrations");
  const { openSession } = useSessionView();
  const [signals, setSignals] = useState<
    SessionProvenanceRecentSignal[] | null
  >(null);
  const [refreshing, setRefreshing] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");
  const [expandedRowKeys, setExpandedRowKeys] = useState<string[]>([]);
  const [open, setOpen] = useState(false);
  const mountedRef = useRef(true);
  useMountedCleanup(mountedRef);
  const requestGenerationRef = useRef(0);
  const inFlightRef = useRef<
    Promise<SessionProvenanceRecentSignal[]> | undefined
  >(undefined);
  // Dedupe per-session final-diff fetches across expanded rows.
  const diffCache = useRef(
    new Map<string, Promise<OrgtrackSessionFinalDiff[]>>()
  );
  const fetchDiffs = useCallback((source: string, sessionId: string) => {
    const key = `${source}::${sessionId}`;
    let pending = diffCache.current.get(key);
    if (!pending) {
      pending = getOrgtrackSessionFinalDiffs({ source, sessionId }).catch(
        () => []
      );
      diffCache.current.set(key, pending);
    }
    return pending;
  }, []);

  const load = useCallback(async () => {
    const requestGeneration = ++requestGenerationRef.current;
    setRefreshing(true);
    // Drop cached diffs so a manual refresh re-reads newly imported patches.
    diffCache.current.clear();
    let pending = inFlightRef.current;
    if (!pending) {
      pending = rpc.agentOrgs.sessionProvenance.recentSignals({
        limit: RECENT_SIGNALS_LIMIT,
      });
      inFlightRef.current = pending;
    }
    try {
      const next = await pending;
      if (requestGeneration !== requestGenerationRef.current) return;
      setSignals(next);
    } catch {
      if (requestGeneration === requestGenerationRef.current) {
        setSignals([]);
      }
    } finally {
      if (inFlightRef.current === pending) {
        inFlightRef.current = undefined;
      }
      if (requestGeneration === requestGenerationRef.current) {
        setRefreshing(false);
      }
    }
  }, []);

  useEffect(() => {
    if (open && signals === null) void load();
  }, [load, open, signals]);

  useEffect(() => {
    return () => {
      // Tauri invokes are not abortable. Invalidate late completions so an
      // unmounted Hooks view cannot retain or publish stale signal rows.
      requestGenerationRef.current += 1;
    };
  }, []);

  const handleOpenChange = useCallback((nextOpen: boolean) => {
    // useCollapsible publishes from its state updater. Defer parent state so
    // React never sees this component update during CollapsibleSection's render.
    queueMicrotask(() => {
      if (!mountedRef.current) return;
      setOpen(nextOpen);
      if (nextOpen) return;

      // Match Usage's recent-request lifecycle: a collapsed history table keeps
      // no row/diff payload. Reopening performs one fresh bounded fetch, while a
      // quick close/reopen shares any equivalent request already in flight.
      requestGenerationRef.current += 1;
      diffCache.current.clear();
      setSignals(null);
      setRefreshing(false);
      setExpandedRowKeys([]);
    });
  }, []);

  const sourceMeta = (source: string) =>
    SIGNAL_SOURCE_META[source] ?? {
      label: source,
      iconId: source as IconProvider,
    };

  const columns: SettingsTableColumn<SessionProvenanceRecentSignal>[] = [
    {
      key: "source",
      label: t("agentOrgs.sessionProvenance.signals.col.source", {
        defaultValue: "Tool",
      }),
      width: SETTINGS_TABLE_COL.valueLg,
      renderCell: (row) => {
        const meta = sourceMeta(row.source);
        return (
          <span className={`${SETTINGS_TABLE_CELL.primaryIcon} min-w-0`}>
            <span className="shrink-0 text-text-2">
              <SessionProvenanceSourceIcon iconId={meta.iconId} />
            </span>
            <span className="truncate">{meta.label}</span>
          </span>
        );
      },
    },
    {
      key: "action",
      label: t("agentOrgs.sessionProvenance.signals.col.action", {
        defaultValue: "Action",
      }),
      width: SETTINGS_TABLE_COL.valueMd,
      renderCell: (row) => (
        <Tag size="mini" color={ACTION_TAG_COLOR[row.action] ?? "default"} pill>
          {t(`agentOrgs.sessionProvenance.signals.action.${row.action}`, {
            defaultValue: row.action,
          })}
        </Tag>
      ),
    },
    {
      key: "when",
      label: t("agentOrgs.sessionProvenance.signals.col.when", {
        defaultValue: "When",
      }),
      width: SETTINGS_TABLE_COL.valueMd,
      sorter: (a, b) => a.occurredAt.localeCompare(b.occurredAt),
      renderCell: (row) => (
        <span className="whitespace-nowrap text-text-3" title={row.occurredAt}>
          {formatRelativeElapsedShort(new Date(row.occurredAt))}
        </span>
      ),
    },
    {
      key: "workspace",
      label: t("agentOrgs.sessionProvenance.signals.col.workspace", {
        defaultValue: "Workspace",
      }),
      width: `${WORKSPACE_COL_MAX_PX}px`,
      renderCell: (row) => {
        const full = tildePath(row.workspacePath);
        return (
          <span
            className="block truncate whitespace-nowrap text-text-3"
            style={{ maxWidth: WORKSPACE_COL_MAX_PX }}
            title={row.workspacePath}
          >
            {middleTruncatePath(full, 28)}
          </span>
        );
      },
    },
    {
      key: "file",
      label: t("agentOrgs.sessionProvenance.signals.col.file", {
        defaultValue: "File",
      }),
      width: `${PATH_COL_MAX_PX}px`,
      renderCell: (row) => {
        const display = middleTruncatePath(row.filePath, 42);
        const slash = display.lastIndexOf("/");
        const dir = slash >= 0 ? display.slice(0, slash + 1) : "";
        const name = slash >= 0 ? display.slice(slash + 1) : display;
        return (
          <span
            className="flex items-center gap-1.5 overflow-hidden"
            style={{ maxWidth: PATH_COL_MAX_PX }}
            title={row.filePath}
          >
            <FileTypeIcon
              fileName={row.filePath}
              size="small"
              className="shrink-0"
            />
            <span className="truncate whitespace-nowrap">
              {dir ? <span className="text-text-3">{dir}</span> : null}
              <span className="text-text-2">{name}</span>
            </span>
          </span>
        );
      },
    },
    {
      key: "session",
      label: t("agentOrgs.sessionProvenance.signals.col.session", {
        defaultValue: "Session",
      }),
      width: `${PATH_COL_MAX_PX}px`,
      renderCell: (row) => {
        const title = row.sessionTitle?.trim();
        const label = title || row.sessionId;
        const tone = title
          ? "text-text-2"
          : "font-mono text-[12px] text-text-3";
        return (
          <button
            type="button"
            onClick={() =>
              openSession(row.sessionId, title || undefined, row.workspacePath)
            }
            title={row.sessionId}
            aria-label={t("agentOrgs.sessionProvenance.signals.openSession", {
              defaultValue: "Open session {{session}}",
              session: label,
            })}
            style={{ maxWidth: PATH_COL_MAX_PX }}
            className={`flex max-w-full min-w-0 items-center text-left hover:text-text-1 hover:underline focus-visible:underline ${tone}`}
          >
            <span className="truncate">{label}</span>
          </button>
        );
      },
    },
  ];

  const term = searchQuery.trim().toLowerCase();
  const rows = (signals ?? []).filter((row) =>
    term
      ? [
          row.source,
          row.filePath,
          row.workspacePath,
          row.sessionId,
          row.sessionTitle,
          row.action,
        ]
          .join(" ")
          .toLowerCase()
          .includes(term)
      : true
  );
  const title = t("agentOrgs.sessionProvenance.signals.title", {
    defaultValue: "Recent signals",
  });

  return (
    <div
      className={SECTION_GAP_CLASSES}
      data-testid="session-provenance-recent-signals"
    >
      <CollapsibleSection
        title={signals === null ? title : `${title} (${signals.length})`}
        defaultOpen={false}
        onOpenChange={handleOpenChange}
        titleButtonTestId="session-provenance-recent-signals-toggle"
        compact
        titleClassName={SECTION_SUBHEADING_CLASSES}
      >
        <SettingsTable<SessionProvenanceRecentSignal>
          columns={columns}
          rows={rows}
          getRowKey={(row) =>
            `${row.source}:${row.sessionId}:${row.filePath}:${row.action}:${row.occurredAt}:${row.captureMethod}`
          }
          headerHeight="tall"
          inlineHeaderToolbar
          hover
          loading={signals === null}
          // SettingsTable's built-in pagination snapshots and restores its
          // nearest scroll ancestor while switching pages/page sizes.
          pageSize={RECENT_SIGNALS_PAGE_SIZE}
          pageSizeOptions={RECENT_SIGNALS_PAGE_SIZE_OPTIONS}
          expandable={{
            expandedRowRender: (row) => (
              <SignalDiffCard signal={row} fetchDiffs={fetchDiffs} />
            ),
            // Only edit signals can carry a patch; reads/searches stay flat.
            rowExpandable: (row) => EDIT_ACTIONS.has(row.action),
            expandedRowKeys,
            onExpandedRowsChange: setExpandedRowKeys,
          }}
          emptyTitle={
            term
              ? t("agentOrgs.sessionProvenance.signals.noResults", {
                  defaultValue: "No matching signals",
                })
              : t("agentOrgs.sessionProvenance.signals.empty", {
                  defaultValue: "No hook signals received yet.",
                })
          }
          searchBar={{
            searchValue: searchQuery,
            searchPlaceholder: t("agentOrgs.sessionProvenance.signals.search", {
              defaultValue: "Search signals",
            }),
            onSearchChange: setSearchQuery,
            onSearchClear: () => setSearchQuery(""),
            searchInputSize: "default",
            rightContent: (
              <RuntimeRefreshButton
                iconOnly
                variant="secondary"
                label={t("agentOrgs.sessionProvenance.signals.refresh", {
                  defaultValue: "Refresh",
                })}
                onRefresh={() => void load()}
                refreshing={refreshing}
                dataTestId="session-provenance-recent-signals-refresh"
              />
            ),
          }}
        />
      </CollapsibleSection>
    </div>
  );
};

export default SessionProvenanceRecentSignalsTable;
