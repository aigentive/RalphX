import type { ComponentType } from "react";
import { X } from "lucide-react";

import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";

export function ComposerReferencePill({
  testId,
  icon: Icon,
  typeLabel,
  label,
  description,
  contentTooltip,
  removeLabel,
  removeDisabled = false,
  removeDisabledReason,
  onRemove,
}: {
  testId: string;
  icon: ComponentType<{ className?: string }>;
  typeLabel: string;
  label: string;
  description?: string;
  contentTooltip?: string;
  removeLabel: string;
  removeDisabled?: boolean;
  removeDisabledReason?: string;
  onRemove: () => void;
}) {
  const content = (
    <span
      className="flex min-w-0 items-center gap-2 rounded-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent)]"
      tabIndex={contentTooltip ? 0 : undefined}
    >
      <Icon className="h-3.5 w-3.5 shrink-0 text-[var(--text-secondary)]" />
      <span className="shrink-0 rounded-md border px-1.5 py-0.5 text-[0.625rem] font-medium uppercase text-[var(--text-muted)]">
        {typeLabel}
      </span>
      <span
        className="min-w-0 max-w-[16rem] truncate font-medium"
        title={label}
      >
        {label}
      </span>
      {description && description !== label ? (
        <span
          className="hidden min-w-0 max-w-[18rem] truncate text-[var(--text-muted)] sm:inline"
          title={description}
        >
          {description}
        </span>
      ) : null}
    </span>
  );

  return (
    <span
      data-testid={testId}
      className="inline-flex h-9 max-w-full items-center gap-2 rounded-lg border px-2 text-[0.75rem]"
      style={{
        backgroundColor: "var(--bg-surface)",
        borderColor: "var(--bg-hover)",
        borderStyle: "solid",
        borderWidth: "1px",
        color: "var(--text-primary)",
      }}
    >
      {contentTooltip ? (
        <Tooltip>
          <TooltipTrigger asChild>{content}</TooltipTrigger>
          <TooltipContent side="top" className="text-xs">
            {contentTooltip}
          </TooltipContent>
        </Tooltip>
      ) : (
        content
      )}
      <Tooltip>
        <TooltipTrigger asChild>
          <button
            type="button"
            className="ml-0.5 shrink-0 rounded p-0.5 text-[var(--text-secondary)] transition-colors hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)] disabled:opacity-50"
            aria-label={removeLabel}
            disabled={removeDisabled}
            onClick={onRemove}
          >
            <X className="h-3.5 w-3.5" />
          </button>
        </TooltipTrigger>
        <TooltipContent side="top" className="text-xs">
          {removeDisabledReason ?? removeLabel}
        </TooltipContent>
      </Tooltip>
    </span>
  );
}
