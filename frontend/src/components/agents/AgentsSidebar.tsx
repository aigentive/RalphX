import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import {
  Archive,
  ArrowDownUp,
  Check,
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  Folder,
  GitBranch,
  GitPullRequest,
  MoreHorizontal,
  Pencil,
  Pin,
  PinOff,
  Plus,
  RotateCcw,
  Search,
  SlidersHorizontal,
  X,
} from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";

import { Button } from "@/components/ui/button";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Input } from "@/components/ui/input";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { useChatStore } from "@/stores/chatStore";
import type { AgentSidebarConversationRow } from "@/api/chat";
import {
  useAgentSessionStore,
  type AgentProjectSort,
  type AgentSidebarGroupBy,
  type AgentSidebarPublicationState,
} from "@/stores/agentSessionStore";
import { withAlpha } from "@/lib/theme-colors";
import type { Project } from "@/types/project";
import {
  formatAgentConversationCreatedAt,
  formatAgentConversationCreatedAtTitle,
  getAgentConversationStoreKey,
  toProjectAgentConversation,
  type AgentConversation,
} from "./agentConversations";
import {
  getSidebarPublicationGroupLabel,
  PUBLICATION_STATE_OPTIONS,
} from "./agentSidebarMetadata";
import {
  useAgentSidebarProjectGroup,
  useAgentSidebarPublicationGroup,
} from "./useAgentSidebarPublicationGroup";
import { useArchivedConversationCounts } from "./useArchivedConversationCounts";

const AGENTS_SEARCH_DEBOUNCE_MS = 180;

const PROJECT_SORT_LABELS: Record<AgentProjectSort, string> = {
  latest: "Latest",
  az: "A-Z",
  za: "Z-A",
};

function afterSidebarControlPaint(callback: () => void) {
  if (typeof window === "undefined") {
    callback();
    return;
  }

  window.requestAnimationFrame(() => {
    window.setTimeout(callback, 0);
  });
}

const STATIC_RECENT_RUNS = [
  {
    title: "Add ranking to reefbot homepage",
    project: "reefbot.ai",
    time: "2h",
  },
  {
    title: "Tighten kanban drag handles",
    project: "shapeapp",
    time: "yesterday",
  },
];

interface AgentsSidebarProps {
  projects: Project[];
  focusedProjectId: string | null;
  selectedConversationId: string | null;
  pinnedConversation?: AgentConversation | null;
  onFocusProject: (projectId: string) => void;
  onSelectConversation: (projectId: string, conversation: AgentConversation) => void;
  onCreateAgent: () => void;
  onCreateProject: () => void;
  onArchiveProject: (projectId: string) => void | Promise<void>;
  onRenameConversation: (conversationId: string, title: string) => void | Promise<void>;
  onArchiveConversation: (conversation: AgentConversation) => void;
  onRestoreConversation: (conversation: AgentConversation) => void;
  showArchived: boolean;
  onShowArchivedChange: (showArchived: boolean) => void;
  onCollapse?: () => void;
}

