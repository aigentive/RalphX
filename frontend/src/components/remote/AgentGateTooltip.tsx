/**
 * Wraps a gated control so the disabled state comes with its explanation (2.6-b).
 *
 * A disabled button with no reason is worse than a hidden one — the user retries,
 * assumes a bug, and files it. This renders the control untouched when the gate is
 * open (zero DOM change, zero tooltip subscription on the local path) and adds the
 * hint only when it is closed.
 *
 * The `<span>` wrapper is required: a disabled button fires no pointer events, so
 * Radix would never see the hover that opens the tooltip.
 */

import type { ReactNode } from "react";

import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";

export interface AgentGateTooltipProps {
  gated: boolean;
  reason: string | null;
  children: ReactNode;
  /** Layout passthrough for the wrapper span; only used when gated. */
  className?: string;
  testId?: string;
}

export function AgentGateTooltip({
  gated,
  reason,
  children,
  className,
  testId,
}: AgentGateTooltipProps) {
  if (!gated || reason === null) {
    return <>{children}</>;
  }

  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <span
          className={className ?? "inline-flex"}
          data-testid={testId ?? "agent-gate-tooltip"}
          data-agent-gated="true"
        >
          {children}
        </span>
      </TooltipTrigger>
      <TooltipContent side="top" className="max-w-[280px] text-xs">
        {reason}
      </TooltipContent>
    </Tooltip>
  );
}
