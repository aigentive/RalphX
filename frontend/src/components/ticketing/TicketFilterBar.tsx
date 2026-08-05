import { Columns3, Eye, List, RefreshCw, Search, X } from "lucide-react";

import type { TicketingContainer, TicketingColumn } from "@/api/ticketing";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import type { TicketingFilterState, TicketingViewMode } from "@/stores/ticketingStore";
import { cn } from "@/lib/utils";

import { TicketSearchableMultiSelect } from "./TicketSearchableMultiSelect";
import { TicketSearchableSelect } from "./TicketSearchableSelect";
import { TicketStatusGlyph } from "./TicketStatusGlyph";
import { hasActiveTicketFilters, UNASSIGNED_ASSIGNEE } from "./ticketing-read-state";
import { statusColor } from "./ticketing-status-presentation";

interface TicketFilterBarProps {
  refreshDisabled?: boolean | undefined;
  containers: TicketingContainer[];
  columns: TicketingColumn[];
  assigneeOptions: string[];
  sprintOptions?: string[];
  showWatcherFilter?: boolean;
  containerLabel: string;
  allContainersLabel: string;
  activeContainerId: string | null;
  /** When true, a container must be selected before tickets load — shows a `*`. */
  containerSelectionNeeded?: boolean;
  filters: TicketingFilterState;
  viewMode: TicketingViewMode;
  isRefreshing: boolean;
  onContainerChange: (containerId: string | null) => void;
  onFiltersChange: (filters: Partial<TicketingFilterState>) => void;
  onResetFilters: () => void;
  onViewModeChange: (mode: TicketingViewMode) => void;
  onRefresh: () => void;
}

function ViewModeButton({
  mode,
  activeMode,
  icon: Icon,
  label,
  onClick,
}: {
  mode: TicketingViewMode;
  activeMode: TicketingViewMode;
  icon: React.ElementType;
  label: string;
  onClick: (mode: TicketingViewMode) => void;
}) {
  const isActive = activeMode === mode;
  return (
    <button
      type="button"
      aria-label={`${label} view`}
      aria-pressed={isActive}
      className={cn(
        "inline-flex h-8 items-center gap-1.5 rounded px-3 text-xs font-medium",
        "focus-visible:outline-none focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:2px]",
      )}
      style={{
        backgroundColor: isActive ? "var(--bg-elevated)" : "transparent",
        color: isActive ? "var(--text-primary)" : "var(--text-muted)",
      }}
      onClick={() => onClick(mode)}
    >
      <Icon className="h-3.5 w-3.5" aria-hidden="true" />
      {label}
    </button>
  );
}

