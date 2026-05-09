/**
 * ActivityFilters - Filter components for ActivityView
 */

import { useCallback } from "react";
import {
  Search,
  X,
  ChevronDown,
  History,
  Radio,
} from "lucide-react";

// Re-export TaskFilter from dedicated module
export { TaskFilter } from "./TaskFilter";
export type { TaskFilterProps } from "./TaskFilter";

// Re-export SessionFilter from dedicated module
export { SessionFilter } from "./SessionFilter";
export type { SessionFilterProps } from "./SessionFilter";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuCheckboxItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { cn } from "@/lib/utils";
import type { ViewMode, MessageTypeFilter, RoleFilterValue } from "./ActivityView.types";
import { MESSAGE_TYPES, STATUS_OPTIONS, ROLE_OPTIONS } from "./ActivityView.types";

// ============================================================================
// ViewModeToggle
// ============================================================================

export interface ViewModeToggleProps {
  mode: ViewMode;
  onChange: (mode: ViewMode) => void;
  disabled?: boolean;
  /** Whether Live mode is actively receiving events (triggers pulsating animation) */
  isReceiving?: boolean;
}

export function ViewModeToggle({
  mode,
  onChange,
  disabled,
  isReceiving,
}: ViewModeToggleProps) {
  const showPulsating = mode === "realtime" && isReceiving;

  return (
    <div className="flex gap-1 p-1 rounded-lg bg-[var(--bg-hover)]">
      <button
        data-testid="activity-mode-realtime"
        onClick={() => onChange("realtime")}
        disabled={disabled}
        className={cn(
          "flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium rounded-md transition-colors relative",
          mode === "realtime"
            ? "bg-[var(--bg-elevated)] text-[var(--text-primary)] shadow-[var(--shadow-xs)]"
            : "text-[var(--text-secondary)] hover:text-[var(--text-primary)]",
          disabled && "opacity-50 cursor-not-allowed",
          showPulsating && "live-receiving"
        )}
      >
        <Radio className={cn("w-3 h-3", showPulsating && "text-[var(--accent-primary)]")} />
        Live
        {showPulsating && (
          <span className="absolute -top-0.5 -right-0.5 w-2 h-2 rounded-full bg-[var(--accent-primary)] live-pulse-dot" />
        )}
      </button>
      <button
        data-testid="activity-mode-historical"
        onClick={() => onChange("historical")}
        disabled={disabled}
        className={cn(
          "flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium rounded-md transition-colors",
          mode === "historical"
            ? "bg-[var(--bg-elevated)] text-[var(--text-primary)] shadow-[var(--shadow-xs)]"
            : "text-[var(--text-secondary)] hover:text-[var(--text-primary)]",
          disabled && "opacity-50 cursor-not-allowed"
        )}
      >
        <History className="w-3 h-3" />
        History
      </button>
    </div>
  );
}

// ============================================================================
// StatusFilter
// ============================================================================

export interface StatusFilterProps {
  selectedStatuses: string[];
  onChange: (statuses: string[]) => void;
}