export function AgentsSidebar({
  projects,
  focusedProjectId,
  selectedConversationId,
  pinnedConversation = null,
  onFocusProject,
  onSelectConversation,
  onCreateAgent,
  onCreateProject,
  onArchiveProject,
  onRenameConversation,
  onArchiveConversation,
  onRestoreConversation,
  showArchived,
  onShowArchivedChange,
  onCollapse,
}: AgentsSidebarProps) {
  const [isSearchOpen, setIsSearchOpen] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");
  const [isSearchFocused, setIsSearchFocused] = useState(false);
  const normalizedSearchInput = searchQuery.trim().toLowerCase();
  const normalizedSearch = useDebouncedValue(
    normalizedSearchInput,
    AGENTS_SEARCH_DEBOUNCE_MS
  );
  const showAllProjects = useAgentSessionStore((s) => s.showAllProjects);
  const setShowAllProjects = useAgentSessionStore((s) => s.setShowAllProjects);
  const projectSort = useAgentSessionStore((s) => s.projectSort);
  const setProjectSort = useAgentSessionStore((s) => s.setProjectSort);
  const sidebarGroupBy = useAgentSessionStore((s) => s.sidebarGroupBy);
  const setSidebarGroupBy = useAgentSessionStore((s) => s.setSidebarGroupBy);
  const sidebarProjectFilterIds = useAgentSessionStore(
    (s) => s.sidebarProjectFilterIds
  );
  const setSidebarProjectFilterIds = useAgentSessionStore(
    (s) => s.setSidebarProjectFilterIds
  );
  const toggleSidebarProjectFilter = useAgentSessionStore(
    (s) => s.toggleSidebarProjectFilter
  );
  const sidebarPublicationStateFilters = useAgentSessionStore(
    (s) => s.sidebarPublicationStateFilters
  );
  const toggleSidebarPublicationStateFilter = useAgentSessionStore(
    (s) => s.toggleSidebarPublicationStateFilter
  );
  const pinnedConversationIds = useAgentSessionStore((s) => s.pinnedConversationIds);
  const togglePinnedConversation = useAgentSessionStore(
    (s) => s.togglePinnedConversation
  );
  const pinnedConversationIdList = useMemo(
    () => Object.keys(pinnedConversationIds),
    [pinnedConversationIds]
  );
  const priorityConversationIds = useMemo(() => {
    const ids = new Set(pinnedConversationIdList);
    if (pinnedConversation) {
      ids.add(pinnedConversation.id);
    }
    return Array.from(ids);
  }, [pinnedConversation, pinnedConversationIdList]);
  const selectedProjectFilterIds = useMemo(() => {
    if (showAllProjects) {
      return projects.map((project) => project.id);
    }
    if (sidebarProjectFilterIds.length > 0) {
      return sidebarProjectFilterIds;
    }
    if (focusedProjectId) {
      return [focusedProjectId];
    }
    return projects[0] ? [projects[0].id] : [];
  }, [focusedProjectId, projects, showAllProjects, sidebarProjectFilterIds]);
  const selectedProjectFilterSet = useMemo(
    () => new Set(selectedProjectFilterIds),
    [selectedProjectFilterIds]
  );
  const pinnedProjectId = pinnedConversation?.projectId ?? null;
  const archivedCountProjectIds = useMemo(() => {
    if (selectedProjectFilterIds.length > 0) {
      return selectedProjectFilterIds;
    }

    const projectIds = new Set<string>();
    if (focusedProjectId) {
      projectIds.add(focusedProjectId);
    }
    if (pinnedProjectId) {
      projectIds.add(pinnedProjectId);
    }
    if (projectIds.size === 0 && projects[0]) {
      projectIds.add(projects[0].id);
    }
    return Array.from(projectIds);
  }, [
    focusedProjectId,
    pinnedProjectId,
    projects,
    selectedProjectFilterIds,
  ]);
  const { totalArchivedCount } = useArchivedConversationCounts(archivedCountProjectIds);
  const orderedProjects = useMemo(() => {
    if (projectSort === "latest") {
      return projects.filter((project) => selectedProjectFilterSet.has(project.id));
    }

    const sortedProjects = [...projects].sort((a, b) =>
      a.name.localeCompare(b.name, undefined, { sensitivity: "base" })
    );
    const nextProjects = projectSort === "za" ? sortedProjects.reverse() : sortedProjects;
    return nextProjects.filter((project) => selectedProjectFilterSet.has(project.id));
  }, [projectSort, projects, selectedProjectFilterSet]);
  const selectedPublicationStates = sidebarPublicationStateFilters;

  return (
    <aside
      className="w-full h-full flex flex-col border-r overflow-hidden"
      style={{
        backgroundColor: "var(--app-sidebar-bg)",
        borderRightColor: "var(--app-sidebar-border)",
        borderRightStyle: "solid",
        borderRightWidth: "1px",
        boxShadow: "none",
      }}
      data-testid="agents-sidebar"
    >
      <div
        className="flex shrink-0 items-center gap-3 px-3 pb-2 pt-3"
      >
        <button
          type="button"
          className="inline-flex h-7 items-center gap-1.5 rounded-[6px] border px-2 pr-2.5 text-[0.7812rem] font-medium transition-colors duration-[120ms] ease-[cubic-bezier(.2,.8,.2,1)] outline-none focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:2px]"
          onClick={onCreateAgent}
          aria-label="New agent"
          data-testid="agents-new-agent"
          style={{
            backgroundColor: "var(--bg-elevated)",
            borderColor: "var(--border-subtle)",
            color: "var(--text-primary)",
            letterSpacing: "-0.005em",
            boxShadow: "none",
          }}
        >
          <Plus className="h-[13px] w-[13px]" style={{ color: "var(--text-muted)" }} />
          <span>New</span>
        </button>
        <div className="ml-auto flex items-center gap-1">
          <Tooltip>
            <TooltipTrigger asChild>
              <button
                type="button"
                className="grid h-7 w-7 place-items-center rounded-[6px] border-0 p-0 transition-colors duration-[120ms] ease-[cubic-bezier(.2,.8,.2,1)] outline-none hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)] focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:2px]"
                onClick={() => {
                  setIsSearchOpen((open) => {
                    if (open) {
                      setSearchQuery("");
                    }
                    return !open;
                  });
                }}
                aria-label={isSearchOpen ? "Close search" : "Search"}
                data-testid="agents-search-toggle"
                style={{ color: "var(--text-muted)", boxShadow: "none" }}
              >
                {isSearchOpen ? <X className="h-3.5 w-3.5" /> : <Search className="h-3.5 w-3.5" />}
              </button>
            </TooltipTrigger>
            <TooltipContent side="bottom" className="text-xs">
              {isSearchOpen ? "Close search" : "Search"}
            </TooltipContent>
          </Tooltip>
          {onCollapse && (
            <Tooltip>
              <TooltipTrigger asChild>
                <button
                  type="button"
                  className="grid h-7 w-7 place-items-center rounded-[6px] border-0 p-0 transition-colors duration-[120ms] ease-[cubic-bezier(.2,.8,.2,1)] outline-none hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)] focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:2px]"
                  onClick={onCollapse}
                  aria-label="Collapse sidebar"
                  data-testid="agents-sidebar-collapse-button"
                  style={{ color: "var(--text-muted)", boxShadow: "none" }}
                >
                  <ChevronLeft className="h-3.5 w-3.5" />
                </button>
              </TooltipTrigger>
              <TooltipContent side="bottom" className="text-xs">
                Collapse sidebar
              </TooltipContent>
            </Tooltip>
          )}
        </div>
      </div>

      {isSearchOpen && (
        <div className="px-3.5 pb-2 shrink-0">
          <div
            className="relative flex items-center"
            style={{
              backgroundColor: "var(--overlay-faint)",
              borderColor: isSearchFocused
                ? "var(--accent-border)"
                : "var(--overlay-weak)",
              borderStyle: "solid",
              borderWidth: "1px",
              borderRadius: "6px",
            }}
          >
            <Search
              className="absolute left-2.5 w-3.5 h-3.5 pointer-events-none"
              style={{ color: "var(--text-muted)" }}
            />
            <input
              value={searchQuery}
              onChange={(event) => setSearchQuery(event.target.value)}
              onFocus={() => setIsSearchFocused(true)}
              onBlur={() => setIsSearchFocused(false)}
              placeholder="Search"
              className="w-full h-7 pl-8 pr-8 text-[0.75rem] bg-transparent outline-none ring-0 focus:ring-0 focus:outline-none focus-visible:outline-none border-0"
              style={{
                color: "var(--text-primary)",
                caretColor: "var(--accent-primary)",
              }}
              autoFocus
              data-testid="agents-search-input"
              data-agent-sidebar-search="true"
            />
            {searchQuery !== "" && (
              <button
                type="button"
                aria-label="Clear search"
                onClick={() => setSearchQuery("")}
                className="absolute right-2 w-4 h-4 flex items-center justify-center rounded-sm transition-colors duration-100 outline-none ring-0 focus:ring-0 focus:outline-none focus-visible:outline-none"
                style={{ color: "var(--text-muted)" }}
              >
                <X className="w-3.5 h-3.5" />
              </button>
            )}
          </div>
        </div>
      )}

      {projects.length > 0 && (
        <AgentsSidebarToolbar
          projects={projects}
          focusedProjectId={focusedProjectId}
          projectSort={projectSort}
          selectedProjectFilterSet={selectedProjectFilterSet}
          selectedPublicationStates={selectedPublicationStates}
          setProjectSort={setProjectSort}
          setShowAllProjects={setShowAllProjects}
          setSidebarGroupBy={setSidebarGroupBy}
          setSidebarProjectFilterIds={setSidebarProjectFilterIds}
          showAllProjects={showAllProjects}
          showArchived={showArchived}
          sidebarGroupBy={sidebarGroupBy}
          toggleSidebarProjectFilter={toggleSidebarProjectFilter}
          toggleSidebarPublicationStateFilter={toggleSidebarPublicationStateFilter}
          totalArchivedCount={totalArchivedCount}
          onShowArchivedChange={onShowArchivedChange}
        />
      )}

      <div className="flex-1 overflow-y-auto px-3 pb-3 pt-0.5">
        {projects.length === 0 ? (
          <div className="h-full px-5 flex flex-col items-center justify-center text-center gap-3">
            <div className="space-y-1">
              <div className="text-sm font-medium" style={{ color: "var(--text-primary)" }}>
                No agent conversations yet.
              </div>
              <div className="text-xs leading-5" style={{ color: "var(--text-muted)" }}>
                Open the starter from the + button to begin a conversation and create a
                project inline if you need one.
              </div>
            </div>
            <Button type="button" size="sm" onClick={onCreateAgent} className="gap-2">
              <Plus className="w-4 h-4" />
              Open starter
            </Button>
          </div>
        ) : sidebarGroupBy === "publication" ? (
          <PublicationStateGroups
            projects={orderedProjects}
            priorityConversationIds={priorityConversationIds}
            pinnedConversationIds={pinnedConversationIds}
            rowSort={projectSort}
            selectedConversationId={selectedConversationId}
            searchQuery={normalizedSearch}
            selectedPublicationStates={selectedPublicationStates}
            onArchiveConversation={onArchiveConversation}
            onRenameConversation={onRenameConversation}
            onRestoreConversation={onRestoreConversation}
            onSelectConversation={onSelectConversation}
            onTogglePinnedConversation={togglePinnedConversation}
            showArchived={showArchived}
          />
        ) : (
          orderedProjects.map((project) => (
            <ProjectSessionGroup
              key={project.id}
              project={project}
              isFocused={focusedProjectId === project.id}
              selectedConversationId={selectedConversationId}
              searchQuery={normalizedSearch}
              onFocusProject={onFocusProject}
              onSelectConversation={onSelectConversation}
              onArchiveProject={onArchiveProject}
              onRenameConversation={onRenameConversation}
              onArchiveConversation={onArchiveConversation}
              onRestoreConversation={onRestoreConversation}
              onTogglePinnedConversation={togglePinnedConversation}
              priorityConversationIds={priorityConversationIds}
              pinnedConversationIds={pinnedConversationIds}
              selectedPublicationStates={selectedPublicationStates}
              showArchived={showArchived}
              showAllProjects={showAllProjects}
              showProjectHeader
              showProjectNameInMeta={false}
            />
          ))
        )}
      </div>

      <StaticRecentRuns />

      <div
        className="shrink-0 border-t px-3 py-3"
        style={{
          borderTopColor: "var(--app-sidebar-border)",
          borderTopStyle: "solid",
          borderTopWidth: "1px",
        }}
      >
        <button
          type="button"
          onClick={onCreateProject}
          data-testid="agents-add-project"
          className="inline-flex w-full items-center justify-center gap-2 rounded-[6px] border border-dashed px-3 py-2 text-[0.7812rem] font-medium transition-colors duration-[120ms] ease-[cubic-bezier(.2,.8,.2,1)] outline-none hover:bg-[var(--bg-elevated)] hover:text-[var(--text-primary)] focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:2px]"
          style={{
            color: "var(--text-muted)",
            borderColor: "var(--border-strong)",
            backgroundColor: "transparent",
            boxShadow: "none",
          }}
        >
          <Plus className="h-[13px] w-[13px]" />
          Add project
        </button>
      </div>
    </aside>
  );
}

interface AgentsSidebarToolbarProps {
  projects: Project[];
  focusedProjectId: string | null;
  projectSort: AgentProjectSort;
  selectedProjectFilterSet: Set<string>;
  selectedPublicationStates: AgentSidebarPublicationState[];
  setProjectSort: (projectSort: AgentProjectSort) => void;
  setShowAllProjects: (showAllProjects: boolean) => void;
  setSidebarGroupBy: (groupBy: AgentSidebarGroupBy) => void;
  setSidebarProjectFilterIds: (projectIds: string[]) => void;
  showAllProjects: boolean;
  showArchived: boolean;
  sidebarGroupBy: AgentSidebarGroupBy;
  toggleSidebarProjectFilter: (projectId: string) => void;
  toggleSidebarPublicationStateFilter: (
    state: AgentSidebarPublicationState
  ) => void;
  totalArchivedCount: number;
  onShowArchivedChange: (showArchived: boolean) => void;
}

