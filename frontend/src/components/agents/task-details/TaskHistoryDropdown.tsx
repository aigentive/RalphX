import { ChevronDown, History, Loader2 } from "lucide-react";

import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { useTaskStateTransitions } from "@/hooks/useTaskStateTransitions";
import { STATUS_TOKEN_REFS, withAlpha } from "@/lib/theme-colors";
import type { InternalStatus } from "@/types/task";
import type { TaskHistoryState } from "@/types/task-history";

import {
  entryToHistoryState,
  getTaskHistoryEntryKey,
  getTaskHistoryStatusConfig,
  isSelectedTaskHistoryEntry,
  buildTaskHistoryEntries,
  type TaskHistoryEntry,
} from "./taskHistoryEntries";

export interface TaskHistoryDropdownProps {
  taskId: string;
  currentStatus: InternalStatus;
  onStateSelect: (state: TaskHistoryState | null) => void;
  selectedState?: TaskHistoryState | null;
}

const CURRENT_VALUE = "current";

function formatRelativeTime(dateString: string): string {
  const diff = Date.now() - new Date(dateString).getTime();
  const mins = Math.floor(diff / 60000);
  if (mins < 1) return "Just now";
  if (mins < 60) return `${mins}m ago`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours}h ago`;
  return `${Math.floor(hours / 24)}d ago`;
}

function HistoryMenuItem({ entry }: { entry: TaskHistoryEntry }) {
  const config = getTaskHistoryStatusConfig(entry.status);
  const color = config.color === "muted" ? "var(--text-muted)" : STATUS_TOKEN_REFS[config.color];
  const itemLabel = entry.isCurrent ? `Current — ${entry.label}` : entry.label;

  return (
    <DropdownMenuRadioItem
      value={entry.isCurrent ? CURRENT_VALUE : getTaskHistoryEntryKey(entry)}
      data-testid={`task-history-dropdown-item-${getTaskHistoryEntryKey(entry)}`}
      className="min-w-0 items-start gap-2 rounded-md px-3 py-2"
      style={{ color: "var(--text-primary)" }}
    >
      <span
        aria-hidden="true"
        className="mt-1.5 h-1.5 w-1.5 shrink-0 rounded-full"
        style={{ backgroundColor: color }}
      />
      <span className="flex min-w-0 flex-1 flex-col gap-0.5">
        <span className="truncate text-[0.75rem] font-semibold leading-tight">{itemLabel}</span>
        <span
          className="text-[0.625rem] font-medium leading-tight"
          style={{ color: withAlpha("var(--text-primary)", 45) }}
        >
          {entry.isCurrent ? "Live task state" : formatRelativeTime(entry.timestamp)} ·{" "}
          {entry.hasConversation ? "Chat available" : "No chat recorded"}
        </span>
      </span>
    </DropdownMenuRadioItem>
  );
}

export function TaskHistoryDropdown({
  taskId,
  currentStatus,
  onStateSelect,
  selectedState,
}: TaskHistoryDropdownProps) {
  const { data: transitions, isLoading, error } = useTaskStateTransitions(taskId);
  const entries = buildTaskHistoryEntries(transitions, currentStatus);

  if (isLoading) {
    return (
      <div data-testid="task-history-loading" className="flex items-center gap-2 px-4 py-3">
        <Loader2 className="h-4 w-4 animate-spin" style={{ color: "var(--text-muted)" }} />
        <span className="text-[0.6875rem] font-medium text-text-primary/40">Loading history...</span>
      </div>
    );
  }

  if (error) {
    return (
      <div
        data-testid="task-history-error"
        className="flex items-center gap-2 px-4 py-3 text-[0.6875rem] font-medium"
        style={{ color: "var(--status-error)" }}
      >
        <History className="h-4 w-4" />
        <span>Failed to load history</span>
      </div>
    );
  }

  if (entries.length <= 1) {
    return null;
  }

  const currentEntry = entries.find((entry) => entry.isCurrent);
  const selectedEntry = entries.find((entry) => isSelectedTaskHistoryEntry(entry, selectedState));
  const selectedValue = selectedEntry ? getTaskHistoryEntryKey(selectedEntry) : CURRENT_VALUE;
  const orderedEntries = [
    ...entries.filter((entry) => entry.isCurrent),
    ...entries.filter((entry) => !entry.isCurrent).reverse(),
  ];
  const triggerEntry = selectedEntry ?? currentEntry;

  return (
    <div
      data-testid="task-history-dropdown"
      className="flex min-w-0 items-center gap-2 px-4 py-2.5"
      style={{
        backgroundColor: withAlpha("var(--bg-base)", 60),
        borderBottom: "0.5px solid var(--overlay-weak)",
      }}
    >
      <div className="flex shrink-0 items-center gap-2" style={{ color: "var(--text-muted)" }}>
        <History className="h-4 w-4" />
        <span className="text-[0.625rem] font-semibold uppercase tracking-wider">History</span>
      </div>

      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <button
            type="button"
            data-testid="task-history-dropdown-trigger"
            className="flex min-w-0 flex-1 items-center gap-2 rounded-md px-3 py-2 text-left transition-colors hover:bg-[var(--overlay-faint)] focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-[var(--accent-primary)]"
            style={{
              backgroundColor: "var(--bg-elevated)",
              border: "0.5px solid var(--overlay-moderate)",
              color: "var(--text-primary)",
            }}
            aria-label={`Task history: ${triggerEntry?.label ?? "Current"}`}
          >
            <span className="min-w-0 flex-1 truncate text-[0.75rem] font-semibold">
              {triggerEntry?.label ?? "Current"}
            </span>
            <span
              className="shrink-0 text-[0.625rem] font-medium"
              style={{ color: "var(--text-muted)" }}
            >
              {selectedEntry ? "Viewing history" : "Current"}
            </span>
            <ChevronDown aria-hidden="true" className="h-3.5 w-3.5 shrink-0" />
          </button>
        </DropdownMenuTrigger>
        <DropdownMenuContent
          align="start"
          sideOffset={6}
          data-testid="task-history-dropdown-content"
          className="w-[min(22rem,var(--radix-dropdown-menu-trigger-width))] max-h-80 p-1"
          style={{
            backgroundColor: "var(--bg-elevated)",
            border: "0.5px solid var(--overlay-moderate)",
          }}
        >
          <DropdownMenuRadioGroup
            value={selectedValue}
            onValueChange={(value) => {
              if (value === CURRENT_VALUE) {
                onStateSelect(null);
                return;
              }
              const entry = entries.find((candidate) => getTaskHistoryEntryKey(candidate) === value);
              if (entry) {
                onStateSelect(entryToHistoryState(entry));
              }
            }}
          >
            {orderedEntries.map((entry) => (
              <HistoryMenuItem key={getTaskHistoryEntryKey(entry)} entry={entry} />
            ))}
          </DropdownMenuRadioGroup>
        </DropdownMenuContent>
      </DropdownMenu>
    </div>
  );
}
