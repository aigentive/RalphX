import { Search } from "lucide-react";

import type { AutomationListFilter } from "./automationListPresentation";

const FILTER_LABELS: Array<{ id: AutomationListFilter; label: string }> = [
  { id: "all", label: "All" },
  { id: "attention", label: "Needs attention" },
  { id: "running", label: "Running" },
  { id: "finished", label: "Finished" },
  { id: "drafts", label: "Drafts" },
];

interface AutomationListToolbarProps {
  activeFilter: AutomationListFilter;
  counts: Record<AutomationListFilter, number>;
  searchText: string;
  onFilterChange: (filter: AutomationListFilter) => void;
  onSearchTextChange: (value: string) => void;
}

export function AutomationListToolbar({
  activeFilter,
  counts,
  searchText,
  onFilterChange,
  onSearchTextChange,
}: AutomationListToolbarProps) {
  return (
    <div
      className="flex flex-col gap-3 lg:flex-row lg:items-center lg:justify-between"
      data-testid="automations-list-toolbar"
    >
      <div className="flex flex-wrap gap-2" role="group" aria-label="Automation filters">
        {FILTER_LABELS.map((filter) => {
          const selected = activeFilter === filter.id;
          return (
            <button
              key={filter.id}
              type="button"
              className="rounded-full px-3 py-1.5 text-xs font-semibold transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent-primary)]"
              style={{
                backgroundColor: selected
                  ? "var(--accent-muted, #3a2a22)"
                  : "var(--bg-surface, #1e1e23)",
                borderColor: selected
                  ? "var(--accent-border, #59392a)"
                  : "var(--border-subtle, #2e2e36)",
                borderStyle: "solid",
                borderWidth: "1px",
                color: selected ? "var(--accent-primary, #ff6b35)" : "var(--text-secondary, #c7c7cc)",
              }}
              aria-pressed={selected}
              onClick={() => onFilterChange(filter.id)}
              data-testid={`automations-filter-${filter.id}`}
            >
              {filter.label} <span className="ml-1 tabular-nums opacity-70">{counts[filter.id]}</span>
            </button>
          );
        })}
      </div>
      <label
        className="flex h-9 w-full items-center gap-2 rounded-md px-3 lg:max-w-64"
        style={{
          backgroundColor: "var(--bg-surface, #1e1e23)",
          borderColor: "var(--border-subtle, #2e2e36)",
          borderStyle: "solid",
          borderWidth: "1px",
          color: "var(--text-muted, #8e8e96)",
        }}
      >
        <Search className="h-4 w-4 shrink-0" aria-hidden="true" />
        <span className="sr-only">Search automations</span>
        <input
          type="search"
          className="min-w-0 flex-1 bg-transparent text-sm outline-none placeholder:text-[var(--text-subtle)]"
          style={{ color: "var(--text-primary, #f2f2f4)" }}
          value={searchText}
          onChange={(event) => onSearchTextChange(event.target.value)}
          placeholder="Search automations…"
          data-testid="automations-search"
        />
      </label>
    </div>
  );
}