function AgentsSidebarToolbar({
  projects,
  focusedProjectId,
  projectSort,
  selectedProjectFilterSet,
  selectedPublicationStates,
  setProjectSort,
  setShowAllProjects,
  setSidebarGroupBy,
  setSidebarProjectFilterIds,
  showAllProjects,
  showArchived,
  sidebarGroupBy,
  toggleSidebarProjectFilter,
  toggleSidebarPublicationStateFilter,
  totalArchivedCount,
  onShowArchivedChange,
}: AgentsSidebarToolbarProps) {
  const sortTarget = sidebarGroupBy === "project" ? "projects" : "conversations";
  const ensureScopedProjectSelection = () => {
    if (selectedProjectFilterSet.size > 0) {
      return;
    }
    const fallbackProjectId = focusedProjectId ?? projects[0]?.id;
    if (fallbackProjectId) {
      setSidebarProjectFilterIds([fallbackProjectId]);
    }
  };

  const handleAllProjectsChange = (checked: boolean | "indeterminate") => {
    const nextChecked = checked === true;
    setShowAllProjects(nextChecked);
    if (!nextChecked) {
      ensureScopedProjectSelection();
    }
  };

  const handleProjectFilterChange = (
    projectId: string,
    checked: boolean | "indeterminate"
  ) => {
    if (showAllProjects) {
      setShowAllProjects(false);
      const nextProjectIds = projects
        .map((project) => project.id)
        .filter((candidateProjectId) =>
          checked === true
            ? true
            : candidateProjectId !== projectId
        );
      setSidebarProjectFilterIds(nextProjectIds);
      return;
    }

    toggleSidebarProjectFilter(projectId);
  };

  const handleSortChange = (value: string) => {
    const nextSort = value as AgentProjectSort;
    afterSidebarControlPaint(() => setProjectSort(nextSort));
  };

  return (
    <div
      className="mb-2 flex h-8 shrink-0 items-center gap-1 px-3"
      role="toolbar"
      aria-label="Agent list filters"
      data-testid="agents-filter-toolbar"
      style={{
        backgroundColor: "var(--bg-surface)",
      }}
    >
      <Popover modal={false}>
        <PopoverTrigger asChild>
          <button
            type="button"
            data-testid="agents-filters-trigger"
            className="inline-flex h-full min-w-0 shrink-0 items-center gap-1.5 rounded-[4px] border border-transparent px-2 text-[0.7188rem] font-medium transition-colors duration-[120ms] outline-none hover:text-[var(--text-primary)] focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:-2px]"
            style={{
              backgroundColor: "transparent",
              borderColor: "transparent",
              color: "var(--text-muted)",
              boxShadow: "none",
            }}
          >
            <span>Filters</span>
            <SlidersHorizontal className="h-3.5 w-3.5" aria-hidden="true" />
          </button>
        </PopoverTrigger>
        <PopoverContent
          align="start"
          className="w-60 px-1.5 py-2.5"
          data-testid="agents-filter-popover"
          style={{
            backgroundColor: "var(--bg-elevated)",
            borderColor: "var(--border-subtle)",
            borderStyle: "solid",
            borderWidth: "1px",
            boxShadow: "var(--shadow-sm)",
          }}
        >
          <div className="space-y-3 text-xs">
            <div className="space-y-1">
              <FilterSectionLabel>Visibility</FilterSectionLabel>
              <FilterToggleRow
                selected={showArchived}
                onToggle={() => onShowArchivedChange(!showArchived)}
                label="Archived"
                ariaLabel="Show archived conversations"
                testId="agents-filter-archived"
                rightSlot={
                  <span
                    className="rounded-full px-1.5 text-[0.625rem] font-semibold leading-[1.6]"
                    style={{
                      backgroundColor: "var(--overlay-weak)",
                      color: "var(--text-secondary)",
                    }}
                  >
                    {totalArchivedCount}
                  </span>
                }
              />
            </div>

            <div className="space-y-1.5" data-testid="agents-filter-group-by">
              <FilterSectionLabel>Group by</FilterSectionLabel>
              <div
                className="grid grid-cols-2 gap-1"
                role="radiogroup"
                aria-label="Group by"
              >
                <button
                  type="button"
                  role="radio"
                  aria-checked={sidebarGroupBy === "project"}
                  className="truncate rounded-[4px] px-1.5 py-1 text-left whitespace-nowrap outline-none focus-visible:[outline:1px_solid_var(--accent-border)] focus-visible:[outline-offset:0px]"
                  onClick={() => setSidebarGroupBy("project")}
                  style={{
                    backgroundColor:
                      sidebarGroupBy === "project"
                        ? "var(--accent-muted)"
                        : "transparent",
                    color:
                      sidebarGroupBy === "project"
                        ? "var(--text-primary)"
                        : "var(--text-muted)",
                  }}
                >
                  Project
                </button>
                <button
                  type="button"
                  role="radio"
                  aria-checked={sidebarGroupBy === "publication"}
                  className="truncate rounded-[4px] px-1.5 py-1 text-left whitespace-nowrap outline-none focus-visible:[outline:1px_solid_var(--accent-border)] focus-visible:[outline-offset:0px]"
                  onClick={() => setSidebarGroupBy("publication")}
                  style={{
                    backgroundColor:
                      sidebarGroupBy === "publication"
                        ? "var(--accent-muted)"
                        : "transparent",
                    color:
                      sidebarGroupBy === "publication"
                        ? "var(--text-primary)"
                        : "var(--text-muted)",
                  }}
                >
                  Publication state
                </button>
              </div>
            </div>

            <FilterCollapsibleSection
              label="Projects"
              testId="agents-filter-projects-section"
              summary={
                showAllProjects
                  ? "All"
                  : `${selectedProjectFilterSet.size}/${projects.length}`
              }
            >
              <div className="max-h-44 space-y-0.5 overflow-y-auto">
                <FilterToggleRow
                  selected={showAllProjects}
                  onToggle={() => handleAllProjectsChange(!showAllProjects)}
                  label="All projects"
                  ariaLabel="All projects"
                  testId="agents-filter-all-projects"
                />
                {projects.map((project) => {
                  const projectSelected =
                    showAllProjects || selectedProjectFilterSet.has(project.id);
                  return (
                    <FilterToggleRow
                      key={project.id}
                      selected={projectSelected}
                      onToggle={() =>
                        handleProjectFilterChange(project.id, !projectSelected)
                      }
                      label={project.name}
                      ariaLabel={`Show ${project.name}`}
                      testId={`agents-filter-project-${project.id}`}
                    />
                  );
                })}
              </div>
            </FilterCollapsibleSection>

            <FilterCollapsibleSection
              label="Publication state"
              testId="agents-filter-publication-section"
              summary={`${selectedPublicationStates.length}/${PUBLICATION_STATE_OPTIONS.length}`}
            >
              <div className="space-y-0.5">
                {PUBLICATION_STATE_OPTIONS.map((option) => (
                  <FilterToggleRow
                    key={option.value}
                    selected={selectedPublicationStates.includes(option.value)}
                    onToggle={() =>
                      toggleSidebarPublicationStateFilter(option.value)
                    }
                    label={option.label}
                    ariaLabel={option.label}
                    testId={`agents-filter-publication-state-${option.value}`}
                  />
                ))}
              </div>
            </FilterCollapsibleSection>
          </div>
        </PopoverContent>
      </Popover>

      <DropdownMenu modal={false}>
        <DropdownMenuTrigger asChild>
          <button
            type="button"
            data-testid="agents-sort-trigger"
            aria-label={`Sort ${sortTarget}: ${PROJECT_SORT_LABELS[projectSort]}`}
            className="inline-flex h-full min-w-0 shrink-0 items-center gap-1.5 rounded-[4px] border border-transparent px-2 text-[0.7188rem] font-medium transition-colors duration-[120ms] outline-none hover:text-[var(--text-primary)] focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:-2px]"
            style={{
              backgroundColor: "transparent",
              borderColor: "transparent",
              color: "var(--text-muted)",
              boxShadow: "none",
            }}
          >
            <span>Sort</span>
            <ArrowDownUp className="h-3.5 w-3.5" aria-hidden="true" />
          </button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="start" className="min-w-[120px]">
          <DropdownMenuRadioGroup
            value={projectSort}
            onValueChange={handleSortChange}
          >
            {(["latest", "az", "za"] as AgentProjectSort[]).map((sort) => (
              <DropdownMenuRadioItem key={sort} value={sort} className="text-xs">
                {PROJECT_SORT_LABELS[sort]}
              </DropdownMenuRadioItem>
            ))}
          </DropdownMenuRadioGroup>
        </DropdownMenuContent>
      </DropdownMenu>
    </div>
  );
}

