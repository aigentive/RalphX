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
  AlertTriangle,
  Archive,
  Bot,
  CheckCircle2,
  ChevronLeft,
  ChevronDown,
  ChevronRight,
  Circle,
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
  XCircle,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

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
import { useAgentSidebarPublicationPolling } from "./useAgentSidebarPublicationPolling";
import { useAgentSidebarRunningStates } from "./useAgentSidebarRunningStates";
import { useArchivedConversationCounts } from "./useArchivedConversationCounts";

const PROJECT_SORT_LABELS: Record<AgentProjectSort, string> = {
  latest: "Latest",
  az: "A-Z",
  za: "Z-A",
};
const AGENTS_SEARCH_DEBOUNCE_MS = 180;

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
  isVisible?: boolean;
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
  isVisible = true,
  onCollapse,
}: AgentsSidebarProps) {
  const [isSearchOpen, setIsSearchOpen] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");
  const showAllProjects = useAgentSessionStore((s) => s.showAllProjects);
  const projectSort = useAgentSessionStore((s) => s.projectSort);
  const setShowAllProjects = useAgentSessionStore((s) => s.setShowAllProjects);
  const setProjectSort = useAgentSessionStore((s) => s.setProjectSort);
  const normalizedSearchInput = searchQuery.trim().toLowerCase();
  const normalizedSearch = useDebouncedValue(
    normalizedSearchInput,
    AGENTS_SEARCH_DEBOUNCE_MS
  );
  const pinnedProjectId = pinnedConversation?.projectId ?? null;
  const shouldHydrateAllSidebarProjects =
    showAllProjects || showArchived || normalizedSearch.length > 0;
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

    return projects
      .filter((project) => projectIds.has(project.id))
      .map((project) => project.id);
  }, [
    focusedProjectId,
    pinnedProjectId,
    projects,
    shouldHydrateAllSidebarProjects,
  ]);
  const { totalArchivedCount } = useArchivedConversationCounts(archivedCountProjectIds);
  const orderedProjects = useMemo(() => {
    if (projectSort === "latest") {
      return projects.filter((project) => selectedProjectFilterSet.has(project.id));
    }

    const sorted = [...projects].sort((left, right) =>
      left.name.localeCompare(right.name, undefined, { sensitivity: "base" })
    );

    return projectSort === "za" ? sorted.reverse() : sorted;
  }, [projectSort, projects]);

  return (
    <aside
      className="w-full h-full flex flex-col border-r overflow-hidden"
      style={{
        background: "color-mix(in srgb, var(--bg-surface) 92%, transparent)",
        backdropFilter: "blur(20px) saturate(180%)",
        WebkitBackdropFilter: "blur(20px) saturate(180%)",
        borderColor: "var(--overlay-faint)",
      }}
      data-testid="agents-sidebar"
    >
      <div
        className="px-3.5 pt-3.5 pb-2.5 flex items-center gap-2 shrink-0"
        style={{
          borderColor: "var(--overlay-faint)",
        }}
      >
        <Bot className="w-4 h-4 shrink-0" style={{ color: "var(--accent-primary)" }} />
        <span className="text-[14px] font-semibold tracking-[-0.01em] truncate" style={{ color: "var(--text-primary)" }}>
          Projects
        </span>
        <div className="ml-auto flex items-center gap-1">
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                type="button"
                variant="ghost"
                size="sm"
                className="h-7 w-7 p-0 rounded-md border-0 outline-none ring-0 focus:ring-0 focus:outline-none focus-visible:outline-none focus-visible:ring-0"
                onClick={onCreateAgent}
                aria-label="New agent"
                data-testid="agents-new-agent"
                style={{ boxShadow: "none" }}
              >
                <Plus className="w-4 h-4" />
              </Button>
            </TooltipTrigger>
            <TooltipContent side="bottom" className="text-xs">
              New agent
            </TooltipContent>
          </Tooltip>
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                type="button"
                variant="ghost"
                size="sm"
                className="h-7 w-7 p-0 rounded-md border-0 outline-none ring-0 focus:ring-0 focus:outline-none focus-visible:outline-none focus-visible:ring-0"
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
                style={{ boxShadow: "none" }}
              >
                {isSearchOpen ? <X className="w-4 h-4" /> : <Search className="w-4 h-4" />}
              </Button>
            </TooltipTrigger>
            <TooltipContent side="bottom" className="text-xs">
              {isSearchOpen ? "Close search" : "Search"}
            </TooltipContent>
          </Tooltip>
          {onCollapse && (
            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  className="h-7 w-7 p-0 rounded-md border-0 outline-none ring-0 focus:ring-0 focus:outline-none focus-visible:outline-none focus-visible:ring-0"
                  onClick={onCollapse}
                  aria-label="Collapse sidebar"
                  data-testid="agents-sidebar-collapse-button"
                  style={{ boxShadow: "none" }}
                >
                  <ChevronLeft className="w-4 h-4" />
                </Button>
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
              background: "var(--overlay-faint)",
              border: "1px solid var(--overlay-weak)",
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
              placeholder="Search"
              className="w-full h-7 pl-8 pr-8 text-[12px] bg-transparent outline-none ring-0 focus:ring-0 focus:outline-none focus-visible:outline-none border-0"
              style={{
                color: "var(--text-primary)",
                caretColor: "var(--accent-primary)",
              }}
              autoFocus
              data-testid="agents-search-input"
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
        <div className="px-3.5 pb-2 flex items-center gap-2 shrink-0">
          <button
            type="button"
            onClick={() => setShowAllProjects(!showAllProjects)}
            data-testid="agents-show-all-projects-pill"
            className="h-7 inline-flex items-center rounded-full border px-2.5 text-[11px] font-medium transition-colors outline-none ring-0 focus:outline-none focus-visible:outline-none"
            style={{
              color: showAllProjects ? "var(--text-primary)" : "var(--text-secondary)",
              background: showAllProjects
                ? withAlpha("var(--accent-primary)", 12)
                : "transparent",
              borderColor: showAllProjects ? withAlpha("var(--accent-primary)", 30) : "var(--overlay-weak)",
            }}
          >
            All projects
          </button>

          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <button
                type="button"
                data-testid="agents-project-sort-pill"
                className="h-7 inline-flex items-center gap-1.5 rounded-full border px-2.5 text-[11px] font-medium transition-colors outline-none ring-0 focus:outline-none focus-visible:outline-none"
                style={{
                  color: "var(--text-secondary)",
                  background: "transparent",
                  borderColor: "var(--overlay-weak)",
                }}
              >
                {PROJECT_SORT_LABELS[projectSort]}
                <ChevronDown className="h-3.5 w-3.5" />
              </button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="start">
              <DropdownMenuRadioGroup
                value={projectSort}
                onValueChange={(value) => setProjectSort(value as AgentProjectSort)}
              >
                {Object.entries(PROJECT_SORT_LABELS).map(([value, label]) => (
                  <DropdownMenuRadioItem key={value} value={value} className="text-xs">
                    {label}
                  </DropdownMenuRadioItem>
                ))}
              </DropdownMenuRadioGroup>
            </DropdownMenuContent>
          </DropdownMenu>

          {(showArchived || totalArchivedCount > 0) && (
            <button
              type="button"
              onClick={() => onShowArchivedChange(!showArchived)}
              data-testid="agents-show-archived-pill"
              className="h-7 inline-flex items-center gap-1.5 rounded-full border px-2.5 text-[11px] font-medium transition-colors outline-none ring-0 focus:outline-none focus-visible:outline-none"
              style={{
                color: showArchived ? "var(--text-primary)" : "var(--text-secondary)",
                background: showArchived
                  ? withAlpha("var(--accent-primary)", 12)
                  : "transparent",
                borderColor: showArchived
                  ? withAlpha("var(--accent-primary)", 30)
                  : "var(--overlay-weak)",
              }}
            >
              Archived
              <span
                className="text-[10px] font-semibold leading-none"
                style={{
                  color: showArchived ? "var(--accent-primary)" : "var(--text-muted)",
                }}
              >
                {totalArchivedCount}
              </span>
            </button>
          )}
        </div>
      )}

      <div className="flex-1 overflow-y-auto py-1.5 border-t" style={{ borderColor: "var(--overlay-faint)" }}>
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
            isSidebarVisible={isVisible}
            priorityConversationIds={priorityConversationIds}
            pinnedConversationIds={pinnedConversationIds}
            rowSort={projectSort}
            selectedConversationId={selectedConversationId}
            searchQuery={normalizedSearch}
            selectedPublicationStates={selectedPublicationStates}
            onArchiveConversation={onArchiveConversation}
            onRenameConversation={onRenameConversation}
            onRestoreConversation={onRestoreConversation}
            onSelectConversation={handleSelectVisibleConversation}
            onTogglePinnedConversation={togglePinnedConversation}
            showArchived={showArchived}
          />
        ) : (
          orderedProjects.map((project) => (
            <ProjectSessionGroup
              key={project.id}
              project={project}
              isFocused={focusedProjectId === project.id}
              isSidebarVisible={isVisible}
              selectedConversationId={selectedConversationId}
              searchQuery={normalizedSearch}
              onFocusProject={onFocusProject}
              onSelectConversation={handleSelectVisibleConversation}
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

      <div
        className="px-3.5 py-3 border-t shrink-0"
        style={{ borderColor: "var(--overlay-faint)" }}
      >
        <button
          type="button"
          onClick={onCreateProject}
          data-testid="agents-add-project"
          className="w-full h-10 inline-flex items-center justify-center gap-2 rounded-xl border border-dashed text-[12px] font-medium transition-colors outline-none ring-0 focus:outline-none focus-visible:outline-none"
          style={{
            color: "var(--text-secondary)",
            borderColor: "var(--overlay-weak)",
            background: "transparent",
          }}
        >
          <Plus className="w-4 h-4" />
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
            className="inline-flex h-full min-w-0 shrink-0 items-center gap-1.5 rounded-[4px] border border-transparent bg-transparent px-2 text-[0.7188rem] font-medium text-[var(--text-muted)] transition-colors duration-[120ms] outline-none hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)] focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:-2px]"
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
            className="inline-flex h-full min-w-0 shrink-0 items-center gap-1.5 rounded-[4px] border border-transparent bg-transparent px-2 text-[0.7188rem] font-medium text-[var(--text-muted)] transition-colors duration-[120ms] outline-none hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)] focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:-2px]"
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
  isSidebarVisible: boolean;
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
  isSidebarVisible,
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
  const handleSelectedConversationPublicationState = useCallback(
    (publicationState: AgentSidebarPublicationState) => {
      setExpandedPublicationState((current) =>
        current === publicationState ? current : publicationState
      );
    },
    []
  );

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
          isSidebarVisible={isSidebarVisible}
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
          onSelectedConversationPublicationState={
            handleSelectedConversationPublicationState
          }
        />
      ))}
    </>
  );
}

