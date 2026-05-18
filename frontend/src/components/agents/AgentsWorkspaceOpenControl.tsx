import { memo, useEffect, useMemo, useState } from "react";
import { Check, ChevronDown, Code2, FolderOpen, Loader2 } from "lucide-react";

import type { WorkspaceOpenTarget } from "@/api/chat";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";

const PREFERRED_WORKSPACE_OPEN_TARGET_KEY =
  "ralphx:agents:preferred-workspace-open-target";

interface AgentsWorkspaceOpenControlProps {
  targets: readonly WorkspaceOpenTarget[];
  openingTargetId?: string | null;
  onOpenTarget: (targetId: string) => void;
}

function readPreferredTargetId(): string | null {
  if (typeof window === "undefined") {
    return null;
  }
  return window.localStorage.getItem(PREFERRED_WORKSPACE_OPEN_TARGET_KEY);
}

function writePreferredTargetId(targetId: string): void {
  if (typeof window === "undefined") {
    return;
  }
  window.localStorage.setItem(PREFERRED_WORKSPACE_OPEN_TARGET_KEY, targetId);
}

export const AgentsWorkspaceOpenControl = memo(function AgentsWorkspaceOpenControl({
  targets,
  openingTargetId = null,
  onOpenTarget,
}: AgentsWorkspaceOpenControlProps) {
  const [preferredTargetId, setPreferredTargetId] = useState(readPreferredTargetId);
  const preferredTarget = useMemo(
    () =>
      targets.find((target) => target.id === preferredTargetId) ??
      targets[0] ??
      null,
    [preferredTargetId, targets],
  );
  const openingTarget = useMemo(
    () =>
      openingTargetId
        ? targets.find((target) => target.id === openingTargetId) ?? null
        : null,
    [openingTargetId, targets],
  );
  const displayedTarget = openingTarget ?? preferredTarget;

  useEffect(() => {
    if (
      targets.length > 0 &&
      preferredTargetId &&
      !targets.some((target) => target.id === preferredTargetId)
    ) {
      setPreferredTargetId(targets[0]?.id ?? null);
    }
  }, [preferredTargetId, targets]);

  if (!preferredTarget || !displayedTarget) {
    return null;
  }

  const openTarget = (target: WorkspaceOpenTarget) => {
    setPreferredTargetId(target.id);
    writePreferredTargetId(target.id);
    onOpenTarget(target.id);
  };
  const PreferredIcon =
    displayedTarget.kind === "fileManager" ? FolderOpen : Code2;
  const isOpening = openingTargetId !== null;

  return (
    <div className="inline-flex shrink-0 items-center">
      <Button
        type="button"
        variant="ghost"
        size="sm"
        className="h-8 min-w-[5.75rem] gap-2 rounded-r-none px-2.5 py-0 text-xs"
        onClick={() => openTarget(preferredTarget)}
        disabled={isOpening}
        aria-busy={isOpening ? "true" : undefined}
        aria-label={`Open workspace in ${displayedTarget.label}`}
        data-testid="agents-open-workspace"
      >
        {isOpening ? (
          <Loader2 className="h-3.5 w-3.5 shrink-0 animate-spin" />
        ) : (
          <PreferredIcon className="h-3.5 w-3.5 shrink-0" />
        )}
        <span className="flex min-w-0 flex-col items-start justify-center leading-none">
          <span className="text-[0.75rem] leading-[0.85rem]">
            {isOpening ? "Opening" : "Open"}
          </span>
          <span
            className="max-w-[5.25rem] truncate text-[0.625rem] font-normal leading-[0.72rem]"
            style={{ color: "var(--text-muted)" }}
            data-testid="agents-open-workspace-current-target"
          >
            {displayedTarget.label}
          </span>
        </span>
      </Button>
      <DropdownMenu>
        <Tooltip>
          <DropdownMenuTrigger asChild>
            <TooltipTrigger asChild>
              <Button
                type="button"
                variant="ghost"
                size="sm"
                className="h-8 w-7 rounded-l-none border-l px-0"
                disabled={isOpening}
                aria-label="Open workspace options"
                data-testid="agents-open-workspace-options"
                style={{ borderColor: "var(--border-subtle)" }}
              >
                <ChevronDown className="h-3.5 w-3.5" />
              </Button>
            </TooltipTrigger>
          </DropdownMenuTrigger>
          <TooltipContent side="bottom" className="text-xs">
            Open workspace options
          </TooltipContent>
        </Tooltip>
        <DropdownMenuContent align="end" className="min-w-[180px]">
          {targets.map((target) => {
            const Icon = target.kind === "fileManager" ? FolderOpen : Code2;
            const selected = target.id === preferredTarget.id;
            return (
              <DropdownMenuItem
                key={target.id}
                onClick={() => openTarget(target)}
              >
                <Icon className="h-4 w-4" />
                <span>{target.label}</span>
                {selected ? <Check className="ml-auto h-3.5 w-3.5" /> : null}
              </DropdownMenuItem>
            );
          })}
        </DropdownMenuContent>
      </DropdownMenu>
    </div>
  );
});
