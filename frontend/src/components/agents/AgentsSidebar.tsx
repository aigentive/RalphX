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
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  Folder,
  MoreHorizontal,
  Pencil,
  Plus,
  RotateCcw,
  Search,
  X,
} from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";

import { Button } from "@/components/ui/button";
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
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { useChatStore } from "@/stores/chatStore";
import {
  useAgentSessionStore,
  type AgentProjectSort,
} from "@/stores/agentSessionStore";
import { withAlpha } from "@/lib/theme-colors";
import type { Project } from "@/types/project";
import {
  formatAgentConversationCreatedAt,
  formatAgentConversationCreatedAtTitle,
  getAgentConversationStoreKey,
  type AgentConversation,
} from "./agentConversations";
import { useProjectAgentConversations } from "./useProjectAgentConversations";
import { useArchivedConversationCounts } from "./useArchivedConversationCounts";

const AGENTS_SEARCH_DEBOUNCE_MS = 180;

const PROJECT_SORT_LABELS: Record<AgentProjectSort, string> = {
  latest: "Latest",
  az: "A-Z",
  za: "Z-A",
};

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
  const normalizedSearchInput = searchQuery.trim().toLowerCase();
  const normalizedSearch = useDebouncedValue(
    normalizedSearchInput,
    AGENTS_SEARCH_DEBOUNCE_MS
  );
  const showAllProjects = useAgentSessionStore((s) => s.showAllProjects);
  const setShowAllProjects = useAgentSessionStore((s) => s.setShowAllProjects);
  const projectSort = useAgentSessionStore((s) => s.projectSort);
  const setProjectSort = useAgentSessionStore((s) => s.setProjectSort);
  const shouldHydrateAllSidebarProjects =
    showAllProjects || showArchived || normalizedSearch.length > 0;
  const pinnedProjectId = pinnedConversation?.projectId ?? null;
  const archivedCountProjectIds = useMemo(() => {
    if (shouldHydrateAllSidebarProjects) {
      return projects.map((project) => project.id);
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
  }, [focusedProjectId, pinnedProjectId, projects, shouldHydrateAllSidebarProjects]);
  const { totalArchivedCount } = useArchivedConversationCounts(archivedCountProjectIds);
  const orderedProjects = useMemo(() => {
    if (projectSort === "latest") {
      return projects;
    }

    const sortedProjects = [...projects].sort((a, b) =>
      a.name.localeCompare(b.name, undefined, { sensitivity: "base" })
    );
    return projectSort === "za" ? sortedProjects.reverse() : sortedProjects;
  }, [projectSort, projects]);

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
              borderColor: "var(--overlay-weak)",
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
              placeholder="Search"
              className="w-full h-7 pl-8 pr-8 text-[0.75rem] bg-transparent outline-none ring-0 focus:ring-0 focus:outline-none focus-visible:outline-none border-0"
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
        <div className="flex shrink-0 items-center gap-2 px-3 pb-2">
          <button
            type="button"
            data-testid="agents-show-all-projects-pill"
            aria-pressed={showAllProjects}
            onClick={() => setShowAllProjects(!showAllProjects)}
            className="inline-flex h-7 items-center rounded-[6px] border px-2.5 text-[0.7188rem] font-medium transition-colors duration-[120ms] outline-none hover:border-[var(--border-strong)] hover:text-[var(--text-primary)] focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:2px]"
            style={{
              backgroundColor: showAllProjects
                ? withAlpha("var(--accent-primary)", 15)
                : "var(--bg-elevated)",
              borderColor: showAllProjects ? "var(--accent-primary)" : "var(--border-subtle)",
              color: showAllProjects ? "var(--text-primary)" : "var(--text-muted)",
              boxShadow: "none",
            }}
          >
            All projects
          </button>

          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <button
                type="button"
                data-testid="agents-project-sort-pill"
                aria-label={`Sort projects: ${PROJECT_SORT_LABELS[projectSort]}`}
                className="inline-flex h-7 items-center gap-1.5 rounded-[6px] border px-2.5 text-[0.7188rem] font-medium transition-colors duration-[120ms] outline-none hover:border-[var(--border-strong)] hover:text-[var(--text-primary)] focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:2px]"
                style={{
                  backgroundColor: "var(--bg-elevated)",
                  borderColor: "var(--border-subtle)",
                  color: "var(--text-muted)",
                  boxShadow: "none",
                }}
              >
                <span>{PROJECT_SORT_LABELS[projectSort]}</span>
                <ChevronDown className="h-3 w-3" />
              </button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="start" className="min-w-[120px]">
              <DropdownMenuRadioGroup
                value={projectSort}
                onValueChange={(value) => setProjectSort(value as AgentProjectSort)}
              >
                {(["latest", "az", "za"] as AgentProjectSort[]).map((sort) => (
                  <DropdownMenuRadioItem key={sort} value={sort} className="text-xs">
                    {PROJECT_SORT_LABELS[sort]}
                  </DropdownMenuRadioItem>
                ))}
              </DropdownMenuRadioGroup>
            </DropdownMenuContent>
          </DropdownMenu>

          {(showArchived || totalArchivedCount > 0) && (
            <button
              type="button"
              data-testid="agents-show-archived-pill"
              aria-pressed={showArchived}
              aria-label="Show archived sessions"
              onClick={() => onShowArchivedChange(!showArchived)}
              className="ml-auto inline-flex h-7 items-center gap-1.5 rounded-[6px] border px-2.5 text-[0.7188rem] font-medium transition-colors duration-[120ms] outline-none hover:border-[var(--border-strong)] hover:text-[var(--text-primary)] focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:2px]"
              style={{
                backgroundColor: showArchived
                  ? withAlpha("var(--accent-primary)", 15)
                  : "var(--bg-elevated)",
                borderColor: showArchived ? "var(--accent-primary)" : "var(--border-subtle)",
                color: showArchived ? "var(--text-primary)" : "var(--text-muted)",
                boxShadow: "none",
              }}
            >
              <span>Archived</span>
              <span
                className="rounded-full px-1.5 text-[0.625rem] font-semibold leading-[1.6]"
                style={{
                  backgroundColor: "var(--overlay-weak)",
                  color: "var(--text-secondary)",
                }}
              >
                {totalArchivedCount}
              </span>
            </button>
          )}
        </div>
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
        ) : (
          orderedProjects.map((project) => (
            <ProjectSessionGroup
              key={project.id}
              project={project}
              isFocused={focusedProjectId === project.id}
              selectedConversationId={selectedConversationId}
              pinnedConversation={
                pinnedConversation?.projectId === project.id ? pinnedConversation : null
              }
              searchQuery={normalizedSearch}
              onFocusProject={onFocusProject}
              onSelectConversation={onSelectConversation}
              onArchiveProject={onArchiveProject}
              onRenameConversation={onRenameConversation}
              onArchiveConversation={onArchiveConversation}
              onRestoreConversation={onRestoreConversation}
              showArchived={showArchived}
              showAllProjects={showAllProjects}
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

interface ProjectSessionGroupProps {
  project: Project;
  isFocused: boolean;
  selectedConversationId: string | null;
  pinnedConversation: AgentConversation | null;
  searchQuery: string;
  onFocusProject: (projectId: string) => void;
  onSelectConversation: (projectId: string, conversation: AgentConversation) => void;
  onArchiveProject: (projectId: string) => void | Promise<void>;
  onRenameConversation: (conversationId: string, title: string) => void | Promise<void>;
  onArchiveConversation: (conversation: AgentConversation) => void;
  onRestoreConversation: (conversation: AgentConversation) => void;
  showArchived: boolean;
  showAllProjects: boolean;
}

function ProjectSessionGroup({
  project,
  isFocused,
  selectedConversationId,
  pinnedConversation,
  searchQuery,
  onFocusProject,
  onSelectConversation,
  onArchiveProject,
  onRenameConversation,
  onArchiveConversation,
  onRestoreConversation,
  showArchived,
  showAllProjects,
}: ProjectSessionGroupProps) {
  const projectActionsTriggerRef = useRef<HTMLButtonElement | null>(null);
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
  const shouldEnableConversationQuery =
    showAllProjects ||
    showArchived ||
    isFocused ||
    Boolean(pinnedConversation) ||
    searchQuery.length > 0;
  const conversations = useProjectAgentConversations(project.id, showArchived, {
    search: searchQuery,
    enabled: shouldEnableConversationQuery,
  });
  const activeConversationIds = useChatStore((s) => s.activeConversationIds);
  const agentStatuses = useChatStore((s) => s.agentStatus);
  const visibleConversations = useMemo(() => {
    const items = conversations.data ?? [];
    if (
      !pinnedConversation ||
      items.some((conversation) => conversation.id === pinnedConversation.id)
    ) {
      return items;
    }
    return [pinnedConversation, ...items];
  }, [conversations.data, pinnedConversation]);
  const totalConversationCount = conversations.total;
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
    !conversations.isLoading &&
    visibleConversations.length === 0 &&
    (showArchived || searchQuery.length > 0 || !showAllProjects)
  ) {
    return null;
  }

  return (
    <div className="my-1 flex flex-col gap-0.5" data-testid={`agents-project-${project.id}`}>
        <div className="group/project relative">
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
                    : "opacity-100 group-hover/project:opacity-0 group-focus-within/project:opacity-0"
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
                    : "opacity-100 group-hover/project:opacity-0 group-focus-within/project:opacity-0"
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
                  : "opacity-0 group-hover/project:opacity-100 group-focus-within/project:opacity-100"
              }`}
              data-testid={`agents-project-actions-${project.id}`}
              onClick={(event) => event.stopPropagation()}
            >
              <DropdownMenu
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

          {expanded && (
            <div className="mb-2 mt-1 flex flex-col gap-0.5" role="group">
                {visibleConversations.map((conversation) => {
                  const rowKey = getAgentConversationStoreKey(conversation);
                  const activeConversationId = activeConversationIds[rowKey] ?? null;
                  const agentStatus = agentStatuses[rowKey] ?? "idle";
                  const isSelected = selectedConversationId === conversation.id;
                  const isActiveRuntime = activeConversationId === conversation.id;
                  const title = conversation.title || "Untitled agent";
                  const createdLabel = formatAgentConversationCreatedAt(conversation.createdAt);
                  const createdTitle = formatAgentConversationCreatedAtTitle(conversation.createdAt);
                  const branchLabel = project.baseBranch ?? "master";
                  const runtimeState = getSessionRuntimeState(
                    conversation,
                    isActiveRuntime,
                    agentStatus
                  );
                  const showRuntimeState = runtimeState === "running";
                  const sessionActionsOpen = openSessionActionsId === conversation.id;

                  return (
                    <div
                      key={conversation.id}
                      className="group/session relative"
                      data-testid={`agents-session-${conversation.id}`}
                    >
                      <button
                        type="button"
                        className="agents-session-row grid w-full min-w-0 grid-cols-[minmax(0,1fr)_auto] items-center gap-2 rounded-[6px] px-2.5 py-1.5 pr-8 text-left transition-colors duration-[120ms] ease-[cubic-bezier(.2,.8,.2,1)] outline-none hover:bg-[var(--bg-elevated)] focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:2px]"
                        onClick={() => onSelectConversation(project.id, conversation)}
                        aria-current={isSelected ? "true" : undefined}
                        style={{
                          opacity: conversation.archivedAt ? 0.58 : 1,
                          boxShadow: "none",
                        }}
                      >
                        <span className="min-w-0 flex flex-col gap-px">
                          <span
                            className="agents-session-title min-w-0 truncate text-[0.8125rem] leading-[1.35] tracking-[-0.005em]"
                          >
                            {title}
                          </span>
                          <span
                            className="agents-session-meta min-w-0 truncate text-[0.6875rem] leading-[1.35]"
                            style={{
                              fontFamily: "var(--font-mono, ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace)",
                            }}
                          >
                            <span>{branchLabel}</span>
                            <span>{" · "}</span>
                            <span title={createdTitle || undefined}>{createdLabel}</span>
                            {showRuntimeState && (
                              <>
                                <span>{" · "}</span>
                                <SessionRuntimeLabel state={runtimeState} />
                              </>
                            )}
                          </span>
                        </span>
                        <span
                          className={`agents-session-status-slot transition-opacity duration-150 ${
                            sessionActionsOpen
                              ? "opacity-0"
                              : "opacity-100 group-hover/session:opacity-0 group-focus-within/session:opacity-0"
                          }`}
                        >
                          <SessionStatusDot state={runtimeState} selected={isSelected} />
                        </span>
                      </button>
                      <DropdownMenu
                        onOpenChange={(open) =>
                          setOpenSessionActionsId(open ? conversation.id : null)
                        }
                      >
                        <DropdownMenuTrigger asChild>
                          <Button
                            type="button"
                            variant="ghost"
                            size="sm"
                            className="absolute right-1 top-1/2 h-6 w-6 -translate-y-1/2 rounded-[6px] border-0 p-0 opacity-0 outline-none ring-0 transition-opacity focus:outline-none focus:ring-0 focus-visible:outline-none focus-visible:ring-0 group-hover/session:opacity-100 group-focus-within/session:opacity-100 data-[state=open]:opacity-100"
                            aria-label="Session actions"
                            style={{ boxShadow: "none" }}
                          >
                            <MoreHorizontal className="h-3.5 w-3.5" />
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
                    );
                  })}

                {visibleConversations.length > 0 && conversations.hasNextPage && (
                  <div className="py-0.5">
                    <button
                      type="button"
                      className="inline-flex items-center pl-[26px] text-[0.6719rem] font-medium transition-colors"
                      onClick={() => void conversations.fetchNextPage()}
                      disabled={conversations.isFetchingNextPage}
                      data-testid={`agents-load-more-${project.id}`}
                      style={{
                        color: "var(--text-muted)",
                        opacity: conversations.isFetchingNextPage ? 0.7 : 1,
                      }}
                    >
                      {conversations.isFetchingNextPage ? "Loading..." : "Load more"}
                    </button>
                  </div>
                )}

                {conversations.isLoading && (
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
              className="h-[7px] w-[7px] rounded-full"
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
        className="h-[7px] w-[7px] shrink-0 rounded-full"
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
        className="h-[7px] w-[7px] shrink-0 rounded-full"
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
        className="h-[7px] w-[7px] shrink-0 rounded-full"
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