function FilterSectionLabel({
  children,
  inline = false,
}: {
  children: string;
  inline?: boolean;
}) {
  return (
    <div
      className={`text-[0.625rem] font-semibold uppercase leading-none tracking-[0.12em] ${
        inline ? "" : "px-1.5"
      }`}
      style={{ color: "var(--text-muted)" }}
    >
      {children}
    </div>
  );
}

interface FilterToggleRowProps {
  selected: boolean;
  onToggle: () => void;
  label: string;
  ariaLabel: string;
  testId: string;
  rightSlot?: React.ReactNode;
}

function FilterToggleRow({
  selected,
  onToggle,
  label,
  ariaLabel,
  testId,
  rightSlot,
}: FilterToggleRowProps) {
  return (
    <button
      type="button"
      role="checkbox"
      aria-checked={selected}
      aria-label={ariaLabel}
      data-testid={testId}
      onClick={onToggle}
      className="flex w-full min-w-0 items-center justify-between gap-2 rounded-[4px] px-1.5 py-1 text-left text-xs transition-colors duration-[120ms] outline-none hover:bg-[var(--overlay-weak)] focus-visible:[outline:1px_solid_var(--accent-border)] focus-visible:[outline-offset:0px]"
      style={{
        backgroundColor: "transparent",
        color: selected ? "var(--text-primary)" : "var(--text-muted)",
      }}
    >
      <span className="truncate">{label}</span>
      <span className="inline-flex shrink-0 items-center gap-2">
        {rightSlot}
        <Check
          className="h-3.5 w-3.5"
          aria-hidden="true"
          style={{
            color: selected ? "var(--accent-primary)" : "var(--text-muted)",
            opacity: selected ? 1 : 0.35,
          }}
        />
      </span>
    </button>
  );
}

interface FilterCollapsibleSectionProps {
  label: string;
  summary: string;
  testId: string;
  defaultOpen?: boolean;
  children: React.ReactNode;
}

function FilterCollapsibleSection({
  label,
  summary,
  testId,
  defaultOpen = false,
  children,
}: FilterCollapsibleSectionProps) {
  const [open, setOpen] = useState(defaultOpen);
  return (
    <Collapsible open={open} onOpenChange={setOpen} data-testid={testId}>
      <CollapsibleTrigger
        data-testid={`${testId}-trigger`}
        aria-label={`${label} filter`}
        className="flex w-full items-center justify-between gap-2 rounded-[4px] px-1.5 py-1 text-left transition-colors duration-[120ms] outline-none hover:bg-[var(--overlay-weak)] focus-visible:[outline:1px_solid_var(--accent-border)] focus-visible:[outline-offset:0px]"
      >
        <FilterSectionLabel inline>{label}</FilterSectionLabel>
        <span
          className="inline-flex shrink-0 items-center gap-1.5 text-[0.625rem] font-medium"
          style={{ color: "var(--text-secondary)" }}
        >
          <span>{summary}</span>
          <ChevronDown
            className="h-3 w-3 transition-transform duration-[120ms]"
            aria-hidden="true"
            style={{
              transform: open ? "rotate(180deg)" : "rotate(0deg)",
            }}
          />
        </span>
      </CollapsibleTrigger>
      <CollapsibleContent className="pt-1">{children}</CollapsibleContent>
    </Collapsible>
  );
}

interface PublicationStateGroupsProps {
  projects: Project[];
  priorityConversationIds: string[];
  pinnedConversationIds: Record<string, true>;
  rowSort: AgentProjectSort;
  selectedConversationId: string | null;
  searchQuery: string;
  selectedPublicationStates: AgentSidebarPublicationState[];
  onSelectConversation: (projectId: string, conversation: AgentConversation) => void;
  onRenameConversation: (conversationId: string, title: string) => void | Promise<void>;
  onArchiveConversation: (conversation: AgentConversation) => void;
  onRestoreConversation: (conversation: AgentConversation) => void;
  onTogglePinnedConversation: (conversationId: string) => void;
  showArchived: boolean;
}

function PublicationStateGroups({
  projects,
  priorityConversationIds,
  pinnedConversationIds,
  rowSort,
  selectedConversationId,
  searchQuery,
  selectedPublicationStates,
  onArchiveConversation,
  onRenameConversation,
  onRestoreConversation,
  onSelectConversation,
  onTogglePinnedConversation,
  showArchived,
}: PublicationStateGroupsProps) {
  const [expandedPublicationState, setExpandedPublicationState] =
    useState<AgentSidebarPublicationState | null>(() => selectedPublicationStates[0] ?? null);

  useEffect(() => {
    if (selectedPublicationStates.length === 0) {
      setExpandedPublicationState(null);
      return;
    }
    if (
      expandedPublicationState !== null &&
      !selectedPublicationStates.includes(expandedPublicationState)
    ) {
      setExpandedPublicationState(selectedPublicationStates[0] ?? null);
    }
  }, [expandedPublicationState, selectedPublicationStates]);

  return (
    <>
      {selectedPublicationStates.map((publicationState) => (
        <PublicationStateGroup
          key={publicationState}
          expandedPublicationState={expandedPublicationState}
          projects={projects}
          priorityConversationIds={priorityConversationIds}
          pinnedConversationIds={pinnedConversationIds}
          publicationState={publicationState}
          rowSort={rowSort}
          searchQuery={searchQuery}
          selectedConversationId={selectedConversationId}
          showArchived={showArchived}
          onArchiveConversation={onArchiveConversation}
          onRenameConversation={onRenameConversation}
          onRestoreConversation={onRestoreConversation}
          onSelectConversation={onSelectConversation}
          onTogglePinnedConversation={onTogglePinnedConversation}
          onTogglePublicationState={(state, expanded) =>
            setExpandedPublicationState(expanded ? state : null)
          }
        />
      ))}
    </>
  );
}

interface PublicationStateGroupProps {
  expandedPublicationState: AgentSidebarPublicationState | null;
  projects: Project[];
  priorityConversationIds: string[];
  pinnedConversationIds: Record<string, true>;
  publicationState: AgentSidebarPublicationState;
  rowSort: AgentProjectSort;
  searchQuery: string;
  selectedConversationId: string | null;
  showArchived: boolean;
  onSelectConversation: (projectId: string, conversation: AgentConversation) => void;
  onRenameConversation: (conversationId: string, title: string) => void | Promise<void>;
  onArchiveConversation: (conversation: AgentConversation) => void;
  onRestoreConversation: (conversation: AgentConversation) => void;
  onTogglePinnedConversation: (conversationId: string) => void;
  onTogglePublicationState: (
    publicationState: AgentSidebarPublicationState,
    expanded: boolean,
  ) => void;
}