export function TicketFilterBar({
  refreshDisabled = false,
  containers,
  columns,
  assigneeOptions,
  sprintOptions = [],
  showWatcherFilter = false,
  containerLabel,
  allContainersLabel,
  activeContainerId,
  containerSelectionNeeded = false,
  filters,
  viewMode,
  isRefreshing,
  onContainerChange,
  onFiltersChange,
  onResetFilters,
  onViewModeChange,
  onRefresh,
}: TicketFilterBarProps) {
  const filtersActive = hasActiveTicketFilters(filters);
  return (
    <div
      className="flex flex-wrap items-center gap-2 px-4 py-3"
      style={{
        backgroundColor: "var(--bg-surface)",
        borderBottomColor: "var(--border-subtle)",
        borderBottomStyle: "solid",
        borderBottomWidth: "1px",
      }}
    >
      <label className="flex min-w-[180px] items-center gap-2 text-xs text-[var(--text-muted)]">
        <span>
          {containerLabel}
          {containerSelectionNeeded ? (
            <span aria-hidden="true" className="ml-0.5 text-[var(--accent-primary)]">*</span>
          ) : null}
        </span>
        <TicketSearchableSelect
          ariaLabel={containerLabel}
          className="min-w-[150px] max-w-[220px]"
          value={activeContainerId ?? ""}
          onValueChange={(nextValue) => onContainerChange(nextValue || null)}
          placeholder={allContainersLabel}
          searchPlaceholder={`Search ${containerLabel.toLowerCase()}s...`}
          clearable
          clearLabel={`Clear ${containerLabel.toLowerCase()} filter`}
          options={[
            { value: "", label: allContainersLabel },
            ...containers.map((container) => ({
              value: container.id,
              label: container.name,
              description: container.key ?? undefined,
            })),
          ]}
        />
      </label>

      <label className="flex min-w-[170px] items-center gap-2 text-xs text-[var(--text-muted)]">
        Status
        <TicketSearchableSelect
          ariaLabel="Status"
          className="min-w-[120px] max-w-[200px]"
          value={filters.stateIds[0] ?? ""}
          onValueChange={(nextValue) =>
            onFiltersChange({
              stateIds: nextValue ? [nextValue] : [],
            })
          }
          placeholder="Any status"
          searchPlaceholder="Search statuses..."
          clearable
          clearLabel="Clear status filter"
          options={[
            { value: "", label: "Any status" },
            ...columns.map((column) => ({
              value: column.id,
              label: column.name,
              leadingIcon: (
                <span className="inline-flex" style={{ color: statusColor(column) }}>
                  <TicketStatusGlyph category={column.category} className="h-4 w-4" />
                </span>
              ),
            })),
          ]}
        />
      </label>

      <label className="flex items-center gap-2 text-xs text-[var(--text-muted)]">
        Assignee
        <TicketSearchableMultiSelect
          ariaLabel="Assignee"
          className="min-w-[130px] max-w-[200px]"
          selectedValues={filters.assignees}
          onSelectedValuesChange={(assignees) => onFiltersChange({ assignees })}
          placeholder="Everyone"
          searchPlaceholder="Search assignees..."
          clearLabel="Clear assignee filter"
          options={[
            { value: UNASSIGNED_ASSIGNEE, label: "Unassigned" },
            ...assigneeOptions.map((name) => ({ value: name, label: name })),
          ]}
        />
      </label>

      {sprintOptions.length > 0 && (
        <label className="flex items-center gap-2 text-xs text-[var(--text-muted)]">
          Sprint
          <TicketSearchableSelect
            ariaLabel="Sprint"
            className="min-w-[150px] max-w-[220px]"
            value={filters.sprint ?? ""}
            onValueChange={(nextValue) => onFiltersChange({ sprint: nextValue || null })}
            placeholder="All sprints"
            searchPlaceholder="Search sprints..."
            clearable
            clearLabel="Clear sprint filter"
            options={[
              { value: "", label: "All sprints" },
              ...sprintOptions.map((name) => ({ value: name, label: name })),
            ]}
          />
        </label>
      )}

      {showWatcherFilter && (
        <button
          type="button"
          aria-pressed={filters.watcherMe}
          className={cn(
            "inline-flex h-8 min-w-[124px] items-center justify-center gap-1.5 rounded px-3 text-xs font-medium",
            "focus-visible:outline-none focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:2px]",
          )}
          style={{
            backgroundColor: filters.watcherMe ? "var(--bg-elevated)" : "transparent",
            borderColor: filters.watcherMe ? "var(--accent-primary)" : "var(--border-default)",
            borderStyle: "solid",
            borderWidth: "1px",
            color: filters.watcherMe ? "var(--text-primary)" : "var(--text-muted)",
          }}
          onClick={() => onFiltersChange({ watcherMe: !filters.watcherMe })}
        >
          <Eye className="h-3.5 w-3.5" aria-hidden="true" />
          Watched by me
        </button>
      )}

      <div className="relative min-w-[220px] flex-1">
        <Search
          className="pointer-events-none absolute left-2.5 top-1/2 h-4 w-4 -translate-y-1/2 text-[var(--text-muted)]"
          aria-hidden="true"
        />
        <Input
          value={filters.text}
          onChange={(event) => onFiltersChange({ text: event.target.value })}
          placeholder="Search tickets"
          aria-label="Search tickets"
          className="h-8 pl-8 text-sm"
          style={{
            backgroundColor: "var(--bg-elevated)",
            borderColor: "var(--border-default)",
            borderStyle: "solid",
            borderWidth: "1px",
            color: "var(--text-primary)",
          }}
        />
      </div>

      {filtersActive && (
        <Button
          type="button"
          variant="outline"
          size="sm"
          className="h-8 gap-1.5"
          onClick={onResetFilters}
          style={{
            backgroundColor: "var(--bg-elevated)",
            borderColor: "var(--border-default)",
            borderStyle: "solid",
            borderWidth: "1px",
            color: "var(--text-secondary)",
          }}
        >
          <X className="h-3.5 w-3.5" aria-hidden="true" />
          Reset
        </Button>
      )}

      <div
        className="inline-flex h-9 items-center rounded-md p-0.5"
        style={{
          backgroundColor: "var(--bg-sunken)",
          borderColor: "var(--border-subtle)",
          borderStyle: "solid",
          borderWidth: "1px",
        }}
      >
        <ViewModeButton
          mode="list"
          activeMode={viewMode}
          icon={List}
          label="List"
          onClick={onViewModeChange}
        />
        <ViewModeButton
          mode="kanban"
          activeMode={viewMode}
          icon={Columns3}
          label="Kanban"
          onClick={onViewModeChange}
        />
      </div>

      <Tooltip>
        <TooltipTrigger asChild>
          <button
            type="button"
            aria-label="Refresh tickets"
            className="grid h-9 w-9 place-items-center rounded-md text-[var(--text-secondary)] hover:text-[var(--text-primary)] focus-visible:outline-none focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:2px]"
          disabled={isRefreshing || refreshDisabled}
            onClick={onRefresh}
            style={{
              backgroundColor: "var(--bg-elevated)",
              borderColor: "var(--border-default)",
              borderStyle: "solid",
              borderWidth: "1px",
            }}
          >
            <RefreshCw className={cn("h-4 w-4", isRefreshing && "animate-spin")} aria-hidden="true" />
          </button>
        </TooltipTrigger>
        <TooltipContent>Refresh tickets</TooltipContent>
      </Tooltip>
    </div>
  );
}
