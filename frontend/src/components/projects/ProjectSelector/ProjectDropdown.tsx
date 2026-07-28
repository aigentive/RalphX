import { useEffect, useMemo, useState } from "react";
import { Check, ChevronDown, FolderOpen, Plus, Search } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import { cn } from "@/lib/utils";

export interface ProjectDropdownProject {
  id: string;
  name: string;
  workingDirectory?: string | null;
}

type ProjectDropdownVariant = "navbar" | "insights";

export interface ProjectDropdownProps {
  projects: ProjectDropdownProject[];
  value: string | null;
  onValueChange: (projectId: string | null) => void;
  selectedProject?: ProjectDropdownProject | null;
  includeAllProjects?: boolean;
  allProjectsLabel?: string;
  allProjectsDescription?: string;
  placeholder?: string;
  onNewProject?: () => void;
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
  className?: string;
  align?: "start" | "center" | "end";
  variant?: ProjectDropdownVariant;
  pageSize?: number;
  testId?: string;
  dropdownTestId?: string;
  listTestId?: string;
  searchTestId?: string;
  newProjectTestId?: string;
  showMoreTestId?: string;
  projectOptionTestId?: (project: ProjectDropdownProject) => string;
  allProjectsTestId?: string;
}

function matchesQuery(label: string, description: string | null | undefined, query: string) {
  if (!query) return true;
  const haystack = `${label} ${description ?? ""}`.toLowerCase();
  return haystack.includes(query);
}

function ProjectOption({
  label,
  description,
  isSelected,
  onSelect,
  testId,
}: {
  label: string;
  description?: string | null;
  isSelected: boolean;
  onSelect: () => void;
  testId?: string;
}) {
  return (
    <button
      type="button"
      role="option"
      aria-selected={isSelected}
      className={cn(
        "flex w-full min-w-0 items-start gap-2 rounded-md px-2 py-2 text-left text-xs transition-colors",
        isSelected
          ? "bg-[var(--accent-muted)] text-[var(--accent-primary)]"
          : "text-[var(--text-primary)] hover:bg-[var(--bg-hover)]"
      )}
      onClick={onSelect}
      data-testid={testId}
    >
      <span className="mt-0.5 flex h-4 w-4 shrink-0 items-center justify-center">
        {isSelected && <Check className="h-4 w-4" />}
      </span>
      <span className="min-w-0">
        <span className="block truncate text-[0.875rem] font-medium leading-snug">
          {label}
        </span>
        {description && description !== label && (
          <span
            className="mt-0.5 block truncate font-mono text-[0.6875rem] leading-snug"
            style={{ color: isSelected ? "currentColor" : "var(--text-muted)" }}
          >
            {description}
          </span>
        )}
      </span>
    </button>
  );
}

