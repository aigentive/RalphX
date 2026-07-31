function ComposerRuntimeFastModeButton({
  model,
}: {
  model: ComposerRuntimeModelField;
}) {
  const fastMode = model.fastMode;
  if (!fastMode?.visible) return null;
  const disabled = fastMode.disabled ?? false;
  const stateLabel = fastMode.value
    ? "Fast mode is on"
    : disabled
      ? "Fast mode is unavailable"
      : "Fast mode is off";

  return (
    <Tooltip delayDuration={180}>
      <TooltipTrigger asChild>
        <span className="inline-flex shrink-0">
          <button
            type="button"
            data-testid={
              fastMode.testId ?? "agent-composer-runtime-codex-fast-mode"
            }
            aria-label={
              fastMode.value ? "Turn Fast mode off" : "Turn Fast mode on"
            }
            aria-pressed={fastMode.value}
            disabled={disabled}
            className={cn(
              "flex h-8 w-8 shrink-0 items-center justify-center rounded-md border outline-none transition-colors focus-visible:ring-2 focus-visible:ring-[var(--accent-muted)] disabled:cursor-not-allowed disabled:opacity-45",
              fastMode.value
                ? "bg-[var(--accent-muted)] text-[var(--accent-primary)]"
                : "text-[var(--text-secondary)] hover:bg-[var(--bg-hover)]",
            )}
            style={{
              borderColor: fastMode.value
                ? "var(--accent-border)"
                : "var(--border-subtle)",
              borderStyle: "solid",
              borderWidth: "1px",
            }}
            onClick={() => fastMode.onValueChange(!fastMode.value)}
          >
            <Zap className="h-4 w-4" aria-hidden="true" />
            <span className="sr-only">Fast mode</span>
            {fastMode.description && (
              <span className="sr-only">{fastMode.description}</span>
            )}
          </button>
        </span>
      </TooltipTrigger>
      <TooltipContent side="top" className="max-w-64 leading-snug">
        <div>{stateLabel}</div>
        <div className="mt-1 font-normal opacity-80">
          {fastMode.description ?? "Use Codex priority processing when supported."}
        </div>
      </TooltipContent>
    </Tooltip>
  );
}

interface ComposerRuntimeMenuHeaderProps {
  model: ComposerRuntimeModelField;
  runtimeDefault?: {
    source?: string | null;
    scopeLabel?: string;
    isResetting?: boolean;
    disabled?: boolean;
    onReset: () => Promise<unknown> | void;
  };
}

export function ComposerRuntimeMenuHeader({
  model,
  runtimeDefault,
}: ComposerRuntimeMenuHeaderProps) {
  return (
    <div className="flex shrink-0 items-center gap-2 p-2 pb-1">
      <div className="flex h-8 min-w-0 flex-1 items-center px-2 text-[0.75rem] font-medium text-[var(--text-secondary)]">
        {runtimeDefault?.scopeLabel ?? "Advanced"}
      </div>
                  {runtimeDefault && (
                    <Tooltip delayDuration={180}>
                      <TooltipTrigger asChild>
                        <button
                          type="button"
                          aria-label="Reset runtime to current role default"
                          data-testid="agent-composer-runtime-reset"
                          disabled={
                            runtimeDefault.disabled || runtimeDefault.isResetting
                          }
                          className="flex h-8 w-8 shrink-0 items-center justify-center rounded-md border text-[var(--text-secondary)] outline-none transition-colors hover:bg-[var(--bg-hover)] focus-visible:ring-2 focus-visible:ring-[var(--accent-muted)] disabled:cursor-not-allowed disabled:opacity-45"
                          style={{
                            backgroundColor: "transparent",
                            borderColor: "var(--border-subtle)",
                            borderStyle: "solid",
                            borderWidth: "1px",
                          }}
                          onClick={() => void runtimeDefault.onReset()}
                        >
                          {runtimeDefault.isResetting ? (
                            <Loader2
                              className="h-4 w-4 animate-spin"
                              aria-hidden="true"
                            />
                          ) : (
                            <RotateCcw className="h-4 w-4" aria-hidden="true" />
                          )}
                        </button>
                      </TooltipTrigger>
                      <TooltipContent side="top" className="max-w-64 leading-snug">
                        Reset provider, model, effort, speed, capability, and
                        persona to the {runtimeDefault.scopeLabel ?? "current role default"}
                        {runtimeDefault.source
                          ? ` (${runtimeDefault.source})`
                          : ""}
                      </TooltipContent>
                    </Tooltip>
                  )}
                  <ComposerRuntimeFastModeButton model={model} />
    </div>
  );
}
import { Loader2, RotateCcw, Zap } from "lucide-react";

import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";

import type { ComposerRuntimeModelField } from "./runtimeSelectorTypes";