export function StatusFilter({
  selectedStatuses,
  onChange,
}: StatusFilterProps) {
  const handleToggle = useCallback((status: string) => {
    if (selectedStatuses.includes(status)) {
      onChange(selectedStatuses.filter((s) => s !== status));
    } else {
      onChange([...selectedStatuses, status]);
    }
  }, [selectedStatuses, onChange]);

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          variant="outline"
          size="sm"
          className="h-8 text-xs gap-1.5 bg-[var(--bg-elevated)] border-[var(--border-default)] hover:bg-[var(--bg-hover)]"
        >
          Status
          {selectedStatuses.length > 0 && (
            <span className="px-1.5 py-0.5 rounded-full bg-[var(--accent-primary)] text-white text-[0.625rem]">
              {selectedStatuses.length}
            </span>
          )}
          <ChevronDown className="w-3 h-3 ml-1" />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" className="w-40">
        {STATUS_OPTIONS.map(({ value, label }) => (
          <DropdownMenuCheckboxItem
            key={value}
            checked={selectedStatuses.includes(value)}
            onCheckedChange={() => handleToggle(value)}
          >
            {label}
          </DropdownMenuCheckboxItem>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

// ============================================================================
// RoleFilter
// ============================================================================

export interface RoleFilterProps {
  selectedRoles: RoleFilterValue[];
  onChange: (roles: RoleFilterValue[]) => void;
}

export function RoleFilter({
  selectedRoles,
  onChange,
}: RoleFilterProps) {
  const handleToggle = useCallback((role: RoleFilterValue) => {
    if (selectedRoles.includes(role)) {
      onChange(selectedRoles.filter((r) => r !== role));
    } else {
      onChange([...selectedRoles, role]);
    }
  }, [selectedRoles, onChange]);

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          variant="outline"
          size="sm"
          className="h-8 text-xs gap-1.5 bg-[var(--bg-elevated)] border-[var(--border-default)] hover:bg-[var(--bg-hover)]"
        >
          Role
          {selectedRoles.length > 0 && (
            <span className="px-1.5 py-0.5 rounded-full bg-[var(--accent-primary)] text-white text-[0.625rem]">
              {selectedRoles.length}
            </span>
          )}
          <ChevronDown className="w-3 h-3 ml-1" />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" className="w-32">
        {ROLE_OPTIONS.map(({ value, label }) => (
          <DropdownMenuCheckboxItem
            key={value}
            checked={selectedRoles.includes(value)}
            onCheckedChange={() => handleToggle(value)}
          >
            {label}
          </DropdownMenuCheckboxItem>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

// ============================================================================
// FilterTabs
// ============================================================================

export interface FilterTabsProps {
  active: MessageTypeFilter;
  onChange: (filter: MessageTypeFilter) => void;
}

export function FilterTabs({
  active,
  onChange,
}: FilterTabsProps) {
  return (
    <div className="flex gap-1 p-1 rounded-lg bg-[var(--bg-base)] overflow-x-auto">
      {MESSAGE_TYPES.map(({ key, label }) => {
        const isActive = active === key;
        return (
          <button
            key={key}
            role="tab"
            data-active={isActive ? "true" : "false"}
            onClick={() => onChange(key)}
            className={cn(
              "px-3 py-1.5 text-xs font-medium rounded-md transition-colors whitespace-nowrap",
              isActive
                ? "bg-[var(--bg-elevated)] text-[var(--text-primary)] border border-[var(--border-subtle)]"
                : "text-[var(--text-secondary)] hover:text-[var(--text-primary)] border border-transparent"
            )}
          >
            {label}
          </button>
        );
      })}
    </div>
  );
}

// ============================================================================
// SearchBar
// ============================================================================

export interface SearchBarProps {
  value: string;
  onChange: (value: string) => void;
  onClear: () => void;
}

export function SearchBar({
  value,
  onChange,
  onClear,
}: SearchBarProps) {
  return (
    <div className="relative">
      <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-[var(--text-muted)]" />
      <Input
        type="text"
        data-testid="activity-search"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder="Search activities..."
        className="pl-10 pr-8 h-9 bg-[var(--bg-elevated)] border-[var(--border-default)] focus:border-[var(--accent-primary)] focus:ring-1 focus:ring-[var(--accent-primary)]/30"
      />
      {value && (
        <button
          onClick={onClear}
          className="absolute right-2 top-1/2 -translate-y-1/2 p-1 rounded hover:bg-[var(--overlay-faint)] text-[var(--text-muted)]"
          aria-label="Clear search"
        >
          <X className="w-4 h-4" />
        </button>
      )}
    </div>
  );
}

// ============================================================================
// EmptyState
// ============================================================================

export interface EmptyStateProps {
  hasFilter: boolean;
  /** Whether we're in Live mode (shows History hint when empty) */
  isLiveMode?: boolean;
}

function ActivityEmptyIcon() {
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      width="48"
      height="48"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      strokeDasharray="4 4"
      className="text-[var(--text-muted)]"
    >
      <path d="M22 12h-2.48a2 2 0 0 0-1.93 1.46l-2.35 8.36a.25.25 0 0 1-.48 0L9.24 2.18a.25.25 0 0 0-.48 0l-2.35 8.36A2 2 0 0 1 4.49 12H2" />
    </svg>
  );
}

export function EmptyState({ hasFilter, isLiveMode }: EmptyStateProps) {
  // Determine the message based on context
  const getMessage = () => {
    if (hasFilter) {
      return {
        title: "No matching activities",
        subtitle: "Try adjusting your search or filters",
      };
    }

    if (isLiveMode) {
      return {
        title: "No live activity",
        subtitle: "Agent activity will appear here when tasks are running",
        hint: (
          <span className="flex items-center justify-center gap-1.5 mt-3 text-sm text-[var(--text-secondary)]">
            <History className="w-4 h-4" />
            Switch to <strong className="text-[var(--accent-primary)]">History</strong> to browse past events
          </span>
        ),
      };
    }

    return {
      title: "No activity yet",
      subtitle: "Agent activity will appear here when tasks are running",
    };
  };

  const { title, subtitle, hint } = getMessage();

  return (
    <div
      data-testid="activity-empty"
      className="flex flex-col items-center justify-center h-full p-8 text-center"
    >
      <div className="mb-4 opacity-50">
        <ActivityEmptyIcon />
      </div>
      <p className="text-[var(--text-secondary)]">{title}</p>
      <p className="text-sm text-[var(--text-muted)] mt-1">{subtitle}</p>
      {hint}
    </div>
  );
}
