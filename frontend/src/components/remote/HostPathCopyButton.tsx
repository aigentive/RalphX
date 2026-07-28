/**
 * The honest replacement for "open this path" when the path lives on another Mac
 * (PR 2.6-a).
 *
 * A remote client cannot open, reveal, or preview a host file — but the PATH is
 * still useful: it is what the user types into a terminal on the host, or pastes
 * back into the chat. So the affordance degrades to a copy action rather than
 * disappearing, and the tooltip says where the file actually is instead of leaving
 * the user to discover it by clicking something that does nothing.
 *
 * Icon-only, so rule 23 applies: an `aria-label` AND the app Tooltip component.
 */

import { Copy } from "lucide-react";
import { toast } from "sonner";

import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import {
  HOST_PATH_COPY_HINT,
  HOST_PATH_COPY_LABEL,
} from "@/lib/remote/host-affordances";

export interface HostPathCopyButtonProps {
  path: string;
  testId?: string;
}

export function HostPathCopyButton({ path, testId }: HostPathCopyButtonProps) {
  const copyPath = async (): Promise<void> => {
    try {
      if (!navigator.clipboard) {
        throw new Error("clipboard unavailable");
      }
      await navigator.clipboard.writeText(path);
      toast.success("Path copied");
    } catch {
      toast.error("Failed to copy path");
    }
  };

  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <button
          type="button"
          className="inline-flex h-4 w-4 shrink-0 items-center justify-center rounded-sm p-0 align-baseline transition-colors hover:bg-[var(--overlay-faint)]"
          style={{ color: "var(--accent-primary)" }}
          aria-label={HOST_PATH_COPY_LABEL}
          onClick={() => void copyPath()}
          {...(testId ? { "data-testid": testId } : {})}
        >
          <Copy className="h-3 w-3" aria-hidden="true" />
        </button>
      </TooltipTrigger>
      <TooltipContent side="top" className="text-xs">
        {HOST_PATH_COPY_HINT}
      </TooltipContent>
    </Tooltip>
  );
}
