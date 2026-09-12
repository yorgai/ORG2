import { useMemo } from "react";
import { useTranslation } from "react-i18next";

import type {
  UsageRoundRow,
  UsageSessionSort,
} from "@src/api/tauri/usageDashboard";
import { Placeholder } from "@src/components/Placeholder";
import SettingsTable, {
  type SettingsTableColumn,
  SettingsTablePagination,
  type SettingsTableSelectFilter,
} from "@src/components/SettingsTable";
import Tooltip from "@src/components/Tooltip";
import { SECTION_SUBHEADING_CLASSES } from "@src/modules/shared/layouts/SectionLayout";
import { CollapsibleSection } from "@src/modules/shared/layouts/blocks";
import { formatRelativeElapsedShort } from "@src/util/data/formatters/date";

import { RuntimeRefreshButton } from "./RuntimeSectionHeader";
import UsagePricingHint from "./UsagePricingHint";
import { BucketIcon } from "./usageBuckets";
import { formatCacheRW, formatTokensShort, formatUsd } from "./usageFormat";

const MODEL_ALL = "__all_models__";
const MODEL_UNKNOWN = "__unknown_model__";
const PAGE_SIZE_OPTIONS = [10, 25, 50, 100];
export const USAGE_ROUNDS_DEFAULT_PAGE_SIZE = 10;

interface UsageRoundsTableProps {
  rows: UsageRoundRow[];
  total: number;
  availableModels: string[];
  hasUnknownModel: boolean;
  modelFilter: string | null | undefined;
  onModelFilterChange: (model: string | null | undefined) => void;
  searchQuery: string;
  onSearchQueryChange: (query: string) => void;
  sort: UsageSessionSort;
  onSortChange: (sort: UsageSessionSort) => void;
  pageIndex: number;
  pageSize: number;
  onPageChange: (pageIndex: number) => void;
  onPageSizeChange: (pageSize: number) => void;
  loaded: boolean;
  error?: string | null;
  onOpenChange: (open: boolean) => void;
  onRefresh: () => void;
  loading?: boolean;
  /** Click a round's session to scope the whole dashboard to it. */
  onSelectSession: (sessionId: string) => void;
}

/** Dollar figure: more precision for sub-dollar spend, 2dp otherwise. */
function costLabel(value: number): string {
  return formatUsd(value, value > 0 && value < 1 ? 4 : 2);
}

