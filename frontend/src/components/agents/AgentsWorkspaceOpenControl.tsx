import { memo, useEffect, useMemo, useState } from "react";
import {
  Check,
  ChevronDown,
  Code2,
  FolderOpen,
  Loader2,
  Terminal as TerminalIcon,
} from "lucide-react";

import type { WorkspaceOpenTarget, WorkspaceOpenTargetKind } from "@/api/chat";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import {
  readPreferredWorkspaceOpenTargetId,
  resolvePreferredWorkspaceOpenTarget,
  subscribePreferredWorkspaceOpenTargetId,
  writePreferredWorkspaceOpenTargetId,
} from "@/lib/workspace-open-targets";

interface AgentsWorkspaceOpenControlProps {
  targets: readonly WorkspaceOpenTarget[];
  openingTargetId?: string | null;
  onOpenTarget: (targetId: string) => void;
  builtInTerminal?: {
    open: boolean;
    unavailableReason: string | null;
    onToggle: (() => void) | undefined;
    onPreload: (() => void) | undefined;
  };
}

function iconForTargetKind(kind: WorkspaceOpenTargetKind) {
  switch (kind) {
    case "terminal":
      return TerminalIcon;
    case "fileManager":
      return FolderOpen;
    case "editor":
      return Code2;
  }
}

export const AgentsWorkspaceOpenControl = memo(function AgentsWorkspaceOpenControl({
  targets,
  openingTargetId = null,
  onOpenTarget,
  builtInTerminal,
}: AgentsWorkspaceOpenControlProps) {
  const [preferredTargetId, setPreferredTargetId] = useState(
    readPreferredWorkspaceOpenTargetId,
  );
  const preferredTarget = useMemo(
    () => resolvePreferredWorkspaceOpenTarget(targets, preferredTargetId),
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

  useEffect(
    () => subscribePreferredWorkspaceOpenTargetId(setPreferredTargetId),
    [],
  );

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
    writePreferredWorkspaceOpenTargetId(target.id);
    onOpenTarget(target.id);
  };
  const PreferredIcon = iconForTargetKind(displayedTarget.kind);
  const isOpening = openingTargetId !== null;
  const builtInTerminalDisabled =
    !builtInTerminal?.onToggle || Boolean(builtInTerminal.unavailableReason);
  const builtInTerminalPreload = builtInTerminalDisabled
    ? undefined
    : builtInTerminal?.onPreload;

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
          {builtInTerminal ? (
            <>
              <DropdownMenuItem
                disabled={builtInTerminalDisabled}
                onClick={
                  builtInTerminalDisabled ? undefined : builtInTerminal.onToggle
                }
                onPointerEnter={builtInTerminalPreload}
                onFocus={builtInTerminalPreload}
                aria-label={
                  builtInTerminal.unavailableReason
                    ? `Built-in Terminal unavailable: ${builtInTerminal.unavailableReason}`
                    : "Built-in Terminal"
                }
                title={builtInTerminal.unavailableReason ?? undefined}
              >
                <TerminalIcon className="h-4 w-4" />
                <span>Built-in Terminal</span>
                {builtInTerminal.open ? (
                  <Check
                    className="ml-auto h-3.5 w-3.5"
                    data-testid="agents-built-in-terminal-open-indicator"
                  />
                ) : null}
              </DropdownMenuItem>
              <DropdownMenuSeparator />
            </>
          ) : null}
          {targets.map((target) => {
            const Icon = iconForTargetKind(target.kind);
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
