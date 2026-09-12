/**
 * PageNotice — Shared notice/result card for page and panel content.
 *
 * Single neutral treatment for every type: a 1px `border-border-1` outline, the
 * Workstation-trail radius and a half-strength dropdown shadow, with no
 * background fill so alerts don't compete with sections that already have their
 * own surface color.
 * There is deliberately no danger / warning / success color variant — `type`
 * only selects the leading icon.
 *
 * Padding (p-3, or py-1 pl-3 pr-1 when compact), icon size 14.
 * Header row: icon + title + optional action + close;
 * body (children) and subtitle render below the header.
 * When action is an object, PageNotice builds a secondary Button at 28px height.
 */
import React from "react";

import Button from "@src/components/Button";
import { DROPDOWN_PANEL } from "@src/components/Dropdown/tokens";
import {
  Cancel01Icon,
  ChevronsDownUpIcon,
  HugeiconsIcon,
  InformationCircleIcon,
  Tick01Icon,
  TriangleAlertIcon,
  UnfoldMoreIcon,
} from "@src/icons";

import "./index.scss";

/**
 * Shared neutral surface — flat outline, no tone accent, and a half-strength
 * Workstation-trail shadow for a little lift.
 */
const ALERT_SURFACE_CLASS = `border border-solid border-border-1 text-text-1 ${DROPDOWN_PANEL.shadowSoftClass}`;

/** Matches the Workstation trail surface radius. Collapsed pills stay round. */
const ALERT_RADIUS_CLASS = "rounded-xl";

const DEFAULT_ICONS: Record<string, React.ReactNode> = {
  success: (
    <HugeiconsIcon
      icon={Tick01Icon}
      data-icon="check"
      size={14}
      className="shrink-0"
    />
  ),
  danger: (
    <HugeiconsIcon
      icon={TriangleAlertIcon}
      data-icon="triangle-alert"
      size={14}
      className="shrink-0"
    />
  ),
  warning: (
    <HugeiconsIcon
      icon={TriangleAlertIcon}
      data-icon="triangle-alert"
      size={14}
      className="shrink-0"
    />
  ),
  info: (
    <HugeiconsIcon
      icon={InformationCircleIcon}
      data-icon="info"
      size={14}
      className="shrink-0"
    />
  ),
};

/**
 * Alert copy is content, not chrome — opt back in to text selection over the
 * global `* { user-select: none }` so users can select/copy titles, bodies and
 * technical details. Interactive children opt out again with `select-none`.
 */
const SELECTABLE_TEXT_CLASS = "allow-select-deep page-notice__text";

const PAGE_NOTICE_BASE_TEXT = {
  title: "block text-[13px] font-medium leading-[14px]",
  body: "text-[12px] font-normal leading-snug",
  subtitle: "mt-1 block text-[11px] opacity-70",
} as const;

const PAGE_NOTICE_TOKENS = {
  titleText: `${PAGE_NOTICE_BASE_TEXT.title} ${SELECTABLE_TEXT_CLASS}`,
  bodyText: `${PAGE_NOTICE_BASE_TEXT.body} ${SELECTABLE_TEXT_CLASS}`,
  subtitleText: `${PAGE_NOTICE_BASE_TEXT.subtitle} ${SELECTABLE_TEXT_CLASS}`,
} as const;

interface PageNoticeActionConfig {
  label: string;
  href?: string;
  onClick?: () => void;
  disabled?: boolean;
  icon?: React.ReactNode;
  iconPosition?: "left" | "right";
}

function isActionConfig(
  action: PageNoticeActionConfig | React.ReactNode
): action is PageNoticeActionConfig {
  return (
    typeof action === "object" &&
    action !== null &&
    "label" in action &&
    typeof (action as unknown as Record<string, unknown>).label === "string"
  );
}

interface PageNoticeProps {
  /**
   * Selects the default leading icon only — all types share one neutral style.
   * Defaults to "info".
   */
  type?: "success" | "danger" | "warning" | "info";
  /** Body text (below the header row) */
  children?: React.ReactNode;
  /** Title in the header row (same row as icon, action, close) */
  title?: string;
  /** Optional icon override — defaults to Check/TriangleAlert/AlertCircle/Info per type */
  icon?: React.ReactNode;
  /** Hide the icon entirely */
  hideIcon?: boolean;
  /** Optional subtitle below the body */
  subtitle?: React.ReactNode;
  /** Extra className on the outer container */
  className?: string;
  /** Reduce default-card vertical and right padding without shrinking action buttons. */
  compact?: boolean;
  /** Compact expandable pill that shows only title until expanded */
  presentation?: "default" | "pill";
  /** Optional action — object builds a 28px secondary Button; ReactNode for custom */
  action?: PageNoticeActionConfig | React.ReactNode;
  /** Show a close icon button when provided */
  onClose?: () => void;
  /** Optional close icon override */
  closeIcon?: React.ReactNode;
  /** Accessible label for close button */
  closeAriaLabel?: string;
  /** Automatically invoke onClose after this delay. Requires onClose. */
  autoCloseMs?: number;
  /** Accessible landmark/live-region role for the alert container. */
  role?: React.AriaRole;
  /** Stable test hook for the alert container. */
  dataTestId?: string;
}