interface PublicationStateGroupProps {
  expandedPublicationState: AgentSidebarPublicationState | null;
  isSidebarVisible: boolean;
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
  onSelectedConversationPublicationState: (
    publicationState: AgentSidebarPublicationState
  ) => void;
}

function PublicationStateGroup({
  expandedPublicationState,
  isSidebarVisible,
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
  onSelectedConversationPublicationState,
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
  const visibleConversations = useMemo(
    () => groupQuery.group.rows.map((row) => toProjectAgentConversation(row.conversation)),
    [groupQuery.group.rows]
  );
  const selectedConversationInGroup = useMemo(
    () =>
      selectedConversationId !== null &&
      groupQuery.group.rows.some(
        (row) => row.conversation.id === selectedConversationId
      ),
    [groupQuery.group.rows, selectedConversationId]
  );
  useEffect(() => {
    if (selectedConversationInGroup) {
      onSelectedConversationPublicationState(publicationState);
    }
  }, [
    onSelectedConversationPublicationState,
    publicationState,
    selectedConversationInGroup,
  ]);
  useAgentSidebarRunningStates(visibleConversations, isSidebarVisible && expanded);
  const publicationCurrentStates = useMemo(() => {
    const map = new Map<string, string>();
    for (const row of groupQuery.group.rows) {
      map.set(row.conversation.id, row.publicationState);
    }
    return map;
  }, [groupQuery.group.rows]);
  useAgentSidebarPublicationPolling(
    visibleConversations,
    isSidebarVisible && expanded,
    publicationCurrentStates
  );

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
            <AlertDialogDescription asChild>
              <div>
                <p>
                  This hides{" "}
                  <span className="font-medium">
                    {archiveDialogConversation?.title || "Untitled agent"}
                  </span>{" "}
                  from the active conversation list. You can restore it later
                  from archived sessions.
                </p>
                {archiveDialogConversation?.contextType === "project" && (
                  <p className="mt-2 text-text-muted">
                    Any open PR will be closed. The local workspace branch will
                    be cleaned up on next app restart.
                  </p>
                )}
              </div>
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
  isSidebarVisible: boolean;
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
  isSidebarVisible,
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
  const [projectActionsOpen, setProjectActionsOpen] = useState(false);
  const [archiveDialogOpen, setArchiveDialogOpen] = useState(false);
  const [renameDialogConversation, setRenameDialogConversation] =
    useState<AgentConversation | null>(null);
  const [renameDraftTitle, setRenameDraftTitle] = useState("");
  const [archiveDialogConversation, setArchiveDialogConversation] =
    useState<AgentConversation | null>(null);
  const expanded = useAgentSessionStore((s) => s.expandedProjectIds[project.id] ?? true);
  const toggleProjectExpanded = useAgentSessionStore((s) => s.toggleProjectExpanded);
  const shouldEnableConversationQuery =
    showAllProjects ||
    showArchived ||
    isFocused ||
    Boolean(pinnedConversation) ||
    searchQuery.length > 0;
  const conversations = useProjectAgentConversations(project.id, showArchived, {
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
  useAgentSidebarRunningStates(
    visibleConversations,
    isSidebarVisible && (showProjectHeader ? expanded : true)
  );
  const projectPublicationCurrentStates = useMemo(() => {
    const map = new Map<string, string>();
    for (const row of visibleRows) {
      map.set(row.conversation.id, row.publicationState);
    }
    return map;
  }, [visibleRows]);
  useAgentSidebarPublicationPolling(
    visibleConversations,
    isSidebarVisible && (showProjectHeader ? expanded : true),
    projectPublicationCurrentStates
  );
  const totalConversationCount = groupQuery.group.total;
  const activeRuntimeCount = visibleConversations.filter((conversation) => {
    const rowKey = getAgentConversationStoreKey(conversation);
    return (
      activeConversationIds[rowKey] === conversation.id &&
      (agentStatuses[rowKey] ?? "idle") !== "idle"
    );
  }).length;
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
    (showArchived ||
      searchQuery.length > 0 ||
      !showAllProjects)
  ) {
    return null;
  }

  return (
    <div className="mt-1.5" data-testid={`agents-project-${project.id}`}>
      <div className="px-3">
        <div className="group/project">
          <div
            className="w-full min-h-8 px-1.5 py-1 flex items-center gap-1.5 rounded-md transition-colors duration-150"
            style={{
              color: isFocused ? "var(--text-primary)" : "var(--text-muted)",
              background: "transparent",
            }}
          >
            <button
              type="button"
              className="h-4.5 w-4.5 flex items-center justify-center rounded outline-none ring-0 focus:ring-0 focus:outline-none focus-visible:outline-none shrink-0"
              onClick={() => toggleProjectExpanded(project.id)}
              aria-label={expanded ? "Collapse project" : "Expand project"}
            >
              {expanded ? (
                <ChevronDown className="w-4 h-4" />
              ) : (
                <ChevronRight className="w-4 h-4" />
              )}
            </button>
            <button
              type="button"
              className="min-w-0 flex-1 flex items-center gap-2 bg-transparent border-0 p-0 text-left shadow-none outline-none ring-0 focus:ring-0 focus:outline-none focus-visible:outline-none focus-visible:ring-0"
              onClick={() => onFocusProject(project.id)}
              style={{ boxShadow: "none" }}
            >
              <Folder className="w-3.5 h-3.5 shrink-0" />
              <span className="min-w-0 flex-1 flex items-center gap-2">
                <span className="text-[11px] font-semibold tracking-[-0.01em] truncate">
                  {project.name}
                </span>
                {totalConversationCount > 0 && (
                  <span
                    className="shrink-0 text-[10px] font-medium leading-none"
                    style={{
                      color: isFocused ? "var(--accent-primary)" : "var(--text-muted)",
                    }}
                  >
                    {totalConversationCount}
                  </span>
                )}
              </span>
            </button>
            {!expanded && activeRuntimeCount > 0 && (
              <span
                className="text-[10px] px-1.5 rounded-full font-medium leading-[16px]"
                style={{
                  color: "var(--accent-primary)",
                  background: withAlpha("var(--accent-primary)", 15),
                }}
              >
                {activeRuntimeCount}
              </span>
            )}
            <div
              className={`flex items-center gap-0.5 transition-opacity duration-150 ${
                projectActionsOpen
                  ? "opacity-100"
                  : "opacity-0 group-hover/project:opacity-100 group-focus-within/project:opacity-100"
              }`}
              data-testid={`agents-project-actions-${project.id}`}
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
                    className="h-5.5 w-5.5 p-0 rounded border-0 outline-none ring-0 focus:ring-0 focus:outline-none focus-visible:outline-none focus-visible:ring-0"
                    aria-label="Project actions"
                    data-theme-button-skip="true"
                    style={{ boxShadow: "none" }}
                  >
                    <MoreHorizontal className="w-3.5 h-3.5" />
                  </Button>
                </DropdownMenuTrigger>
                <DropdownMenuContent align="end">
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
                  from the active conversation list. You can restore it later from the
                  archived filter.
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
            <div className="mt-0.5 ml-5 space-y-0.5">
                {visibleConversations.map((conversation) => {
                  const rowKey = getAgentConversationStoreKey(conversation);
                  const activeConversationId = activeConversationIds[rowKey] ?? null;
                  const agentStatus = agentStatuses[rowKey] ?? "idle";
                  const isSelected = selectedConversationId === conversation.id;
                  const isActiveRuntime = activeConversationId === conversation.id;
                  const title = conversation.title || "Untitled agent";
                  const createdLabel = formatAgentConversationCreatedAt(conversation.createdAt);
                  const createdTitle = formatAgentConversationCreatedAtTitle(conversation.createdAt);
                  const statusLabel = conversation.archivedAt
                    ? `Archived * ${createdLabel}`
                    : createdLabel;

                  return (
                    <AgentSessionRow
                      key={conversation.id}
                      className="group/session"
                      data-testid={`agents-session-${conversation.id}`}
                    >
                      <div
                        className="w-full min-h-[30px] px-1.5 py-1 flex items-center gap-1.5 cursor-pointer rounded-md transition-all duration-150 ease-out"
                        style={{
                          color: isSelected ? "var(--text-primary)" : "var(--text-secondary)",
                          background: isSelected
                            ? withAlpha("var(--accent-primary)", 6)
                            : "transparent",
                          opacity: conversation.archivedAt ? 0.58 : 1,
                        }}
                        >
                          <button
                            type="button"
                            className="min-w-0 flex-1 flex items-center gap-1.5 bg-transparent border-0 p-0 text-left shadow-none outline-none ring-0 focus:ring-0 focus:outline-none focus-visible:outline-none focus-visible:ring-0"
                            onClick={() => onSelectConversation(project.id, conversation)}
                            style={{ boxShadow: "none" }}
                          >
                            <span
                              className="w-3.5 h-3.5 flex items-center justify-center shrink-0"
                              style={{
                                color: isSelected ? "var(--accent-primary)" : "var(--text-muted)",
                              }}
                          >
                            <SessionStateGlyph
                              isSelected={isSelected}
                              isActiveRuntime={isActiveRuntime}
                              status={agentStatus}
                              />
                            </span>
                            <span className="min-w-0 flex-1 flex items-baseline gap-2 leading-none">
                              <span className="min-w-0 truncate text-[10.75px] font-medium tracking-[-0.01em]">
                                {title}
                              </span>
                              <span
                                className="shrink-0 text-[10px]"
                                title={createdTitle || undefined}
                                style={{ color: "var(--text-muted)" }}
                              >
                                {statusLabel}
                              </span>
                            </span>
                          </button>
                          <DropdownMenu>
                            <DropdownMenuTrigger asChild>
                              <Button
                                type="button"
                                variant="ghost"
                                size="sm"
                                className="h-5.5 w-5.5 p-0 rounded shrink-0 border-0 outline-none ring-0 focus:ring-0 focus:outline-none focus-visible:outline-none focus-visible:ring-0 opacity-0 group-hover/session:opacity-100 data-[state=open]:opacity-100"
                                aria-label="Session actions"
                                style={{
                                  boxShadow: "none",
                                  ...(isSelected ? { opacity: 1 } : {}),
                                }}
                              >
                                <MoreHorizontal className="w-3.5 h-3.5" />
                              </Button>
                            </DropdownMenuTrigger>
                            <DropdownMenuContent align="end">
                              <DropdownMenuItem
                                className="gap-2 text-xs"
                                onClick={() => openRenameDialog(conversation)}
                              >
                                <Pencil className="w-3.5 h-3.5" />
                                Rename session
                              </DropdownMenuItem>
                              <DropdownMenuSeparator />
                              {conversation.archivedAt ? (
                                <DropdownMenuItem
                                  className="gap-2 text-xs"
                                  onClick={() => onRestoreConversation(conversation)}
                                >
                                  <RotateCcw className="w-3.5 h-3.5" />
                                  Restore session
                                </DropdownMenuItem>
                              ) : (
                                <DropdownMenuItem
                                  className="gap-2 text-xs"
                                  onClick={() => setArchiveDialogConversation(conversation)}
                                >
                                  <Archive className="w-3.5 h-3.5" />
                                  Archive session
                                </DropdownMenuItem>
                              )}
                            </DropdownMenuContent>
                          </DropdownMenu>
                        </div>
                      </div>
                    );
                  })}

                {visibleConversations.length > 0 && groupQuery.hasNextPage && (
                  <div className="flex justify-end py-0.5 pr-2">
                    <button
                      type="button"
                      className="inline-flex items-center pl-[26px] text-[10.75px] font-medium transition-colors"
                      onClick={() => void conversations.fetchNextPage()}
                      disabled={conversations.isFetchingNextPage}
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

                {conversations.isLoading && (
                  <div className="py-1.5 text-[11px]" style={{ color: "var(--text-muted)" }}>
                    Loading...
                  </div>
                )}
            </div>
          )}
        </div>
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

function SessionStateGlyph({
  isSelected,
  isActiveRuntime,
  status,
}: {
  isSelected: boolean;
  isActiveRuntime: boolean;
  status: string;
}) {
  if (isActiveRuntime) {
    if (status === "needs_approval") {
      return (
        <AlertTriangle
          className="w-3 h-3 shrink-0"
          style={{ color: "var(--status-warning)" }}
        />
      );
    }

    if (status === "failed" || status === "error") {
      return (
        <XCircle
          className="w-3 h-3 shrink-0"
          style={{ color: "var(--status-error)" }}
        />
      );
    }

    if (status === "completed") {
      return (
        <CheckCircle2
          className="w-3 h-3 shrink-0"
          style={{ color: "var(--status-success)" }}
        />
      );
    }

    if (status !== "idle") {
      return (
        <Circle
          className="w-2.5 h-2.5 shrink-0 fill-current"
          style={{ color: "var(--status-info)" }}
        />
      );
    }
  }

  return (
    <Circle
      className="w-2.5 h-2.5 shrink-0 fill-current"
      style={{ color: isSelected ? "var(--accent-primary)" : "var(--text-muted)" }}
    />
  );
}