export default function UsageRoundsTable({
  rows,
  total,
  availableModels,
  hasUnknownModel,
  modelFilter,
  onModelFilterChange,
  searchQuery,
  onSearchQueryChange,
  sort,
  onSortChange,
  pageIndex,
  pageSize,
  onPageChange,
  onPageSizeChange,
  loaded,
  error,
  onOpenChange,
  onRefresh,
  loading = false,
  onSelectSession,
}: UsageRoundsTableProps) {
  const { t } = useTranslation("sessions", { keyPrefix: "kanban.dataSource" });
  const { t: tCommon } = useTranslation("common");

  const columns = useMemo<SettingsTableColumn<UsageRoundRow>[]>(
    () => [
      {
        key: "time",
        label: t("usage.roundsTable.time"),
        width: 96,
        renderCell: (record) =>
          record.createdAtMs > 0 ? (
            <span className="text-text-2">
              {formatRelativeElapsedShort(new Date(record.createdAtMs))}
            </span>
          ) : (
            <span className="text-text-3">—</span>
          ),
      },
      {
        key: "session",
        label: t("usage.roundsTable.session"),
        renderCell: (record) => (
          <button
            type="button"
            onClick={() => onSelectSession(record.sessionId)}
            title={t("usage.roundsTable.filterBySession")}
            className="flex items-center gap-1.5 truncate text-left text-text-1 hover:text-primary-6"
          >
            <BucketIcon bucket={record.bucket} size={14} />
            <span className="max-w-[220px] truncate">{record.sessionName}</span>
          </button>
        ),
      },
      {
        key: "model",
        label: t("usage.roundsTable.model"),
        width: 150,
        renderCell: (record) => (
          <span
            className="block max-w-[150px] truncate text-text-3"
            title={record.model ?? ""}
          >
            {record.model || "—"}
          </span>
        ),
      },
      {
        key: "input",
        label: t("usage.roundsTable.input"),
        align: "right",
        width: 120,
        renderCell: (record) => {
          const cache = formatCacheRW(
            record.cacheReadTokens,
            record.cacheWriteTokens
          );
          return (
            <div className="flex flex-col items-end">
              <span className="text-text-2 tabular-nums">
                {formatTokensShort(record.inputTokens)}
              </span>
              {cache && (
                <span className="text-[10px] text-text-3 tabular-nums">
                  {cache}
                </span>
              )}
            </div>
          );
        },
      },
      {
        key: "output",
        label: t("usage.roundsTable.output"),
        align: "right",
        width: 80,
        renderCell: (record) => (
          <span className="text-text-2 tabular-nums">
            {formatTokensShort(record.outputTokens)}
          </span>
        ),
      },
      {
        key: "cost",
        label: t("usage.roundsTable.cost"),
        align: "right",
        width: 88,
        renderCell: (record) => (
          <Tooltip
            position="bottom"
            mouseEnterDelay={500}
            content={
              <UsagePricingHint
                breakdown={{
                  model: record.model,
                  inputTokens: record.inputTokens,
                  outputTokens: record.outputTokens,
                  cacheReadTokens: record.cacheReadTokens,
                  cacheWriteTokens: record.cacheWriteTokens,
                }}
              />
            }
          >
            <span className="cursor-help text-text-1 tabular-nums underline decoration-text-3 decoration-dotted underline-offset-2">
              {costLabel(record.costUsd)}
            </span>
          </Tooltip>
        ),
      },
    ],
    [t, onSelectSession]
  );

  const sortOptions = useMemo(
    () => [
      { value: "recent", label: t("usage.table.sort.recent") },
      { value: "cost", label: t("usage.table.sort.cost") },
      { value: "tokens", label: t("usage.table.sort.tokens") },
    ],
    [t]
  );

  const modelFilterOptions = useMemo(() => {
    return [
      {
        value: MODEL_ALL,
        label: tCommon("selectors.modelSelector.allModels"),
      },
      ...availableModels.map((model) => ({ value: model, label: model })),
      ...(hasUnknownModel
        ? [
            {
              value: MODEL_UNKNOWN,
              label: tCommon("status.unknown"),
            },
          ]
        : []),
    ];
  }, [availableModels, hasUnknownModel, tCommon]);

  const modelFilterValue =
    modelFilter === undefined
      ? MODEL_ALL
      : modelFilter === null
        ? MODEL_UNKNOWN
        : modelFilter;

  const selectFilters = useMemo<SettingsTableSelectFilter[]>(
    () => [
      {
        key: "model",
        value: modelFilterValue,
        defaultValue: MODEL_ALL,
        options: modelFilterOptions,
        onChange: (value) => {
          const next = String(value);
          onModelFilterChange(
            next === MODEL_ALL
              ? undefined
              : next === MODEL_UNKNOWN
                ? null
                : next
          );
        },
      },
      {
        key: "sort",
        value: sort,
        defaultValue: "recent",
        options: sortOptions,
        onChange: (value) => onSortChange(value as UsageSessionSort),
      },
    ],
    [
      modelFilterOptions,
      modelFilterValue,
      onModelFilterChange,
      onSortChange,
      sort,
      sortOptions,
    ]
  );

  const isFiltered =
    searchQuery.trim().length > 0 || modelFilterValue !== MODEL_ALL;
  const pageCount = Math.max(1, Math.ceil(total / pageSize));
  const showPagination = total > Math.min(...PAGE_SIZE_OPTIONS);

  return (
    <CollapsibleSection
      title={
        loaded
          ? `${t("usage.roundsTable.title")} (${total})`
          : t("usage.roundsTable.title")
      }
      defaultOpen={false}
      onOpenChange={onOpenChange}
      titleButtonTestId="usage-rounds-toggle"
      compact
      titleClassName={SECTION_SUBHEADING_CLASSES}
    >
      <SettingsTable<UsageRoundRow>
        columns={columns}
        rows={rows}
        getRowKey={(row) => row.roundId}
        loading={loading}
        footer={
          showPagination ? (
            <div className="flex h-10 w-full items-center border-t border-border-1 px-4">
              <SettingsTablePagination
                pageIndex={pageIndex}
                pageSize={pageSize}
                total={total}
                pageCount={pageCount}
                canPreviousPage={pageIndex > 0}
                canNextPage={pageIndex + 1 < pageCount}
                onPageChange={onPageChange}
                onPageSizeChange={onPageSizeChange}
                pageSizeOptions={PAGE_SIZE_OPTIONS}
              />
            </div>
          ) : undefined
        }
        inlineHeaderToolbar
        searchHeaderClassName="settings-table-toolbar-compact"
        searchBar={{
          searchValue: searchQuery,
          searchPlaceholder: tCommon("common.searchPlaceholder"),
          onSearchChange: onSearchQueryChange,
          onSearchClear: () => onSearchQueryChange(""),
          searchInputSize: "default",
          rightContent: (
            <RuntimeRefreshButton
              iconOnly
              variant="secondary"
              label={t("usage.refresh")}
              onRefresh={onRefresh}
              refreshing={loading}
              dataTestId="usage-rounds-refresh"
            />
          ),
        }}
        selectFilters={selectFilters}
        hover
        headerHeight="tall"
        noDataElement={
          error ? (
            <Placeholder
              variant="error"
              title={t("usage.loadError")}
              subtitle={error}
              onRetry={onRefresh}
            />
          ) : !loaded && !loading ? (
            <Placeholder variant="loading" />
          ) : undefined
        }
        emptyTitle={isFiltered ? tCommon("status.noResults") : undefined}
      />
    </CollapsibleSection>
  );
}