function PublicationStateGroup({
  expandedPublicationState,
  projects,
  priorityConversationIds,
  pinnedConversationIds,
  publicationState,
  rowSort,
  searchQuery,
  selectedConversationId,
  showArchived,
  onArchiveConversation,
  onRenameConversation,
  onRestoreConversation,
  onSelectConversation,
  onTogglePinnedConversation,
  onTogglePublicationState,
}: PublicationStateGroupProps) {
  const projectIds = useMemo(() => projects.map((project) => project.id), [projects]);
  const projectById = useMemo(
    () => new Map(projects.map((project) => [project.id, project])),
    [projects]
  );
  const groupQuery = useAgentSidebarPublicationGroup({
    projectIds,
    publicationState,
    archivedOnly: showArchived,
    search: searchQuery,
    pinnedConversationIds: priorityConversationIds,
    sort: rowSort,
  });
  const activeConversationIds = useChatStore((s) => s.activeConversationIds);
  const agentStatuses = useChatStore((s) => s.agentStatus);
  const sessionActionsTriggerRefs = useRef<Record<string, HTMLButtonElement | null>>({});
  const [renameDialogConversation, setRenameDialogConversation] =
    useState<AgentConversation | null>(null);
  const [renameDraftTitle, setRenameDraftTitle] = useState("");
  const [archiveDialogConversation, setArchiveDialogConversation] =
    useState<AgentConversation | null>(null);
  const [openSessionActionsId, setOpenSessionActionsId] = useState<string | null>(null);

  const openRenameDialog = (conversation: AgentConversation) => {
    setRenameDraftTitle(conversation.title || "Untitled agent");
    setRenameDialogConversation(conversation);
  };

  const handleRenameSubmit = async () => {
    if (!renameDialogConversation) return;
    const trimmed = renameDraftTitle.trim();
    if (!trimmed) return;
    await onRenameConversation(renameDialogConversation.id, trimmed);
    setRenameDialogConversation(null);
  };
  const isCurrentPublicationState = expandedPublicationState === publicationState;
  const expanded = searchQuery.length > 0 ? true : isCurrentPublicationState;
  const groupLabel =
    groupQuery.group.label || getSidebarPublicationGroupLabel(publicationState);
  const totalConversationCount = groupQuery.group.total;

  return (
    <div
      className="my-1 flex flex-col gap-0.5"
      data-testid={`agents-publication-group-${publicationState}`}
    >
      <div className="group/publication-row relative">
        <button
          type="button"
          className="agents-project-row grid w-full grid-cols-[12px_14px_minmax(0,1fr)_auto] items-center gap-[7px] rounded-[6px] px-2 py-1.5 text-left text-[0.8438rem] transition-colors duration-[120ms] ease-[cubic-bezier(.2,.8,.2,1)] outline-none hover:bg-[var(--bg-elevated)] focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:2px]"
          data-testid={`agents-publication-row-${publicationState}`}
          aria-expanded={expanded}
          aria-current={isCurrentPublicationState ? "true" : undefined}
          aria-label={`${expanded ? "Collapse" : "Expand"} publication state ${groupLabel}`}
          onClick={() => onTogglePublicationState(publicationState, !expanded)}
        >
          <span
            className="agents-project-chevron grid h-3 w-3 place-items-center rounded"
            aria-hidden="true"
          >
            <ChevronRight
              className={`h-2.5 w-2.5 transition-transform duration-[120ms] ${expanded ? "rotate-90" : ""}`}
              strokeWidth={2}
            />
          </span>
          <PublicationStateGroupIcon state={publicationState} />
          <span className="min-w-0 truncate">{groupLabel}</span>
          <span className="agents-project-count agents-publication-count grid min-w-[18px] place-items-center rounded-full border px-1.5 text-[0.6562rem] leading-[1.6]">
            {totalConversationCount}
          </span>
        </button>
      </div>

      <Dialog
        open={renameDialogConversation !== null}
        onOpenChange={(open) => {
          if (!open) {
            setRenameDialogConversation(null);
          }
        }}
      >
        <DialogContent hideCloseButton className="max-w-md">
          <DialogHeader className="block space-y-1.5">
            <DialogTitle className="text-base">Rename session</DialogTitle>
            <DialogDescription>
              Update the title shown in the Agents sidebar for this conversation.
            </DialogDescription>
          </DialogHeader>
          <div className="px-6 py-4">
            <Input
              value={renameDraftTitle}
              onChange={(event) => setRenameDraftTitle(event.target.value)}
              aria-label="Session title"
              placeholder="Untitled agent"
              autoFocus
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  event.preventDefault();
                  void handleRenameSubmit();
                }
              }}
            />
          </div>
          <DialogFooter>
            <Button
              type="button"
              variant="outline"
              onClick={() => setRenameDialogConversation(null)}
            >
              Cancel
            </Button>
            <Button
              type="button"
              onClick={() => void handleRenameSubmit()}
              disabled={renameDraftTitle.trim().length === 0}
            >
              Rename session
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <AlertDialog
        open={archiveDialogConversation !== null}
        onOpenChange={(open) => {
          if (!open) {
            setArchiveDialogConversation(null);
          }
        }}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Archive session?</AlertDialogTitle>
            <AlertDialogDescription>
              This hides{" "}
              <span className="font-medium">
                {archiveDialogConversation?.title || "Untitled agent"}
              </span>{" "}
              from the active conversation list. You can restore it later from
              archived sessions.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction
              onClick={() => {
                if (archiveDialogConversation) {
                  void onArchiveConversation(archiveDialogConversation);
                }
                setArchiveDialogConversation(null);
              }}
              variant="destructive"
            >
              Archive session
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      {expanded && (
        <div className="mb-2 mt-1 flex flex-col gap-0.5" role="group">
          {groupQuery.group.rows.map((row) => {
            const conversation = toProjectAgentConversation(row.conversation);
            const project = projectById.get(conversation.projectId);
            const rowKey = getAgentConversationStoreKey(conversation);
            const activeConversationId = activeConversationIds[rowKey] ?? null;
            const agentStatus = agentStatuses[rowKey] ?? "idle";
            const isSelected = selectedConversationId === conversation.id;
            const isActiveRuntime = activeConversationId === conversation.id;
            const isPinned = Boolean(pinnedConversationIds[conversation.id]);
            const runtimeState = getSessionRuntimeState(
              conversation,
              isActiveRuntime,
              agentStatus
            );
            const showRuntimeState = runtimeState === "running";
            const sessionActionsOpen = openSessionActionsId === conversation.id;

            return (
              <AgentSessionRow
                key={conversation.id}
                conversation={conversation}
                projectName={project?.name ?? conversation.projectId}
                showProjectNameInMeta
                refKind={row.refKind}
                refLabel={row.refLabel}
                publicationState={row.publicationState}
                publicationLabel={row.publicationLabel}
                isSelected={isSelected}
                isPinned={isPinned}
                runtimeState={runtimeState}
                showRuntimeState={showRuntimeState}
                sessionActionsOpen={sessionActionsOpen}
                onSelect={() => onSelectConversation(conversation.projectId, conversation)}
                onRename={() => openRenameDialog(conversation)}
                onTogglePinned={() => onTogglePinnedConversation(conversation.id)}
                onRestore={() => onRestoreConversation(conversation)}
                onArchiveRequest={() => setArchiveDialogConversation(conversation)}
                setActionsTriggerRef={(node) => {
                  sessionActionsTriggerRefs.current[conversation.id] = node;
                }}
                onActionsOpenChange={(open) => {
                  setOpenSessionActionsId(open ? conversation.id : null);
                  if (!open) {
                    requestAnimationFrame(() => {
                      sessionActionsTriggerRefs.current[conversation.id]?.blur();
                    });
                  }
                }}
              />
            );
          })}

          {groupQuery.group.rows.length > 0 && groupQuery.hasNextPage && (
            <div className="flex justify-end py-0.5 pr-2">
              <button
                type="button"
                className="inline-flex items-center text-[0.6719rem] font-medium transition-colors"
                onClick={() => void groupQuery.fetchNextPage()}
                disabled={groupQuery.isFetchingNextPage}
                data-testid={`agents-load-more-publication-${publicationState}`}
                style={{
                  color: "var(--text-muted)",
                  opacity: groupQuery.isFetchingNextPage ? 0.7 : 1,
                }}
              >
                {groupQuery.isFetchingNextPage ? "Loading..." : "Load more"}
              </button>
            </div>
          )}

          {groupQuery.isLoading && (
            <div className="py-1.5 text-[0.6875rem]" style={{ color: "var(--text-muted)" }}>
              Loading...
            </div>
          )}
        </div>
      )}
    </div>
  );
}

function PublicationStateGroupIcon({
  state,
}: {
  state: AgentSidebarPublicationState;
}) {
  if (state === "active") {
    return (
      <GitBranch
        className="agents-project-icon h-3.5 w-3.5 shrink-0"
        strokeWidth={1.8}
        aria-hidden="true"
      />
    );
  }

  return (
    <GitPullRequest
      className="agents-project-icon h-3.5 w-3.5 shrink-0"
      strokeWidth={1.8}
      aria-hidden="true"
    />
  );
}

interface AgentSessionRowProps {
  conversation: AgentConversation;
  projectName: string | null;
  showProjectNameInMeta: boolean;
  refKind: AgentSidebarConversationRow["refKind"];
  refLabel: string;
  publicationState: AgentSidebarPublicationState;
  publicationLabel: string | null;
  isSelected: boolean;
  isPinned: boolean;
  runtimeState: SessionRuntimeState;
  showRuntimeState: boolean;
  sessionActionsOpen: boolean;
  onSelect: () => void;
  onRename: () => void;
  onTogglePinned: () => void;
  onRestore: () => void;
  onArchiveRequest: () => void;
  setActionsTriggerRef: (node: HTMLButtonElement | null) => void;
  onActionsOpenChange: (open: boolean) => void;
}

