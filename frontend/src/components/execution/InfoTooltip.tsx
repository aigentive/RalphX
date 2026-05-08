/**
 * InfoTooltip - Educational info icon with rich tooltip content
 *
 * Displays an ⓘ icon that shows contextual help on hover.
 * Used in execution control bar to explain Running/Queued/Merging sections.
 */

import { Info } from "lucide-react";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import type { ReactNode } from "react";

interface InfoTooltipProps {
  /** Tooltip content (can be text or rich JSX) */
  content: ReactNode;
  /** Optional test ID for the trigger button */
  testId?: string;
}

export function InfoTooltip({ content, testId }: InfoTooltipProps) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <button
          data-testid={testId}
          type="button"
          aria-label="More information"
          className="inline-flex items-center justify-center transition-colors duration-150 hover:opacity-70 cursor-help"
          style={{
            color: "var(--text-muted)",
            background: "none",
            border: "none",
            padding: 0,
            margin: 0,
          }}
        >
          <Info className="w-3.5 h-3.5" />
        </button>
      </TooltipTrigger>
      <TooltipContent
        side="top"
        align="center"
        className="max-w-[320px] p-3 text-[0.8125rem] leading-relaxed"
        style={{
          backgroundColor: "var(--bg-surface)",
          border: "1px solid var(--overlay-weak)",
          color: "var(--text-primary)",
        }}
      >
        {content}
      </TooltipContent>
    </Tooltip>
  );
}
