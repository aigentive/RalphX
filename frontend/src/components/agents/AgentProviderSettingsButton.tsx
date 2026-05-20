import { Settings } from "lucide-react";

import { Button } from "@/components/ui/button";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";

export function AgentProviderSettingsButton({
  onClick,
  testId,
  compact,
}: {
  onClick: () => void;
  testId?: string;
  compact?: boolean;
}) {
  if (compact) {
    return (
      <Tooltip delayDuration={300}>
        <TooltipTrigger asChild>
          <button
            type="button"
            aria-label="Open Provider Settings"
            className="flex aspect-square w-full cursor-pointer items-center justify-center rounded transition-colors hover:bg-[var(--bg-hover)]"
            style={{ color: "var(--text-muted)" }}
            onClick={onClick}
            data-testid={testId}
          >
            <Settings className="h-4 w-4" />
          </button>
        </TooltipTrigger>
        <TooltipContent side="right" className="text-xs">
          Provider Settings
        </TooltipContent>
      </Tooltip>
    );
  }

  return (
    <Button
      type="button"
      variant="ghost"
      className="h-8 w-full justify-start rounded-md px-2 text-[0.75rem]"
      style={{ color: "var(--text-secondary)" }}
      onClick={onClick}
      data-testid={testId}
    >
      <Settings className="h-3.5 w-3.5" />
      Open Provider Settings
    </Button>
  );
}