function AgentSessionRow({
  conversation,
  projectName,
  showProjectNameInMeta,
  refKind,
  refLabel,
  publicationState,
  publicationLabel,
  isSelected,
  isPinned,
  runtimeState,
  showRuntimeState,
  sessionActionsOpen,
  onSelect,
  onRename,
  onTogglePinned,
  onRestore,
  onArchiveRequest,
  setActionsTriggerRef,
  onActionsOpenChange,
}: AgentSessionRowProps) {
  const title = conversation.title || "Untitled agent";
  const createdLabel = formatAgentConversationCreatedAt(conversation.createdAt);
  const createdTitle = formatAgentConversationCreatedAtTitle(conversation.createdAt);

  return (
    <div
      className="group/session relative"
      data-testid={`agents-session-${conversation.id}`}
    >
      <button
        type="button"
        className="agents-session-row grid w-full min-w-0 grid-cols-[minmax(0,1fr)_auto] items-center gap-2 rounded-[6px] px-2.5 py-1.5 text-left transition-colors duration-[120ms] ease-[cubic-bezier(.2,.8,.2,1)] outline-none hover:bg-[var(--bg-elevated)] focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:2px]"
        onClick={onSelect}
        aria-current={isSelected ? "true" : undefined}
        style={{
          opacity: conversation.archivedAt ? 0.58 : 1,
          boxShadow: "none",
        }}
      >
        <span className="min-w-0 flex flex-col gap-px">
          <span className="agents-session-title min-w-0 truncate text-[0.8125rem] leading-[1.35]">
            {title}
          </span>
          <span
            className="agents-session-meta flex min-w-0 items-center gap-1 overflow-hidden whitespace-nowrap text-[0.6875rem] leading-[1.35]"
            style={{
              fontFamily: "var(--font-mono, ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace)",
            }}
          >
            {showProjectNameInMeta && projectName && (
              <>
                <span className="max-w-24 shrink-0 truncate">{projectName}</span>
                <span className="flex h-[1em] shrink-0 items-center" aria-hidden="true">
                  ·
                </span>
              </>
            )}
            <span className="inline-flex min-w-0 items-center gap-1">
              {refKind === "pull-request" ? (
                <GitPullRequest
                  className="h-3 w-3 shrink-0 -translate-y-px"
                  data-ref-kind="pull-request"
                  data-testid={`agents-ref-icon-${conversation.id}`}
                  aria-hidden="true"
                />
              ) : (
                <GitBranch
                  className="h-3 w-3 shrink-0 -translate-y-px"
                  data-ref-kind="branch"
                  data-testid={`agents-ref-icon-${conversation.id}`}
                  aria-hidden="true"
                />
              )}
              <span className="min-w-0 truncate">{refLabel}</span>
            </span>
            <span className="flex h-[1em] shrink-0 items-center" aria-hidden="true">
              ·
            </span>
            {publicationLabel && (
              <>
                <span
                  className="agents-session-publication-state shrink-0 font-medium"
                  style={{
                    color:
                      publicationState === "merged"
                        ? "var(--status-success)"
                        : publicationState === "closed"
                          ? "var(--text-muted)"
                          : "var(--status-warning)",
                  }}
                >
                  {publicationLabel}
                </span>
                <span className="flex h-[1em] shrink-0 items-center" aria-hidden="true">
                  ·
                </span>
              </>
            )}
            <span className="shrink-0" title={createdTitle || undefined}>
              {createdLabel}
            </span>
            {showRuntimeState && (
              <>
                <span className="flex h-[1em] shrink-0 items-center" aria-hidden="true">
                  ·
                </span>
                <SessionRuntimeLabel state={runtimeState} />
              </>
            )}
          </span>
        </span>
        <span
          className={`agents-session-status-slot grid h-4 w-4 place-items-center justify-self-end transition-opacity duration-150 ${
            sessionActionsOpen
              ? "opacity-0"
              : "opacity-100 group-hover/session:opacity-0 group-focus-within/session:opacity-0"
          }`}
        >
          <SessionStatusIcon
            isPinned={isPinned}
            state={runtimeState}
            conversationId={conversation.id}
            selected={isSelected}
          />
        </span>
      </button>
      <DropdownMenu modal={false} onOpenChange={onActionsOpenChange}>
        <DropdownMenuTrigger asChild>
          <Button
            ref={setActionsTriggerRef}
            type="button"
            variant="ghost"
            size="sm"
            className="absolute right-2 top-1/2 h-6 w-6 -translate-y-1/2 rounded-[6px] border-0 bg-transparent p-0 opacity-0 outline-none ring-0 transition-opacity hover:bg-transparent focus:bg-transparent focus:outline-none focus:ring-0 focus-visible:bg-transparent focus-visible:outline-none focus-visible:ring-0 group-hover/session:opacity-100 group-focus-within/session:opacity-100 data-[state=open]:bg-transparent data-[state=open]:opacity-100"
            aria-label="Session actions"
            style={{ boxShadow: "none" }}
          >
            <MoreHorizontal className="h-3.5 w-3.5" />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent
          align="end"
          onCloseAutoFocus={(event) => {
            event.preventDefault();
          }}
        >
          <DropdownMenuItem className="gap-2 text-xs" onClick={onRename}>
            <Pencil className="w-3.5 h-3.5" />
            Rename session
          </DropdownMenuItem>
          <DropdownMenuItem className="gap-2 text-xs" onClick={onTogglePinned}>
            {isPinned ? (
              <PinOff className="w-3.5 h-3.5" />
            ) : (
              <Pin className="w-3.5 h-3.5" />
            )}
            {isPinned ? "Unpin session" : "Pin session"}
          </DropdownMenuItem>
          <DropdownMenuSeparator />
          {conversation.archivedAt ? (
            <DropdownMenuItem className="gap-2 text-xs" onClick={onRestore}>
              <RotateCcw className="w-3.5 h-3.5" />
              Restore session
            </DropdownMenuItem>
          ) : (
            <DropdownMenuItem className="gap-2 text-xs" onClick={onArchiveRequest}>
              <Archive className="w-3.5 h-3.5" />
              Archive session
            </DropdownMenuItem>
          )}
        </DropdownMenuContent>
      </DropdownMenu>
    </div>
  );
}

interface ProjectSessionGroupProps {
  project: Project;
  isFocused: boolean;
  selectedConversationId: string | null;
  searchQuery: string;
  onFocusProject: (projectId: string) => void;
  onSelectConversation: (projectId: string, conversation: AgentConversation) => void;
  onArchiveProject: (projectId: string) => void | Promise<void>;
  onRenameConversation: (conversationId: string, title: string) => void | Promise<void>;
  onArchiveConversation: (conversation: AgentConversation) => void;
  onRestoreConversation: (conversation: AgentConversation) => void;
  onTogglePinnedConversation: (conversationId: string) => void;
  priorityConversationIds: string[];
  pinnedConversationIds: Record<string, true>;
  selectedPublicationStates: AgentSidebarPublicationState[];
  showArchived: boolean;
  showAllProjects: boolean;
  showProjectHeader: boolean;
  showProjectNameInMeta: boolean;
}

