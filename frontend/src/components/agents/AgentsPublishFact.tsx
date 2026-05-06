import { ExternalLink } from "lucide-react";
import type { ElementType } from "react";

import { Button } from "@/components/ui/button";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";

export function PublishFact({
  icon: Icon,
  label,
  value,
  description,
  descriptionAction,
  action,
}: {
  icon: ElementType;
  label: string;
  value: string;
  description?: string | null;
  descriptionAction?: {
    label: string;
    testId: string;
    onClick: () => void | Promise<void>;
  } | undefined;
  action?: {
    label: string;
    testId: string;
    onClick: () => void | Promise<void>;
  } | undefined;
}) {
  return (
    <div
      className="flex min-w-0 items-start gap-2 rounded-md border px-3 py-2"
      style={{
        background: "var(--bg-base)",
        borderColor: "var(--overlay-weak)",
      }}
    >
      <Icon className="mt-0.5 h-4 w-4 shrink-0 text-[var(--text-muted)]" />
      <div className="min-w-0 flex-1">
        <div className="text-[0.625rem] font-medium uppercase tracking-[0.14em] text-[var(--text-muted)]">
          {label}
        </div>
        <div className="mt-1 flex min-w-0 items-center gap-2">
          <div className="truncate text-xs font-medium text-[var(--text-primary)]">
            {value}
          </div>
          {action && (
            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  type="button"
                  variant="ghost"
                  className="h-6 w-6 shrink-0 p-0"
                  aria-label={action.label}
                  data-testid={action.testId}
                  onClick={() => void action.onClick()}
                >
                  <ExternalLink className="h-3.5 w-3.5" />
                </Button>
              </TooltipTrigger>
              <TooltipContent side="top" className="text-xs">
                {action.label}
              </TooltipContent>
            </Tooltip>
          )}
        </div>
        {description && (
          descriptionAction ? (
            <button
              type="button"
              className="mt-1 block max-w-full truncate bg-transparent p-0 text-left text-[0.625rem] text-[var(--text-muted)] transition-colors hover:text-[var(--text-secondary)]"
              onClick={() => void descriptionAction.onClick()}
              aria-label={descriptionAction.label}
              data-theme-button-skip="true"
              data-testid={descriptionAction.testId}
            >
              {description}
            </button>
          ) : (
            <div className="mt-1 truncate text-[0.625rem] text-[var(--text-muted)]">
              {description}
            </div>
          )
        )}
      </div>
    </div>
  );
}