export function ProjectDropdown({
  projects,
  value,
  onValueChange,
  selectedProject,
  includeAllProjects = false,
  allProjectsLabel = "All projects",
  allProjectsDescription = "Aggregate metrics across every project",
  placeholder = "Select Project",
  onNewProject,
  open: controlledOpen,
  onOpenChange,
  className,
  align = "center",
  variant = "navbar",
  pageSize = 20,
  testId = "project-selector-trigger",
  dropdownTestId = "project-selector-dropdown",
  listTestId = "project-selector-list",
  searchTestId = "project-selector-search",
  newProjectTestId = "new-project-option",
  showMoreTestId = "project-selector-show-more",
  projectOptionTestId = (project) => `project-option-${project.id}`,
  allProjectsTestId = "project-option-all-projects",
}: ProjectDropdownProps) {
  const [internalOpen, setInternalOpen] = useState(false);
  const open = controlledOpen ?? internalOpen;
  const [searchQuery, setSearchQuery] = useState("");
  const [visibleCount, setVisibleCount] = useState(pageSize);
  const query = searchQuery.trim().toLowerCase();
  const resolvedSelectedProject =
    selectedProject ?? projects.find((project) => project.id === value) ?? null;
  const selectedLabel =
    value === null && includeAllProjects
      ? allProjectsLabel
      : resolvedSelectedProject?.name ?? placeholder;
  const filteredProjects = useMemo(
    () =>
      projects.filter((project) =>
        matchesQuery(project.name, project.workingDirectory, query)
      ),
    [projects, query],
  );
  const showAllProjectsOption =
    includeAllProjects && matchesQuery(allProjectsLabel, allProjectsDescription, query);
  const visibleProjects = filteredProjects.slice(0, visibleCount);
  const hasMoreProjects = filteredProjects.length > visibleProjects.length;

  useEffect(() => {
    setVisibleCount(pageSize);
  }, [pageSize, query, projects.length]);

  const handleOpenChange = (nextOpen: boolean) => {
    if (controlledOpen === undefined) setInternalOpen(nextOpen);
    onOpenChange?.(nextOpen);
    if (!nextOpen) {
      setSearchQuery("");
      setVisibleCount(pageSize);
    }
  };

  const handleSelect = (nextValue: string | null) => {
    onValueChange(nextValue);
    handleOpenChange(false);
    setSearchQuery("");
    setVisibleCount(pageSize);
  };

  const revealNextPage = () => {
    if (hasMoreProjects) {
      setVisibleCount((count) => count + pageSize);
    }
  };

  const triggerClassName =
    variant === "insights"
      ? "h-9 min-w-[180px] max-w-[260px] justify-between rounded-lg border px-3 text-[0.75rem] font-medium shadow-none transition-colors hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]"
      : "inline-flex h-8 max-w-[280px] items-center gap-2 overflow-hidden border px-3";
  const triggerStyle =
    variant === "insights"
      ? {
          backgroundColor: "var(--bg-surface)",
          borderColor: "var(--overlay-faint)",
          borderStyle: "solid",
          borderWidth: 1,
          color: "var(--text-secondary)",
        }
      : {
          borderColor: "var(--border-default)",
          borderStyle: "solid",
          borderWidth: 1,
        };

  return (
    <Popover open={open} onOpenChange={handleOpenChange}>
      <PopoverTrigger asChild>
        <Button
          type="button"
          variant={variant === "insights" ? "outline" : "ghost"}
          size="sm"
          className={cn(triggerClassName, className)}
          style={triggerStyle}
          data-testid={testId}
          aria-label={`Project selector: ${selectedLabel}`}
          aria-haspopup="listbox"
          aria-expanded={open}
          onKeyDown={(event) => {
            if (event.key === "ArrowDown") {
              event.preventDefault();
              handleOpenChange(true);
            }
          }}
        >
          {variant === "navbar" && (
            <FolderOpen className="h-4 w-4 shrink-0 text-[var(--text-secondary)]" />
          )}
          <span
            className={cn(
              "min-w-0 truncate font-medium",
              variant === "navbar" ? "text-sm" : "text-[0.75rem]",
              resolvedSelectedProject || (value === null && includeAllProjects)
                ? "text-[var(--text-primary)]"
                : "text-[var(--text-muted)]"
            )}
          >
            {selectedLabel}
          </span>
          <ChevronDown className="h-3.5 w-3.5 shrink-0 text-[var(--text-muted)]" />
        </Button>
      </PopoverTrigger>
      <PopoverContent
        align={align}
        sideOffset={8}
        className="w-[min(420px,calc(100vw-2rem))] p-0"
        style={{
          backgroundColor: "var(--bg-elevated)",
          borderColor: "var(--border-default)",
          borderStyle: "solid",
          borderWidth: 1,
        }}
        data-testid={dropdownTestId}
      >
        <div className="border-b border-[var(--border-subtle)] p-2">
          <div className="relative">
            <Search
              className="absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2"
              style={{ color: "var(--text-muted)" }}
            />
            <Input
              placeholder="Search projects..."
              value={searchQuery}
              onChange={(event) => setSearchQuery(event.target.value)}
              className="h-8 border-[var(--border-subtle)] bg-[var(--bg-surface)] pl-8 pr-2 text-xs text-[var(--text-primary)] placeholder:text-[var(--text-muted)] focus:ring-1 focus:ring-[var(--accent-primary)]/30"
              style={{ outline: "none", boxShadow: "none" }}
              autoFocus
              data-testid={searchTestId}
            />
          </div>
        </div>

        <div
          className="max-h-72 overflow-y-auto overscroll-contain"
          onScroll={(event) => {
            const target = event.currentTarget;
            const remaining = target.scrollHeight - target.scrollTop - target.clientHeight;
            if (remaining < 32) {
              revealNextPage();
            }
          }}
        >
          <div className="p-1" role="listbox" aria-label="Projects" data-testid={listTestId}>
            {showAllProjectsOption && (
              <ProjectOption
                label={allProjectsLabel}
                description={allProjectsDescription}
                isSelected={value === null}
                onSelect={() => handleSelect(null)}
                testId={allProjectsTestId}
              />
            )}
            {visibleProjects.length === 0 && !showAllProjectsOption ? (
              <div
                className="flex items-center justify-center py-6 text-xs"
                style={{ color: "var(--text-muted)" }}
              >
                No projects found
              </div>
            ) : (
              <div className="space-y-0.5">
                {visibleProjects.map((project) => (
                  <ProjectOption
                    key={project.id}
                    label={project.name}
                    description={project.workingDirectory ?? null}
                    isSelected={project.id === value}
                    onSelect={() => handleSelect(project.id)}
                    testId={projectOptionTestId(project)}
                  />
                ))}
              </div>
            )}
            {hasMoreProjects && (
              <button
                type="button"
                className="mt-1 flex w-full items-center justify-center rounded-md px-2 py-2 text-xs font-medium text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]"
                onClick={revealNextPage}
                data-testid={showMoreTestId}
              >
                Show {Math.min(pageSize, filteredProjects.length - visibleProjects.length)} more
              </button>
            )}
          </div>
        </div>

        {onNewProject && (
          <div className="border-t border-[var(--border-subtle)] p-1">
            <button
              type="button"
              className="flex w-full items-center gap-2 rounded-md px-3 py-2 text-left text-sm font-medium text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]"
              onClick={() => {
                onNewProject();
                handleOpenChange(false);
              }}
              data-testid={newProjectTestId}
            >
              <Plus className="h-4 w-4 text-[var(--accent-primary)]" />
              New Project...
            </button>
          </div>
        )}
      </PopoverContent>
    </Popover>
  );
}