function ProjectSessionGroup({
  project,
  isFocused,
  selectedConversationId,
  searchQuery,
  onFocusProject,
  onSelectConversation,
  onArchiveProject,
  onRenameConversation,
  onArchiveConversation,
  onRestoreConversation,
  onTogglePinnedConversation,
  priorityConversationIds,
  pinnedConversationIds,
  selectedPublicationStates,
  showArchived,
  showAllProjects,
  showProjectHeader,
  showProjectNameInMeta,
}: ProjectSessionGroupProps) {
  const projectActionsTriggerRef = useRef<HTMLButtonElement | null>(null);
  const sessionActionsTriggerRefs = useRef<Record<string, HTMLButtonElement | null>>({});
  const [projectActionsOpen, setProjectActionsOpen] = useState(false);
  const [archiveDialogOpen, setArchiveDialogOpen] = useState(false);
  const [renameDialogConversation, setRenameDialogConversation] =
    useState<AgentConversation | null>(null);
  const [renameDraftTitle, setRenameDraftTitle] = useState("");
  const [archiveDialogConversation, setArchiveDialogConversation] =
    useState<AgentConversation | null>(null);
  const [openSessionActionsId, setOpenSessionActionsId] = useState<string | null>(null);
  const expandedProjectIds = useAgentSessionStore((s) => s.expandedProjectIds);
  const setProjectExpanded = useAgentSessionStore((s) => s.setProjectExpanded);
  const expanded = searchQuery.length > 0 ? true : expandedProjectIds[project.id] ?? isFocused;
  const groupQuery = useAgentSidebarProjectGroup({
    projectId: project.id,
    archivedOnly: showArchived,
    search: searchQuery,
    publicationStates: selectedPublicationStates,
    pinnedConversationIds: priorityConversationIds,
  });
  const activeConversationIds = useChatStore((s) => s.activeConversationIds);
  const agentStatuses = useChatStore((s) => s.agentStatus);
  const visibleRows = groupQuery.group.rows;
  const visibleConversations = useMemo(
    () => visibleRows.map((row) => toProjectAgentConversation(row.conversation)),
    [visibleRows]
  );
  const totalConversationCount = groupQuery.group.total;
  const activeRuntimeCount = visibleConversations.filter((conversation) => {
    const rowKey = getAgentConversationStoreKey(conversation);
    return (
      activeConversationIds[rowKey] === conversation.id &&
      (agentStatuses[rowKey] ?? "idle") !== "idle"
    );
  }).length;
  const isCurrentProject = expanded && isFocused;
  const handleProjectRowToggle = () => {
    onFocusProject(project.id);
    setProjectExpanded(project.id, !expanded);
  };
  const openRenameDialog = (conversation: AgentConversation) => {
    setRenameDraftTitle(conversation.title || "Untitled agent");
    setRenameDialogConversation(conversation);
  };
  const handleRenameSubmit = async () => {
    if (!renameDialogConversation) {
      return;
    }
    const trimmed = renameDraftTitle.trim();
    if (!trimmed) {
      return;
    }

    await onRenameConversation(renameDialogConversation.id, trimmed);
    setRenameDialogConversation(null);
  };

  if (
    !groupQuery.isLoading &&
    visibleConversations.length === 0 &&
    (!showProjectHeader || showArchived || searchQuery.length > 0 || !showAllProjects)
  ) {
    return null;
  }

  return (
    <div
      className="my-1 flex flex-col gap-0.5"
      data-testid={
        showProjectHeader
          ? `agents-project-${project.id}`
          : `agents-project-${project.id}-state`
      }
    >
        <div className="relative">
          {showProjectHeader && (
          <div className="group/project-row relative">
          <button
            type="button"
            className="agents-project-row grid w-full grid-cols-[12px_14px_minmax(0,1fr)_auto] items-center gap-[7px] rounded-[6px] px-2 py-1.5 text-left text-[0.8438rem] transition-colors duration-[120ms] ease-[cubic-bezier(.2,.8,.2,1)] outline-none hover:bg-[var(--bg-elevated)] focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:2px]"
            data-testid={`agents-project-row-${project.id}`}
            aria-expanded={expanded}
            aria-label={`${expanded ? "Collapse" : "Expand"} project ${project.name}`}
            aria-current={isCurrentProject ? "true" : undefined}
            onClick={handleProjectRowToggle}
          >
            <span
              className="agents-project-chevron grid h-3 w-3 place-items-center rounded"
              aria-hidden="true"
            >
              <ChevronRight
                className={`h-2.5 w-2.5 transition-transform duration-[120ms] ${expanded ? "rotate-90" : ""}`}
                strokeWidth={2}
              />
            </span>
            <Folder
              className="agents-project-icon h-3.5 w-3.5 shrink-0"
              strokeWidth={1.8}
            />
            <span className="min-w-0 truncate">
              {project.name}
            </span>
            {totalConversationCount > 0 && (
              <span
                className={`agents-project-count grid min-w-[18px] place-items-center rounded-full border px-1.5 text-[0.6562rem] leading-[1.6] transition-opacity duration-150 ${
                  projectActionsOpen
                    ? "opacity-0"
                    : "opacity-100 group-hover/project-row:opacity-0 group-focus-within/project-row:opacity-0"
                }`}
              >
                {totalConversationCount}
              </span>
            )}
            {totalConversationCount === 0 && !expanded && activeRuntimeCount > 0 && (
              <span
                className={`agents-project-active-count grid min-w-[18px] place-items-center rounded-full px-1.5 text-[0.6562rem] font-medium leading-[1.6] transition-opacity duration-150 ${
                  projectActionsOpen
                    ? "opacity-0"
                    : "opacity-100 group-hover/project-row:opacity-0 group-focus-within/project-row:opacity-0"
                }`}
                style={{
                  color: "var(--accent-primary)",
                  backgroundColor: withAlpha("var(--accent-primary)", 15),
                }}
              >
                {activeRuntimeCount}
              </span>
            )}
          </button>
            <div
              className={`absolute right-1 top-1/2 flex -translate-y-1/2 items-center gap-0.5 rounded-[6px] transition-opacity duration-150 ${
                projectActionsOpen
                  ? "opacity-100"
                  : "opacity-0 group-hover/project-row:opacity-100 group-focus-within/project-row:opacity-100"
              }`}
              data-testid={`agents-project-actions-${project.id}`}
              onClick={(event) => event.stopPropagation()}
            >
              <DropdownMenu
                modal={false}
                onOpenChange={(open) => {
                  setProjectActionsOpen(open);
                  if (!open) {
                    requestAnimationFrame(() => {
                      projectActionsTriggerRef.current?.blur();
                    });
                  }
                }}
              >
                <DropdownMenuTrigger asChild>
                  <Button
                    ref={projectActionsTriggerRef}
                    type="button"
                    variant="ghost"
                    size="sm"
                    className="h-5.5 w-5.5 rounded border-0 bg-transparent p-0 outline-none ring-0 hover:bg-transparent focus:bg-transparent focus:outline-none focus:ring-0 focus-visible:bg-transparent focus-visible:outline-none focus-visible:ring-0 data-[state=open]:bg-transparent"
                    aria-label="Project actions"
                    data-theme-button-skip="true"
                    style={{ boxShadow: "none" }}
                  >
                    <MoreHorizontal className="w-3.5 h-3.5" />
                  </Button>
                </DropdownMenuTrigger>
                <DropdownMenuContent
                  align="end"
                  onCloseAutoFocus={(event) => {
                    event.preventDefault();
                    projectActionsTriggerRef.current?.blur();
                  }}
                >
                  <DropdownMenuItem
                    className="gap-2 text-xs"
                    onClick={() => setArchiveDialogOpen(true)}
                  >
                    <Archive className="w-3.5 h-3.5" />
                    Archive project
                  </DropdownMenuItem>
                </DropdownMenuContent>
              </DropdownMenu>
            </div>
          </div>
          )}

          <AlertDialog open={archiveDialogOpen} onOpenChange={setArchiveDialogOpen}>
            <AlertDialogContent>
              <AlertDialogHeader>
                <AlertDialogTitle>Archive project?</AlertDialogTitle>
                <AlertDialogDescription>
                  This removes <span className="font-medium">{project.name}</span> from the
                  sidebar without deleting it. You can add the same repository again later
                  from the normal project flow.
                </AlertDialogDescription>
              </AlertDialogHeader>
              <AlertDialogFooter>
                <AlertDialogCancel>Cancel</AlertDialogCancel>
                <AlertDialogAction
                  onClick={() => {
                    setArchiveDialogOpen(false);
                    void onArchiveProject(project.id);
                  }}
                  variant="destructive"
                >
                  Archive project
                </AlertDialogAction>
              </AlertDialogFooter>
            </AlertDialogContent>
          </AlertDialog>

          <Dialog
            open={renameDialogConversation !== null}
            onOpenChange={(open) => {
              if (!open) {
                setRenameDialogConversation(null);
              }
            }}
          >
            <DialogContent hideCloseButton className="max-w-md">
              <DialogHeader className="block space-y-1.5">
                <DialogTitle className="text-base">Rename session</DialogTitle>
                <DialogDescription>
                  Update the title shown in the Agents sidebar for this conversation.
                </DialogDescription>
              </DialogHeader>
              <div className="px-6 py-4">
                <Input
                  value={renameDraftTitle}
                  onChange={(event) => setRenameDraftTitle(event.target.value)}
                  aria-label="Session title"
                  placeholder="Untitled agent"
                  autoFocus
                  onKeyDown={(event) => {
                    if (event.key === "Enter") {
                      event.preventDefault();
                      void handleRenameSubmit();
                    }
                  }}
                />
              </div>
              <DialogFooter>
                <Button
                  type="button"
                  variant="outline"
                  onClick={() => setRenameDialogConversation(null)}
                >
                  Cancel
                </Button>
                <Button
                  type="button"
                  onClick={() => void handleRenameSubmit()}
                  disabled={renameDraftTitle.trim().length === 0}
                >
                  Rename session
                </Button>
              </DialogFooter>
            </DialogContent>
          </Dialog>

          <AlertDialog
            open={archiveDialogConversation !== null}
            onOpenChange={(open) => {
              if (!open) {
                setArchiveDialogConversation(null);
              }
            }}
          >
            <AlertDialogContent>
              <AlertDialogHeader>
                <AlertDialogTitle>Archive session?</AlertDialogTitle>
                <AlertDialogDescription>
                  This hides{" "}
                  <span className="font-medium">
                    {archiveDialogConversation?.title || "Untitled agent"}
                  </span>{" "}
                  from the active conversation list. You can restore it later from
                  archived sessions.
                </AlertDialogDescription>
              </AlertDialogHeader>
              <AlertDialogFooter>
                <AlertDialogCancel>Cancel</AlertDialogCancel>
                <AlertDialogAction
                  onClick={() => {
                    if (archiveDialogConversation) {
                      void onArchiveConversation(archiveDialogConversation);
                    }
                    setArchiveDialogConversation(null);
                  }}
                  variant="destructive"
                >
                  Archive session
                </AlertDialogAction>
              </AlertDialogFooter>
            </AlertDialogContent>
          </AlertDialog>

          {(showProjectHeader ? expanded : true) && (
            <div className="mb-2 mt-1 flex flex-col gap-0.5" role="group">
                {visibleRows.map((row) => {
                  const conversation = toProjectAgentConversation(row.conversation);
                  const rowKey = getAgentConversationStoreKey(conversation);
                  const activeConversationId = activeConversationIds[rowKey] ?? null;
                  const agentStatus = agentStatuses[rowKey] ?? "idle";
                  const isSelected = selectedConversationId === conversation.id;
                  const isActiveRuntime = activeConversationId === conversation.id;
                  const isPinned = Boolean(pinnedConversationIds[conversation.id]);
                  const runtimeState = getSessionRuntimeState(
                    conversation,
                    isActiveRuntime,
                    agentStatus
                  );
                  const showRuntimeState = runtimeState === "running";
                  const sessionActionsOpen = openSessionActionsId === conversation.id;

                  return (
                    <AgentSessionRow
                      key={conversation.id}
                      conversation={conversation}
                      projectName={project.name}
                      showProjectNameInMeta={showProjectNameInMeta}
                      refKind={row.refKind}
                      refLabel={row.refLabel}
                      publicationState={row.publicationState}
                      publicationLabel={row.publicationLabel}
                      isSelected={isSelected}
                      isPinned={isPinned}
                      runtimeState={runtimeState}
                      showRuntimeState={showRuntimeState}
                      sessionActionsOpen={sessionActionsOpen}
                      onSelect={() => onSelectConversation(project.id, conversation)}
                      onRename={() => openRenameDialog(conversation)}
                      onTogglePinned={() => onTogglePinnedConversation(conversation.id)}
                      onRestore={() => onRestoreConversation(conversation)}
                      onArchiveRequest={() => setArchiveDialogConversation(conversation)}
                      setActionsTriggerRef={(node) => {
                        sessionActionsTriggerRefs.current[conversation.id] = node;
                      }}
                      onActionsOpenChange={(open) => {
                        setOpenSessionActionsId(open ? conversation.id : null);
                        if (!open) {
                          requestAnimationFrame(() => {
                            sessionActionsTriggerRefs.current[conversation.id]?.blur();
                          });
                        }
                      }}
                    />
                    );
                  })}

                {visibleConversations.length > 0 && groupQuery.hasNextPage && (
                  <div className="flex justify-end py-0.5 pr-2">
                    <button
                      type="button"
                      className="inline-flex items-center text-[0.6719rem] font-medium transition-colors"
                      onClick={() => void groupQuery.fetchNextPage()}
                      disabled={groupQuery.isFetchingNextPage}
                      data-testid={`agents-load-more-${project.id}`}
                      style={{
                        color: "var(--text-muted)",
                        opacity: groupQuery.isFetchingNextPage ? 0.7 : 1,
                      }}
                    >
                      {groupQuery.isFetchingNextPage ? "Loading..." : "Load more"}
                    </button>
                  </div>
                )}

                {groupQuery.isLoading && (
                  <div className="py-1.5 text-[0.6875rem]" style={{ color: "var(--text-muted)" }}>
                    Loading...
                  </div>
                )}
            </div>
          )}
        </div>
    </div>
  );
}

