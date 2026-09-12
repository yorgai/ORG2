import { useAtomValue } from "jotai";
import React, {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { useTranslation } from "react-i18next";

import { rpc } from "@src/api/tauri/rpc";
import type {
  SessionProvenanceHookPlatform,
  SessionProvenanceHookStatus,
} from "@src/api/tauri/rpc/schemas/agentOrgs";
import Button from "@src/components/Button";
import type { IconProvider } from "@src/components/ModelIcon";
import PageNotice from "@src/components/PageNotice";
import SettingsTable, {
  SETTINGS_TABLE_CELL,
  SETTINGS_TABLE_COL,
  type SettingsTableColumn,
} from "@src/components/SettingsTable";
import Switch from "@src/components/Switch";
import Tag, { type TagProps } from "@src/components/Tag";
import { INFO_CARD_TOKENS } from "@src/config/detailPanelTokens";
import { useMountedCleanup } from "@src/hooks/lifecycle/useMounted";
import { ComputerTerminal01Icon, HugeiconsIcon } from "@src/icons";
import {
  SECTION_GAP_CLASSES,
  SectionContainer,
  SectionRow,
} from "@src/modules/shared/layouts/SectionLayout";
import InlineInfoCard from "@src/modules/shared/layouts/blocks/InlineInfoCard";
import { TerminalService } from "@src/services/terminal";
import {
  activeWorkspaceRootPathAtom,
  primaryWorkspaceRootPathAtom,
} from "@src/store/workspace";
import { copyText } from "@src/util/data/clipboard";
import { formatRelativeElapsedShort } from "@src/util/data/formatters/date";
import { openFileInWorkStation } from "@src/util/ui/openFileInWorkStation";

import { RuntimeRefreshButton } from "./RuntimeSectionHeader";
import SessionProvenanceSourceIcon from "./SessionProvenanceSourceIcon";
import { tildePath } from "./sourcePath";
import { startVisibilityAwarePolling } from "./visibilityPolling";

interface PlatformMeta {
  id: SessionProvenanceHookPlatform;
  label: string;
  iconId: IconProvider;
}

// Display metadata for every platform ORGII can install a managed hook into.
// Order mirrors install priority: the three original harnesses, then the newer
// additions (Qwen/Droid have no importer yet; Trae/OpenCode do).
const PLATFORMS: ReadonlyArray<PlatformMeta> = [
  { id: "claude_code", label: "Claude Code", iconId: "claude_code" },
  { id: "codex", label: "Codex", iconId: "codex" },
  { id: "cursor", label: "Cursor", iconId: "cursor" },
  { id: "qwen_code", label: "Qwen Code", iconId: "qwen_code" },
  { id: "factory_droid", label: "Droid", iconId: "droid" },
  { id: "trae", label: "Trae", iconId: "trae" },
  { id: "opencode", label: "OpenCode", iconId: "opencode" },
  { id: "windsurf", label: "Windsurf", iconId: "windsurf" },
  { id: "kimi", label: "Kimi", iconId: "kimi" },
  { id: "antigravity", label: "Antigravity", iconId: "antigravity" },
  { id: "zcode", label: "ZCode", iconId: "zcode" },
];

type StatusByPlatform = Partial<
  Record<SessionProvenanceHookPlatform, SessionProvenanceHookStatus>
>;
type ErrorByPlatform = Partial<Record<SessionProvenanceHookPlatform, string>>;

interface PlatformRow extends PlatformMeta {
  status?: SessionProvenanceHookStatus;
  error?: string;
  pending: boolean;
  loading: boolean;
}

function indexStatuses(
  statuses: SessionProvenanceHookStatus[]
): StatusByPlatform {
  return Object.fromEntries(
    statuses.map((status) => [status.platform, status])
  ) as StatusByPlatform;
}

const SessionProvenanceHookPlatformsTable: React.FC = () => {
  const { t } = useTranslation("integrations");
  const { t: tCommon } = useTranslation("common");
  const activeWorkspaceRootPath = useAtomValue(activeWorkspaceRootPathAtom);
  const primaryWorkspaceRootPath = useAtomValue(primaryWorkspaceRootPathAtom);
  const [statuses, setStatuses] = useState<StatusByPlatform>({});
  const [initialLoading, setInitialLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");
  const [pendingPlatforms, setPendingPlatforms] = useState<
    Set<SessionProvenanceHookPlatform>
  >(() => new Set());
  const [errors, setErrors] = useState<ErrorByPlatform>({});
  const [expandedRowKeys, setExpandedRowKeys] = useState<string[]>([]);
  const [launchingCodexApproval, setLaunchingCodexApproval] = useState(false);
  const approvalAutoExpanded = useRef<Set<SessionProvenanceHookPlatform>>(
    new Set()
  );
  const mountedRef = useRef(true);
  useMountedCleanup(mountedRef);
  const statusRequestRef = useRef(0);

  const [masterEnabled, setMasterEnabled] = useState(true);
  const [masterPending, setMasterPending] = useState(false);
  const [liveStatusEnabled, setLiveStatusEnabled] = useState(true);
  const [liveStatusPending, setLiveStatusPending] = useState(false);

  const handleMasterChange = useCallback(async (enabled: boolean) => {
    setMasterPending(true);
    const previous = !enabled;
    setMasterEnabled(enabled);
    try {
      const nextStatuses =
        await rpc.agentOrgs.sessionProvenance.setMasterEnabled({ enabled });
      setStatuses(indexStatuses(nextStatuses));
      setErrors({});
    } catch (error) {
      setMasterEnabled(previous);
      const message = error instanceof Error ? error.message : String(error);
      setErrors(
        Object.fromEntries(
          PLATFORMS.map(({ id }) => [id, message])
        ) as ErrorByPlatform
      );
    } finally {
      setMasterPending(false);
    }
  }, []);

  const handleLiveStatusChange = useCallback(async (enabled: boolean) => {
    setLiveStatusPending(true);
    const previous = !enabled;
    setLiveStatusEnabled(enabled);
    try {
      const nextStatuses =
        await rpc.agentOrgs.sessionProvenance.setLiveStatusEnabled({ enabled });
      setStatuses(indexStatuses(nextStatuses));
      setErrors({});
    } catch (error) {
      setLiveStatusEnabled(previous);
      const message = error instanceof Error ? error.message : String(error);
      setErrors(
        Object.fromEntries(
          PLATFORMS.map(({ id }) => [id, message])
        ) as ErrorByPlatform
      );
    } finally {
      setLiveStatusPending(false);
    }
  }, []);

  const loadStatuses = useCallback(async (silent = false) => {
    const requestId = ++statusRequestRef.current;
    if (!silent) setRefreshing(true);
    try {
      const [nextStatuses, nextMasterEnabled, nextLiveStatusEnabled] =
        await Promise.all([
          rpc.agentOrgs.sessionProvenance.status(),
          rpc.agentOrgs.sessionProvenance.masterEnabled(),
          rpc.agentOrgs.sessionProvenance.liveStatusEnabled(),
        ]);
      if (!mountedRef.current || requestId !== statusRequestRef.current) return;
      setStatuses(indexStatuses(nextStatuses));
      setMasterEnabled(nextMasterEnabled);
      setLiveStatusEnabled(nextLiveStatusEnabled);
      if (!silent) setErrors({});
    } catch (error) {
      if (
        silent ||
        !mountedRef.current ||
        requestId !== statusRequestRef.current
      ) {
        return;
      }
      const message = error instanceof Error ? error.message : String(error);
      setErrors(
        Object.fromEntries(
          PLATFORMS.map(({ id }) => [id, message])
        ) as ErrorByPlatform
      );
    } finally {
      if (mountedRef.current && requestId === statusRequestRef.current) {
        setInitialLoading(false);
        if (!silent) setRefreshing(false);
      }
    }
  }, []);

  useEffect(() => {
    void loadStatuses();
    return () => {
      statusRequestRef.current += 1;
    };
  }, [loadStatuses]);

  useEffect(() => {
    if (statuses.codex?.activationState !== "awaiting_verification") return;
    return startVisibilityAwarePolling(
      document,
      () => loadStatuses(true),
      2_000
    );
  }, [loadStatuses, statuses.codex?.activationState]);

  useEffect(() => {
    for (const platform of PLATFORMS) {
      const awaitingVerification =
        platform.id === "codex" &&
        statuses[platform.id]?.activationState === "awaiting_verification";
      if (
        awaitingVerification &&
        !approvalAutoExpanded.current.has(platform.id)
      ) {
        approvalAutoExpanded.current.add(platform.id);
        setExpandedRowKeys((current) =>
          current.includes(platform.id) ? current : [...current, platform.id]
        );
      } else if (!awaitingVerification) {
        approvalAutoExpanded.current.delete(platform.id);
      }
    }
  }, [statuses]);

  const handleReviewCodexHooks = useCallback(async () => {
    setLaunchingCodexApproval(true);
    setErrors((current) => ({ ...current, codex: undefined }));
    try {
      await TerminalService.executeInNewSession("codex", {
        name: "Codex hook approval",
        cwd: activeWorkspaceRootPath || primaryWorkspaceRootPath || undefined,
      });
    } catch (error) {
      setErrors((current) => ({
        ...current,
        codex: error instanceof Error ? error.message : String(error),
      }));
    } finally {
      setLaunchingCodexApproval(false);
    }
  }, [activeWorkspaceRootPath, primaryWorkspaceRootPath]);

  const handleChange = useCallback(
    async (platform: SessionProvenanceHookPlatform, enabled: boolean) => {
      const previous = statuses[platform];
      setPendingPlatforms((current) => new Set(current).add(platform));
      setErrors((current) => ({ ...current, [platform]: undefined }));
      setStatuses((current) => ({
        ...current,
        [platform]: current[platform]
          ? { ...current[platform], enabled, desiredEnabled: enabled }
          : current[platform],
      }));

      try {
        const nextStatus = await rpc.agentOrgs.sessionProvenance.setEnabled({
          platform,
          enabled,
        });
        setStatuses((current) => ({ ...current, [platform]: nextStatus }));
      } catch (error) {
        setStatuses((current) => ({ ...current, [platform]: previous }));
        setErrors((current) => ({
          ...current,
          [platform]: error instanceof Error ? error.message : String(error),
        }));
      } finally {
        setPendingPlatforms((current) => {
          const next = new Set(current);
          next.delete(platform);
          return next;
        });
      }
    },
    [statuses]
  );

  const rows = useMemo<PlatformRow[]>(
    () =>
      PLATFORMS.map((platform) => ({
        ...platform,
        status: statuses[platform.id],
        error: errors[platform.id] ?? statuses[platform.id]?.error ?? undefined,
        pending: pendingPlatforms.has(platform.id),
        loading: initialLoading && !statuses[platform.id],
      })),
    [statuses, errors, pendingPlatforms, initialLoading]
  );

  const term = searchQuery.trim().toLowerCase();
  const visibleRows = term
    ? rows.filter((row) => row.label.toLowerCase().includes(term))
    : rows;

  const statusTagFor = (
    row: PlatformRow
  ): { color: TagProps["color"]; labelKey: string } => {
    if (row.loading) return { color: "processing", labelKey: "checking" };
    if (row.error) return { color: "danger", labelKey: "error" };
    const status = row.status;
    if (status && status.desiredEnabled && !status.enabled) {
      return { color: "warning", labelKey: "repair" };
    }
    if (status?.activationState === "awaiting_verification") {
      return { color: "warning", labelKey: "awaitingVerification" };
    }
    return status?.enabled
      ? { color: "success", labelKey: "on" }
      : { color: "default", labelKey: "off" };
  };

  const columns: SettingsTableColumn<PlatformRow>[] = [
    {
      key: "source",
      label: t("agentOrgs.sessionProvenance.col.source", {
        defaultValue: "Tool",
      }),
      renderCell: (row) => {
        const statusTag = statusTagFor(row);
        return (
          <span className={`${SETTINGS_TABLE_CELL.primaryIcon} min-w-0`}>
            <span className="shrink-0 text-text-2">
              <SessionProvenanceSourceIcon iconId={row.iconId} />
            </span>
            <span className="truncate">{row.label}</span>
            <span
              data-testid={`session-provenance-hook-status-${row.id}`}
              data-activation-state={row.status?.activationState ?? "inactive"}
            >
              <Tag
                size="mini"
                color={statusTag.color}
                pill
                className="shrink-0"
              >
                {t(`agentOrgs.sessionProvenance.status.${statusTag.labelKey}`, {
                  defaultValue: statusTag.labelKey,
                })}
              </Tag>
            </span>
          </span>
        );
      },
    },
    {
      key: "config",
      label: t("agentOrgs.sessionProvenance.col.config", {
        defaultValue: "Config",
      }),
      renderCell: (row) =>
        row.status?.configPath ? (
          <span
            className="block truncate text-text-3"
            title={row.status.configPath}
          >
            {tildePath(row.status.configPath)}
          </span>
        ) : null,
    },
    {
      key: "capture",
      label: "",
      width: SETTINGS_TABLE_COL.hug,
      align: "right",
      renderCell: (row) => (
        <div className="flex items-center justify-end">
          <Switch
            checked={row.status?.enabled ?? false}
            disabled={row.loading || !masterEnabled}
            loading={row.pending}
            ariaLabel={`${row.label} — ${t(
              "agentOrgs.sessionProvenance.capture",
              { defaultValue: "Capture file interactions" }
            )}`}
            dataTestId={`session-provenance-hook-switch-${row.id}`}
            onCheckedChange={(enabled) => void handleChange(row.id, enabled)}
          />
        </div>
      ),
    },
  ];

  const description = useCallback(
    (row: PlatformRow): string => {
      if (row.error) return row.error;
      const status = row.status;
      if (status && status.desiredEnabled && !status.enabled) {
        return t("agentOrgs.sessionProvenance.installDrift", {
          defaultValue:
            "The saved preference and installed hook differ. Toggle capture to repair the managed hook. Config: {{path}}",
          path: status.configPath,
        });
      }
      if (status?.activationState === "awaiting_verification") {
        return t("agentOrgs.sessionProvenance.codexApproval.description", {
          defaultValue:
            "Waiting for Codex to approve and execute the current ORG2 hooks.",
        });
      }
      if (status?.activationState === "active" && status.lastActivatedAt) {
        return t("agentOrgs.sessionProvenance.codexApproval.verified", {
          defaultValue: "Verified by a real Codex hook signal {{time}}.",
          time: formatRelativeElapsedShort(new Date(status.lastActivatedAt)),
        });
      }
      return t("agentOrgs.sessionProvenance.description", {
        defaultValue:
          "Records file reads and writes as metadata. Prompts, tool output, and file contents are not stored.",
      });
    },
    [t]
  );

  return (
    <div className={SECTION_GAP_CLASSES}>
      <SectionContainer>
        <SectionRow
          label={t("agentOrgs.sessionProvenance.masterToggle", {
            defaultValue: "Provenance hooks",
          })}
          description={t("agentOrgs.sessionProvenance.masterToggleDesc", {
            defaultValue:
              "When off, all managed hooks are uninstalled and no signals are captured",
          })}
        >
          <Switch
            checked={masterEnabled}
            loading={masterPending}
            onCheckedChange={(enabled) => void handleMasterChange(enabled)}
            ariaLabel={t("agentOrgs.sessionProvenance.masterToggle", {
              defaultValue: "Provenance hooks",
            })}
          />
        </SectionRow>
        <SectionRow
          label={t("agentOrgs.sessionProvenance.liveStatusToggle", {
            defaultValue: "Live agent status",
          })}
          description={t("agentOrgs.sessionProvenance.liveStatusToggleDesc", {
            defaultValue:
              "Shows when running CLI agents are working or waiting. Off keeps provenance capture on.",
          })}
        >
          <Switch
            checked={liveStatusEnabled}
            loading={liveStatusPending}
            disabled={!masterEnabled}
            onCheckedChange={(enabled) => void handleLiveStatusChange(enabled)}
            ariaLabel={t("agentOrgs.sessionProvenance.liveStatusToggle", {
              defaultValue: "Live agent status",
            })}
          />
        </SectionRow>
      </SectionContainer>
      <SettingsTable<PlatformRow>
        columns={columns}
        rows={visibleRows}
        getRowKey={(row) => row.id}
        headerHeight="tall"
        inlineHeaderToolbar
        className="table-expanded-no-hover table-settings-expanded-compact"
        hover
        loading={initialLoading && rows.every((row) => !row.status)}
        emptyTitle={term ? tCommon("status.noResults") : undefined}
        searchBar={{
          searchValue: searchQuery,
          searchPlaceholder: tCommon("common.searchPlaceholder"),
          onSearchChange: setSearchQuery,
          onSearchClear: () => setSearchQuery(""),
          searchInputSize: "default",
          rightContent: (
            <RuntimeRefreshButton
              iconOnly
              variant="secondary"
              label={tCommon("actions.refresh")}
              onRefresh={() => void loadStatuses()}
              refreshing={refreshing}
              dataTestId="session-provenance-hooks-refresh"
            />
          ),
        }}
        expandable={{
          expandedRowRender: (row) => (
            <InlineInfoCard
              dataTestId={`session-provenance-hook-card-${row.id}`}
            >
              <div className={`grid ${INFO_CARD_TOKENS.rowGap}`}>
                <div className="flex items-start justify-between gap-3">
                  <span className={`${INFO_CARD_TOKENS.label} pt-1`}>
                    {t("agentOrgs.sessionProvenance.col.config", {
                      defaultValue: "Config",
                    })}
                  </span>
                  {row.status?.configPath ? (
                    <div className="flex min-w-0 items-center gap-1.5">
                      <span
                        className="min-w-0 truncate text-[12px] text-text-1"
                        title={row.status.configPath}
                      >
                        {tildePath(row.status.configPath)}
                      </span>
                      <Button
                        variant="secondary"
                        size="small"
                        onClick={() => void copyText(row.status!.configPath)}
                      >
                        {t("agentOrgs.sessionProvenance.copyPath", {
                          defaultValue: "Copy",
                        })}
                      </Button>
                      <Button
                        variant="secondary"
                        size="small"
                        onClick={() =>
                          openFileInWorkStation(row.status!.configPath)
                        }
                      >
                        {tCommon("actions.open")}
                      </Button>
                    </div>
                  ) : (
                    <span className={INFO_CARD_TOKENS.value}>—</span>
                  )}
                </div>
                <p className="text-[12px] leading-relaxed text-text-2">
                  {description(row)}
                </p>
                {row.id === "codex" &&
                  row.status?.activationState === "awaiting_verification" && (
                    <PageNotice
                      type="warning"
                      dataTestId="session-provenance-codex-approval"
                      title={t(
                        "agentOrgs.sessionProvenance.codexApproval.title",
                        { defaultValue: "Verify ORG2 hooks in Codex" }
                      )}
                      action={
                        <span data-testid="session-provenance-review-codex-hooks">
                          <Button
                            variant="primary"
                            size="small"
                            icon={
                              <HugeiconsIcon
                                icon={ComputerTerminal01Icon}
                                data-icon="terminal"
                                size={14}
                              />
                            }
                            loading={launchingCodexApproval}
                            onClick={() => void handleReviewCodexHooks()}
                          >
                            {t(
                              "agentOrgs.sessionProvenance.codexApproval.review",
                              {
                                defaultValue: "Review in Codex",
                              }
                            )}
                          </Button>
                        </span>
                      }
                    >
                      {t(
                        "agentOrgs.sessionProvenance.codexApproval.instructions",
                        {
                          defaultValue:
                            "Open Codex, review the ORG2 hooks, then choose Trust all and continue. The SessionStart hook verifies activation automatically when the session starts.",
                        }
                      )}
                    </PageNotice>
                  )}
              </div>
            </InlineInfoCard>
          ),
          rowExpandable: () => true,
          expandedRowKeys,
          onExpandedRowsChange: setExpandedRowKeys,
        }}
      />
    </div>
  );
};

export default SessionProvenanceHookPlatformsTable;