const PageNotice: React.FC<PageNoticeProps> = ({
  type = "info",
  children,
  title,
  icon,
  hideIcon = false,
  subtitle,
  className,
  compact = false,
  presentation = "default",
  action,
  onClose,
  closeIcon,
  closeAriaLabel = "Close",
  autoCloseMs,
  role,
  dataTestId,
}) => {
  const [expanded, setExpanded] = React.useState(presentation !== "pill");
  const isPill = presentation === "pill";
  const cardPaddingClass = compact ? "py-1 pl-3 pr-1" : "p-3";
  const showContent = !isPill || expanded;
  const resolvedIcon =
    icon ??
    (isPill ? (
      expanded ? (
        <HugeiconsIcon
          icon={ChevronsDownUpIcon}
          data-icon="chevrons-down-up"
          size={14}
          className="shrink-0"
        />
      ) : (
        <HugeiconsIcon
          icon={UnfoldMoreIcon}
          data-icon="chevrons-up-down"
          size={14}
          className="shrink-0"
        />
      )
    ) : (
      DEFAULT_ICONS[type]
    ));
  const resolvedCloseIcon = closeIcon ?? (
    <HugeiconsIcon
      icon={Cancel01Icon}
      data-icon="x"
      size={14}
      className="shrink-0"
    />
  );
  const hasTitle = Boolean(title);

  React.useEffect(() => {
    if (!onClose || !autoCloseMs || autoCloseMs <= 0) return;
    const timeout = window.setTimeout(onClose, autoCloseMs);
    return () => window.clearTimeout(timeout);
  }, [autoCloseMs, onClose]);

  const actionNode =
    action &&
    (isActionConfig(action) ? (
      <Button
        variant="secondary"
        size="small"
        href={action.href}
        target={action.href ? "_blank" : undefined}
        rel={action.href ? "noopener noreferrer" : undefined}
        onClick={action.onClick}
        disabled={action.disabled}
        icon={action.icon}
        iconPosition={action.iconPosition}
      >
        {action.label}
      </Button>
    ) : (
      (action as React.ReactNode)
    ));

  const titleNode = (
    <div className="flex min-w-0 flex-1 items-center gap-1.5">
      {!hideIcon && (
        <span className="flex h-[14px] shrink-0 items-center">
          {resolvedIcon}
        </span>
      )}
      <div className="min-w-0 flex-1">
        {hasTitle ? (
          // Pill headers double as the expand/collapse hit area — keep their
          // text non-selectable so a drag doesn't fight the toggle.
          <span
            className={
              isPill
                ? PAGE_NOTICE_BASE_TEXT.title
                : PAGE_NOTICE_TOKENS.titleText
            }
          >
            {title}
          </span>
        ) : (
          showContent &&
          children && (
            <div
              className={`block ${isPill ? PAGE_NOTICE_BASE_TEXT.body : PAGE_NOTICE_TOKENS.bodyText}`}
            >
              {children}
            </div>
          )
        )}
      </div>
    </div>
  );

  return (
    <div
      role={role}
      data-testid={dataTestId}
      className={`page-notice ${ALERT_SURFACE_CLASS} ${isPill ? `inline-block w-fit max-w-full ${expanded ? ALERT_RADIUS_CLASS : "rounded-full"} px-3 py-2` : `${ALERT_RADIUS_CLASS} ${cardPaddingClass}`} ${className ?? ""}`}
    >
      <div className={`flex items-center ${isPill ? "gap-1" : "gap-3"}`}>
        {isPill ? (
          <button
            type="button"
            onClick={() => setExpanded((currentExpanded) => !currentExpanded)}
            aria-expanded={expanded}
            className="flex min-w-0 flex-1 items-center text-left"
          >
            {titleNode}
          </button>
        ) : (
          titleNode
        )}
        {(action || onClose) && (
          <div className="flex shrink-0 items-center gap-px">
            {action && <div className="shrink-0">{actionNode}</div>}
            {onClose && (
              <Button
                variant="tertiary"
                size="small"
                icon={resolvedCloseIcon}
                iconOnly
                title={closeAriaLabel}
                aria-label={closeAriaLabel}
                onClick={onClose}
              />
            )}
          </div>
        )}
      </div>
      {showContent && hasTitle && children && (
        <div className={`mt-2 ${PAGE_NOTICE_TOKENS.bodyText}`}>{children}</div>
      )}
      {showContent && subtitle && (
        <span className={PAGE_NOTICE_TOKENS.subtitleText}>{subtitle}</span>
      )}
    </div>
  );
};

export default PageNotice;