function useDebouncedValue<T>(value: T, delayMs: number): T {
  const [debouncedValue, setDebouncedValue] = useState(value);

  useEffect(() => {
    const timeout = window.setTimeout(() => setDebouncedValue(value), delayMs);
    return () => window.clearTimeout(timeout);
  }, [delayMs, value]);

  return debouncedValue;
}

function StaticRecentRuns() {
  return (
    <div
      className="shrink-0 border-t px-3 pb-1.5 pt-3"
      data-testid="agents-static-recent"
      aria-hidden="true"
      title="Coming soon"
      style={{
        borderColor: "var(--app-sidebar-border)",
        display: "none",
      }}
    >
      <div className="mb-2 flex items-center justify-between px-1">
        <span
          className="text-[0.6562rem] font-semibold uppercase leading-none tracking-[0.12em]"
          style={{ color: "var(--text-muted)" }}
        >
          Recent
        </span>
        <button
          type="button"
          className="rounded-[4px] px-1 text-[0.6875rem] font-medium leading-none outline-none transition-colors hover:text-[var(--text-primary)] focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:2px]"
          style={{ color: "var(--text-muted)", boxShadow: "none" }}
        >
          View all
        </button>
      </div>
      <div className="flex flex-col gap-0.5">
        {STATIC_RECENT_RUNS.map((run) => (
          <button
            type="button"
            key={run.title}
            className="group/recent grid w-full grid-cols-[7px_minmax(0,1fr)_12px] items-center gap-[9px] rounded-[6px] px-2 py-1.5 text-left text-[var(--text-secondary)] transition-colors duration-[120ms] ease-[cubic-bezier(.2,.8,.2,1)] outline-none hover:bg-[var(--bg-elevated)] hover:text-[var(--text-primary)] focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:2px]"
            style={{ boxShadow: "none" }}
          >
            <span
              className="block h-[7px] w-[7px] rounded-full"
              style={{ background: "var(--status-success)" }}
            />
            <span className="min-w-0">
              <span
                className="block whitespace-normal break-words text-[0.7812rem] font-medium leading-[1.4] [text-overflow:clip]"
                style={{
                  overflow: "visible",
                  textOverflow: "clip",
                  whiteSpace: "normal",
                  width: "168px",
                }}
              >
                {run.title}
              </span>
              <span
                className="block truncate text-[0.6562rem] leading-[1.4]"
                style={{
                  color: "var(--text-muted)",
                  fontFamily: "var(--font-mono, ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace)",
                }}
              >
                {run.project}
                <span>{" · "}</span>
                {run.time}
              </span>
            </span>
            <ChevronRight
              aria-hidden="true"
              className="h-3 w-3 opacity-0 transition-opacity duration-[120ms] group-hover/recent:opacity-100"
              style={{ color: "var(--text-subtle)" }}
              strokeWidth={2}
            />
          </button>
        ))}
      </div>
    </div>
  );
}

type SessionRuntimeState = "running" | "queued" | "done" | "blocked" | "archived";

function getSessionRuntimeState(
  conversation: AgentConversation,
  isActiveRuntime: boolean,
  status: string
): SessionRuntimeState {
  if (conversation.archivedAt) {
    return "archived";
  }

  if (!isActiveRuntime || status === "idle") {
    return "queued";
  }

  if (status === "completed") {
    return "done";
  }

  if (status === "failed" || status === "error" || status === "needs_approval") {
    return "blocked";
  }

  return "running";
}

function SessionRuntimeLabel({ state }: { state: SessionRuntimeState }) {
  if (state !== "running") {
    return null;
  }

  return (
    <span className="agents-session-runtime-label font-medium">
      running
    </span>
  );
}

function SessionStatusIcon({
  conversationId,
  isPinned,
  state,
  selected,
}: {
  conversationId: string;
  isPinned: boolean;
  state: SessionRuntimeState;
  selected: boolean;
}) {
  if (isPinned) {
    return (
      <Pin
        aria-hidden="true"
        className="h-3.5 w-3.5"
        data-testid={`agents-pin-icon-${conversationId}`}
        style={{
          color: state === "running" ? "var(--accent-primary)" : "var(--text-subtle)",
        }}
      />
    );
  }

  return <SessionStatusDot state={state} selected={selected} />;
}

function SessionStatusDot({
  state,
}: {
  state: SessionRuntimeState;
  selected: boolean;
}) {
  if (state === "running") {
    return (
      <span
        aria-hidden="true"
        className="block h-[7px] w-[7px] shrink-0 rounded-full"
        style={{
          backgroundColor: "var(--accent-primary)",
          border: "1.5px solid transparent",
        }}
      />
    );
  }

  if (state === "done") {
    return (
      <span
        aria-hidden="true"
        className="block h-[7px] w-[7px] shrink-0 rounded-full"
        style={{
          backgroundColor: "var(--status-success)",
          border: "1.5px solid transparent",
        }}
      />
    );
  }

  if (state === "queued") {
    return (
      <span
        aria-hidden="true"
        className="block h-[7px] w-[7px] shrink-0 rounded-full"
        style={{
          backgroundColor: "transparent",
          borderColor: "var(--text-subtle)",
          borderStyle: "solid",
          borderWidth: "1.5px",
        }}
      />
    );
  }

  return null;
}
