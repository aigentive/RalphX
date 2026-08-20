/**
 * RecentRepositoriesList - shows recently used local repository paths inside
 * ExistingRepositoryStep, before a folder has been selected. Entries whose
 * path already belongs to a registered project are hidden (registration
 * status is read from the already-populated project store, not a new
 * fetch). Selecting an entry feeds the normal debounced probe path - no
 * validation bypass, so a moved/deleted folder degrades to the existing
 * probe error card.
 */

import { useMemo } from "react";
import { History } from "lucide-react";
import { useProjectStore } from "@/stores/projectStore";
import type { RecentRepository } from "@/stores/uiStore";

export interface RecentRepositoriesListProps {
  recents: RecentRepository[];
  onSelect: (path: string) => void;
  disabled?: boolean | undefined;
}

function normalizePath(path: string): string {
  return path.trim().toLowerCase();
}

export function RecentRepositoriesList({
  recents,
  onSelect,
  disabled = false,
}: RecentRepositoriesListProps) {
  const projects = useProjectStore((state) => state.projects);
  const registeredPaths = useMemo(() => {
    const paths = new Set<string>();
    for (const project of Object.values(projects)) {
      paths.add(normalizePath(project.workingDirectory));
    }
    return paths;
  }, [projects]);

  const visibleRecents = recents.filter(
    (entry) => !registeredPaths.has(normalizePath(entry.path))
  );

  if (visibleRecents.length === 0) return null;

  return (
    <div data-testid="recent-repositories-list" className="space-y-1.5">
      <div className="flex items-center gap-1.5 text-xs font-medium text-[var(--text-muted)]">
        <History className="h-3 w-3" />
        Recent
      </div>
      <div className="space-y-1">
        {visibleRecents.map((entry) => (
          <button
            key={entry.path}
            type="button"
            data-testid="recent-repository-item"
            onClick={() => onSelect(entry.path)}
            disabled={disabled}
            className="w-full flex flex-col items-start gap-0.5 px-3 py-2 rounded-lg text-left bg-[var(--bg-base)] hover:bg-[var(--bg-hover)] disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
          >
            <span className="text-sm text-[var(--text-primary)]">{entry.name}</span>
            <span className="text-xs truncate w-full text-[var(--text-muted)]">{entry.path}</span>
          </button>
        ))}
      </div>
    </div>
  );
}
