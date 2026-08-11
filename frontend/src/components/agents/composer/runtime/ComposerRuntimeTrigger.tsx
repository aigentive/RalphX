import {
  forwardRef,
  type ComponentPropsWithoutRef,
} from "react";

import { ChevronDown, Cpu, Zap } from "lucide-react";

import type { AgentProvider } from "@/stores/agentSessionStore";
import { cn } from "@/lib/utils";

import { effortTone, selectedOptionIndex } from "./runtimeSelectorModel";
import { PROVIDER_ICONS } from "./runtimeProviderIcons";
import type { ComposerRuntimeOption } from "./runtimeSelectorTypes";


export function ComposerRuntimeEffortBars({
  effortId,
  options,
  className,
}: {
  effortId: string;
  options: readonly ComposerRuntimeOption[];
  className?: string;
}) {
  const activeCount = selectedOptionIndex(options, effortId) + 1;
  const color = effortTone(options, effortId);
  return (
    <span className={cn("inline-flex items-end gap-px", className)} aria-hidden>
      {Array.from({ length: options.length }, (_, i) => (
        <span
          key={i}
          className="rounded-[1px]"
          style={{
            width: 3,
            height: 6 + i * 2,
            backgroundColor: i < activeCount ? color : "var(--text-muted)",
            opacity: i < activeCount ? 1 : 0.25,
          }}
        />
      ))}
    </span>
  );
}

interface ComposerRuntimeTriggerProps
  extends Omit<ComponentPropsWithoutRef<"button">, "children"> {
  provider: AgentProvider;
  modelLabel: string;
  effortValue: string;
  effortLabel: string;
  effortOptions: readonly ComposerRuntimeOption[];
  runtimeSummary: string;
  visibleLabel?: string;
  includesCapabilities?: boolean;
  compact: boolean;
  fastMode: boolean;
  scopeTag?: string;
  className?: string;
}

export const ComposerRuntimeTrigger = forwardRef<
  HTMLButtonElement,
  ComposerRuntimeTriggerProps
>(function ComposerRuntimeTrigger(
  {
    provider,
    modelLabel,
    effortValue,
    effortLabel,
    effortOptions,
    runtimeSummary,
    visibleLabel,
    includesCapabilities = false,
    compact,
    fastMode,
    scopeTag,
    className,
    ...buttonProps
  },
  ref,
) {
  const ProviderIcon = PROVIDER_ICONS[provider] ?? Cpu;
  return (
    <button
      ref={ref}
      type="button"
      {...buttonProps}
      data-testid="agent-composer-runtime-pill"
      aria-label={`Runtime: ${runtimeSummary}. Choose provider, model, effort${includesCapabilities ? ", and capabilities" : ""}. Shortcut Control Shift M.`}
      aria-keyshortcuts="Control+Shift+M"
      className={cn(
        "group flex min-w-0 items-center gap-2 rounded-md border outline-none transition-[height,padding,background-color,border-color] duration-150 ease-out hover:bg-[var(--bg-hover)] focus-visible:border-[var(--accent-border)] focus-visible:bg-[var(--bg-hover)] focus-visible:ring-2 focus-visible:ring-[var(--accent-muted)]",
        compact ? "h-8 px-2.5" : "h-10 px-3",
        className,
      )}
      style={{
        backgroundColor: "transparent",
        borderColor: "transparent",
        borderStyle: "solid",
        borderWidth: "1px",
      }}
    >
      <ProviderIcon className="h-3.5 w-3.5 shrink-0 text-[var(--text-secondary)]" />
      <span className="truncate text-[0.8125rem] font-medium text-[var(--text-primary)]">
        {visibleLabel ?? modelLabel}
      </span>
      {scopeTag && (
        <span
          className="rounded border px-1 py-0.5 text-[0.5625rem] font-semibold tracking-wide"
          style={{
            color: "var(--status-warning)",
            borderColor: "var(--status-warning-border)",
            backgroundColor: "var(--status-warning-muted)",
          }}
          data-testid="agent-composer-runtime-role-tag"
        >
          {scopeTag}
        </span>
      )}
      {effortOptions.length > 0 && (
        <span
          className="inline-flex shrink-0"
          aria-label={`${effortLabel} effort`}
        >
          <ComposerRuntimeEffortBars
            effortId={effortValue}
            options={effortOptions}
          />
        </span>
      )}
      {fastMode && (
        <Zap
          className="h-3 w-3 shrink-0 fill-current text-[var(--accent-primary)]"
          aria-hidden="true"
        />
      )}
      <ChevronDown className="h-3.5 w-3.5 shrink-0 text-[var(--text-secondary)] transition-transform group-data-[state=open]:rotate-180" />
    </button>
  );
});
