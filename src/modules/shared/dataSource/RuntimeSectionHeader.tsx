import type { ReactNode } from "react";

import Button from "@src/components/Button";
import { useRefreshSpin } from "@src/hooks/ui/useRefreshSpin";
import { HugeiconsIcon, Refresh04Icon } from "@src/icons";
import { SECTION_SUBHEADING_CLASSES } from "@src/modules/shared/layouts/SectionLayout";

interface RuntimeSectionHeaderProps {
  title: ReactNode;
  children?: ReactNode;
  className?: string;
  dataTestId?: string;
  headingLevel?: "h2" | "h3" | "h4";
}

/**
 * Common title/action row for Runtime sections. Keeping this small makes the
 * title baseline and page-level actions consistent without constraining the
 * layout of the section content beneath it.
 */
export function RuntimeSectionHeader({
  title,
  children,
  className,
  dataTestId,
  headingLevel = "h3",
}: RuntimeSectionHeaderProps): ReactNode {
  const Heading = headingLevel;

  return (
    <div
      className={`flex min-h-9 items-center justify-between gap-3 ${className ?? ""}`}
      data-testid={dataTestId}
    >
      <Heading className={SECTION_SUBHEADING_CLASSES}>{title}</Heading>
      {children ? (
        <div className="flex shrink-0 items-center gap-1">{children}</div>
      ) : null}
    </div>
  );
}

interface RuntimeRefreshButtonProps {
  label: string;
  variant?: "secondary" | "tertiary";
  iconOnly?: boolean;
  onRefresh: () => void;
  refreshing: boolean;
  disabled?: boolean;
  dataTestId?: string;
}

/** Standard page and table-toolbar refresh action for Runtime. */
export function RuntimeRefreshButton({
  label,
  iconOnly = false,
  variant = "tertiary",
  onRefresh,
  refreshing,
  disabled = false,
  dataTestId,
}: RuntimeRefreshButtonProps): ReactNode {
  const { spinClass, handleClick } = useRefreshSpin(onRefresh, refreshing);

  return (
    <Button
      htmlType="button"
      variant={variant}
      iconOnly={iconOnly}
      size={variant === "secondary" ? "default" : "small"}
      disabled={disabled || refreshing}
      aria-label={label}
      title={label}
      onClick={handleClick}
      icon={
        <HugeiconsIcon
          icon={Refresh04Icon}
          data-icon="refresh-cw"
          size={14}
          className={spinClass}
        />
      }
      data-testid={dataTestId}
    >
      {iconOnly ? null : label}
    </Button>
  );
}
