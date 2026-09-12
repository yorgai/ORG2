/**
 * RuntimeScanningPanel
 *
 * Runtime-owned inventory of every external coding tool ORGII detects, driven
 * by the one shared detect pipeline (`external_cli_sources_detect`). Importable
 * apps (Cursor, Codex, Claude, OpenCode, Windsurf, WorkBuddy) show their
 * imported-session count and can be enabled/disabled, auto-scanned on a
 * schedule, and rescanned on demand; the rest show install status. Every row
 * shows the on-disk path + file type.
 *
 * Per-source config (enabled / frequency / lastScannedAt) is persisted via
 * `dataSourceConfigAtom`. A disabled source is gated out of external-history
 * sidebar loads so its sessions never load anywhere. Rescan re-runs detection
 * and performs an incremental import by default, with an explicit
 * clear-and-rebuild option.
 *
 * Scanning-inventory state (detect + stats fan-out, rescan, toggle) lives in
 * `useRuntimeScanningPanelInventory`; the settings-table column definitions
 * live in `buildRuntimeScanningPanelColumns`. This file wires that state to
 * the tab/search UI and the table/expandable-row layout.
 */
import { invoke } from "@tauri-apps/api/core";
import { useAtom } from "jotai";
import React, { useCallback, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import Select from "@src/components/Select";
import SettingsTable from "@src/components/SettingsTable";
import Switch from "@src/components/Switch";
import TabPill, { type TabPillItem } from "@src/components/TabPill";
import {
  SECTION_CONTROL_STYLE,
  SECTION_GAP_CLASSES,
  SectionContainer,
  SectionRow,
} from "@src/modules/shared/layouts/SectionLayout";
import {
  ACTIVE_EXTERNAL_SESSION_REFRESH_FREQUENCIES,
  type ActiveExternalSessionRefreshFrequency,
  GLOBAL_FREQUENCIES,
  SOURCE_FREQUENCIES,
  type ScanFrequency,
  activeExternalSessionRefreshFrequencyAtom,
  dataSourceGlobalFrequencyAtom,
  externalSessionsEnabledAtom,
} from "@src/store/session/dataSourceConfigAtom";
import { copyText } from "@src/util/data/clipboard";

import DataSourceDetailsCard from "./DataSourceDetailsCard";
import { buildRuntimeScanningPanelColumns } from "./RuntimeScanningPanelColumns";
import { useRuntimeScanningPanelInventory } from "./RuntimeScanningPanelInventory";
import type { DataSourceTab, SourceRow } from "./RuntimeScanningPanelTypes";
import {
  RuntimeRefreshButton,
  RuntimeSectionHeader,
} from "./RuntimeSectionHeader";

const RuntimeScanningPanel: React.FC = () => {
  const { t } = useTranslation("sessions", {
    keyPrefix: "kanban.dataSource",
  });
  const { t: tCommon } = useTranslation("common");
  const {
    rows,
    rescanningAll,
    configMap,
    handleRescan,
    handleRescanAll,
    toggleEnabled,
    updateConfig,
  } = useRuntimeScanningPanelInventory();
  // sourceId whose rescan split-menu is open (null = none).
  const [openRescanMenu, setOpenRescanMenu] = useState<string | null>(null);
  const [tab, setTab] = useState<DataSourceTab>("all");
  const [expandedRowKeys, setExpandedRowKeys] = useState<string[]>([]);
  const [searchQuery, setSearchQuery] = useState("");
  const [globalFrequency, setGlobalFrequency] = useAtom(
    dataSourceGlobalFrequencyAtom
  );
  const [activeSessionRefreshFrequency, setActiveSessionRefreshFrequency] =
    useAtom(activeExternalSessionRefreshFrequencyAtom);
  const [externalSessionsEnabled, setExternalSessionsEnabled] = useAtom(
    externalSessionsEnabledAtom
  );

  const openFolder = useCallback((path: string) => {
    void invoke("open_folder", { path }).catch(() => {
      /* best-effort reveal */
    });
  }, []);

  const tabs = useMemo<TabPillItem[]>(
    () => [
      { key: "all", label: t("tabs.all") },
      { key: "apps", label: t("tabs.apps") },
      { key: "clis", label: t("tabs.clis") },
    ],
    [t]
  );

  const sourceFrequencyOptions = useMemo(
    () => SOURCE_FREQUENCIES.map((f) => ({ value: f, label: t(`freq.${f}`) })),
    [t]
  );
  const globalFrequencyOptions = useMemo(
    () => GLOBAL_FREQUENCIES.map((f) => ({ value: f, label: t(`freq.${f}`) })),
    [t]
  );
  const activeSessionRefreshFrequencyOptions = useMemo(
    () =>
      ACTIVE_EXTERNAL_SESSION_REFRESH_FREQUENCIES.map((frequency) => ({
        value: frequency,
        label: t(`activeSessionFreq.${frequency}`),
      })),
    [t]
  );

  const visibleRows = (rows ?? []).filter((row) =>
    tab === "apps" ? row.importable : tab === "clis" ? !row.importable : true
  );
  const importableCount = (rows ?? []).filter((r) => r.importable).length;

  const searchTerm = searchQuery.trim().toLowerCase();
  const searchedRows = searchTerm
    ? visibleRows.filter((row) =>
        [row.probe.displayName, row.probe.sourceId, ...row.probe.historyPaths]
          .join(" ")
          .toLowerCase()
          .includes(searchTerm)
      )
    : visibleRows;

  const columns = buildRuntimeScanningPanelColumns({
    t,
    configMap,
    sourceFrequencyOptions,
    openRescanMenu,
    setOpenRescanMenu,
    toggleEnabled,
    updateConfig,
    handleRescan,
  });

  return (
    <div className={SECTION_GAP_CLASSES} data-testid="runtime-scanning-panel">
      <RuntimeSectionHeader
        title={t("views.scanning")}
        className="-mx-4 bg-chat-pane px-4 pt-2 pb-1"
        dataTestId="runtime-scanning-title"
      />
      <>
        {importableCount > 0 && (
          <SectionContainer>
            <SectionRow
              label={t("externalSessionsToggle")}
              description={t("externalSessionsToggleDesc")}
            >
              <Switch
                checked={externalSessionsEnabled}
                onCheckedChange={(checked) =>
                  setExternalSessionsEnabled(checked)
                }
                ariaLabel={t("externalSessionsToggle")}
              />
            </SectionRow>
            <SectionRow
              label={t("globalFrequency")}
              description={t("globalFrequencyDesc")}
            >
              <Select
                value={globalFrequency}
                onChange={(value) => {
                  if (typeof value === "string") {
                    setGlobalFrequency(value as ScanFrequency);
                  }
                }}
                options={globalFrequencyOptions}
                size="default"
                style={SECTION_CONTROL_STYLE}
                aria-label={t("globalFrequency")}
                disabled={!externalSessionsEnabled}
              />
            </SectionRow>
            <SectionRow
              label={t("activeSessionRefresh")}
              description={t("activeSessionRefreshDesc")}
            >
              <Select
                value={activeSessionRefreshFrequency}
                onChange={(value) => {
                  if (typeof value === "string") {
                    setActiveSessionRefreshFrequency(
                      value as ActiveExternalSessionRefreshFrequency
                    );
                  }
                }}
                options={activeSessionRefreshFrequencyOptions}
                size="default"
                style={SECTION_CONTROL_STYLE}
                aria-label={t("activeSessionRefresh")}
                disabled={!externalSessionsEnabled}
              />
            </SectionRow>
          </SectionContainer>
        )}

        <SettingsTable<SourceRow>
          columns={columns}
          rows={searchedRows}
          getRowKey={(row) => row.probe.sourceId}
          headerHeight="tall"
          // Keep search + tabs + rescan inline when space allows; the shared
          // toolbar stacks search/actions above the tabs in narrow panels.
          inlineHeaderToolbar
          className="table-expanded-no-hover table-settings-expanded-compact"
          hover
          loading={rows === null}
          emptyTitle={searchTerm ? tCommon("status.noResults") : undefined}
          searchBar={{
            searchValue: searchQuery,
            searchPlaceholder: tCommon("common.searchPlaceholder"),
            onSearchChange: setSearchQuery,
            onSearchClear: () => setSearchQuery(""),
            rightContent:
              (rows?.length ?? 0) > 0 ? (
                <RuntimeRefreshButton
                  iconOnly
                  variant="secondary"
                  label={t("rescanAll")}
                  onRefresh={() => void handleRescanAll()}
                  refreshing={rescanningAll}
                  disabled={!externalSessionsEnabled}
                  dataTestId="runtime-scanning-rescan-all"
                />
              ) : undefined,
            tabPills: (
              <TabPill
                activeTab={tab}
                tabs={tabs}
                onChange={(key) => setTab(key as DataSourceTab)}
                variant="pill"
                color="fill"
                className="h-8 [&>button]:h-full!"
                fillWidth={false}
                size="small"
                buttonStyle
              />
            ),
            searchInputSize: "default",
            searchCountText:
              searchTerm && searchedRows.length !== visibleRows.length
                ? `${searchedRows.length} / ${visibleRows.length}`
                : undefined,
          }}
          expandable={{
            expandedRowRender: (row) => (
              <DataSourceDetailsCard
                probe={row.probe}
                stats={row.stats}
                onOpenFolder={openFolder}
                onCopyPath={(path) => void copyText(path)}
              />
            ),
            rowExpandable: (row) => row.probe.historyPaths.length > 0,
            expandedRowKeys,
            onExpandedRowsChange: setExpandedRowKeys,
          }}
        />
      </>
    </div>
  );
};

export default RuntimeScanningPanel;
