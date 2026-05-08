/**
 * PlanBrowser - macOS Tahoe styled sidebar for ideation plans
 *
 * Design: Native macOS sidebar with frosted glass, refined typography,
 * and smooth spring animations. Warm orange accent (#ff6b35).
 *
 * Five semantic groups: Drafts, In Progress, Accepted, Done, Archived.
 * Uses server-side paginated queries per group, lazy-loaded on expand
 * with infinite scroll. Groups default to collapsed except In Progress.
 */

import { useState, useRef, useEffect, useCallback } from "react";
import { EmptyState } from "@/components/ui/empty-state";
import {
  ChevronLeft,
  MessageSquare,
  Plus,
  Search,
  X,
  Loader2,
  Pencil,
  Zap,
  CheckCircle,
  CircleCheck,
  Archive,
} from "lucide-react";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import type { IdeationSessionWithProgress } from "@/types/ideation";
import { ideationApi } from "@/api/ideation";
import { PlanItem } from "./PlanItem";
import type { SessionGroup } from "./planBrowserUtils";
import { GroupSection } from "./GroupSection";
import { useSessionGroupCounts } from "@/hooks/useIdeation";
import { usePlanBrowserSearch } from "@/hooks/usePlanBrowserSearch";

// ============================================================================
// Types
// ============================================================================

interface PlanBrowserProps {
  projectId: string;
  currentPlanId: string | null;
  onSelectPlan: (planId: string) => void;
  onNewPlan: () => void;
  onArchivePlan?: (planId: string) => void;
  onReopenPlan?: (planId: string) => void;
  onResetReacceptPlan?: (planId: string) => void;
  width?: number;
  onCollapse?: () => void;
}

// ============================================================================
// Group Config
// ============================================================================

const GROUP_CONFIG: {
  key: SessionGroup;
  label: string;
  icon: typeof Pencil;
  accentColor?: string;
}[] = [
  { key: "drafts", label: "Drafts", icon: Pencil },
  { key: "in-progress", label: "In Progress", icon: Zap, accentColor: "var(--accent-primary)" },
  { key: "accepted", label: "Accepted", icon: CheckCircle, accentColor: "var(--status-success)" },
  { key: "done", label: "Done", icon: CircleCheck, accentColor: "var(--text-muted)" },
  { key: "archived", label: "Archived", icon: Archive, accentColor: "var(--text-muted)" },
];

// ============================================================================
// Component
// ============================================================================

export function PlanBrowser({
  projectId,
  currentPlanId,
  onSelectPlan,
  onNewPlan,
  onArchivePlan,
  onReopenPlan,
  onResetReacceptPlan,
  width = 340,
  onCollapse,
}: PlanBrowserProps) {
  // Default expand state: all collapsed except In Progress (if count > 0)
  const [groupOpen, setGroupOpen] = useState<Record<SessionGroup, boolean>>(() => ({
    drafts: true, // always show drafts flat (no header toggle)
    "in-progress": false, // will be updated once counts load
    accepted: false,
    done: false,
    archived: false,
  }));

  const {
    searchTerm,
    debouncedSearch,
    isSearchActive,
    isSearchLoading: isDebouncePending,
    handleSearchChange,
    handleSearchClear,
  } = usePlanBrowserSearch(groupOpen, setGroupOpen);

  const { data: counts, isFetching: isCountsFetching } = useSessionGroupCounts(projectId, debouncedSearch || undefined);

  const isSearchLoading = isDebouncePending || (isSearchActive && isCountsFetching);

  const totalCount = counts
    ? counts.drafts + counts.inProgress + counts.accepted + counts.done + counts.archived
    : 0;

  // Open In Progress automatically once counts load and inProgress > 0
  const countsLoadedRef = useRef(false);
  useEffect(() => {
    if (counts && !countsLoadedRef.current) {
      countsLoadedRef.current = true;
      if (counts.inProgress > 0) {
        setGroupOpen((prev) => ({ ...prev, "in-progress": true }));
      }
    }
  }, [counts]);

  // Auto-expand groups with matches, auto-collapse empty groups during active search
  useEffect(() => {
    if (!counts || !isSearchActive) return;

    const groupKeyToCount: Record<SessionGroup, number> = {
      drafts: counts.drafts,
      "in-progress": counts.inProgress,
      accepted: counts.accepted,
      done: counts.done,
      archived: counts.archived,
    };

    setGroupOpen((prev) => {
      const next = { ...prev };
      for (const groupKey of Object.keys(groupKeyToCount) as SessionGroup[]) {
        const count = groupKeyToCount[groupKey];
        next[groupKey] = count > 0;
      }
      return next;
    });
  }, [counts, isSearchActive]);

  const [editingPlanId, setEditingPlanId] = useState<string | null>(null);
  const [editingTitle, setEditingTitle] = useState("");
  const [openMenuId, setOpenMenuId] = useState<string | null>(null);
  const [isSearchOpen, setIsSearchOpen] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);
  const searchInputRef = useRef<HTMLInputElement>(null);

  // Keep a ref to editingTitle so confirm/keydown handlers don't close over stale state
  const editingTitleRef = useRef(editingTitle);
  useEffect(() => {
    editingTitleRef.current = editingTitle;
  }, [editingTitle]);

  useEffect(() => {
    if (editingPlanId && inputRef.current) {
      inputRef.current.focus();
      inputRef.current.select();
    }
  }, [editingPlanId]);

  // Stable callbacks — defined at component level with planId parameter

  const handleSelect = useCallback((planId: string) => {
    onSelectPlan(planId);
  }, [onSelectPlan]);

  const handleStartRename = useCallback((planId: string, currentTitle: string) => {
    setEditingPlanId(planId);
    setEditingTitle(currentTitle);
  }, []);

  const handleCancelRename = useCallback(() => {
    setEditingPlanId(null);
    setEditingTitle("");
  }, []);

  const handleConfirmRename = useCallback(async (planId: string) => {
    const trimmedTitle = editingTitleRef.current.trim();
    if (trimmedTitle) {
      try {
        await ideationApi.sessions.updateTitle(planId, trimmedTitle);
      } catch (error) {
        console.error("Failed to rename plan:", error);
      }
    }
    setEditingPlanId(null);
    setEditingTitle("");
  }, []); // Uses ref — no editingTitle dep

  const handleKeyDown = useCallback((e: React.KeyboardEvent, planId: string) => {
    if (e.key === "Enter") {
      e.preventDefault();
      void handleConfirmRename(planId);
    } else if (e.key === "Escape") {
      e.preventDefault();
      handleCancelRename();
    }
  }, [handleConfirmRename, handleCancelRename]);

  const handleGroupToggle = useCallback((group: SessionGroup, open: boolean) => {
    setGroupOpen((prev) => ({ ...prev, [group]: open }));
  }, []);

  const handleMenuOpenChange = useCallback((open: boolean, planId: string) => {
    setOpenMenuId(open ? planId : null);
  }, []);

  const handleArchive = useCallback((planId: string) => {
    onArchivePlan?.(planId);
  }, [onArchivePlan]);

  const handleReopen = useCallback((planId: string) => {
    onReopenPlan?.(planId);
  }, [onReopenPlan]);

  const handleResetReaccept = useCallback((planId: string) => {
    onResetReacceptPlan?.(planId);
  }, [onResetReacceptPlan]);

  const renderPlanItem = useCallback((plan: IdeationSessionWithProgress, group: SessionGroup) => (
    <PlanItem
      key={plan.id}
      plan={plan}
      isSelected={plan.id === currentPlanId}
      group={group}
      isEditing={editingPlanId === plan.id}
      editingTitle={plan.id === editingPlanId ? editingTitle : undefined}
      isMenuOpen={openMenuId === plan.id}
      inputRef={inputRef}
      onSelect={handleSelect}
      onStartRename={handleStartRename}
      onConfirmRename={handleConfirmRename}
      onTitleChange={setEditingTitle}
      onKeyDown={handleKeyDown}
      onMenuOpenChange={handleMenuOpenChange}
      onArchive={handleArchive}
      onReopen={handleReopen}
      onResetReaccept={handleResetReaccept}
    />
  ), [
    currentPlanId,
    editingPlanId,
    editingTitle,
    openMenuId,
    handleSelect,
    handleStartRename,
    handleConfirmRename,
    handleKeyDown,
    handleMenuOpenChange,
    handleArchive,
    handleReopen,
    handleResetReaccept,
  ]);

  const hasAnySessions = totalCount > 0;
  const isEmptySearchResult = isSearchActive && totalCount === 0;

  // Accessible result count announcement
  const resultCountText = isSearchActive
    ? totalCount === 0
      ? "No sessions match"
      : `${totalCount} ${totalCount === 1 ? "session" : "sessions"} found`
    : "";

  return (
    <div
      data-testid="plan-browser"
      className="flex flex-col h-full"
      style={{
        width,
        minWidth: width,
        flexShrink: 0,
      }}
    >
      {/* Panel inner container — matches AgentsSidebar chrome via the
         shared --app-sidebar-* tokens so Light / Dark / HC stay in
         lockstep. Longhand border props are required by the WKWebView
         CSS-vars rule. */}
      <div
        className="flex flex-col h-full border-r"
        style={{
          backgroundColor: "var(--app-sidebar-bg)",
          borderRightColor: "var(--app-sidebar-border)",
          borderRightStyle: "solid",
          borderRightWidth: "1px",
          boxShadow: "none",
        }}
      >
        {/* Header — matches AgentsSidebar: small "+ New" + search-toggle + collapse */}
        <div className="flex shrink-0 items-center gap-3 px-3 pb-2 pt-3">
          <button
            type="button"
            className="inline-flex h-7 items-center gap-1.5 rounded-[6px] border px-2 pr-2.5 text-[0.7812rem] font-medium transition-colors duration-[120ms] ease-[cubic-bezier(.2,.8,.2,1)] outline-none focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:2px]"
            onClick={onNewPlan}
            aria-label="New plan"
            data-testid="ideation-new-plan"
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
                        handleSearchClear();
                      }
                      return !open;
                    });
                  }}
                  aria-label={isSearchOpen ? "Close search" : "Search"}
                  data-testid="ideation-search-toggle"
                  style={{ color: "var(--text-muted)", boxShadow: "none" }}
                >
                  {isSearchOpen ? <X className="h-3.5 w-3.5" /> : <Search className="h-3.5 w-3.5" />}
                </button>
              </TooltipTrigger>
              <TooltipContent side="bottom" className="text-xs">
                {isSearchOpen ? "Close search" : "Search"}
              </TooltipContent>
            </Tooltip>
            {onCollapse != null && (
              <Tooltip>
                <TooltipTrigger asChild>
                  <button
                    type="button"
                    className="grid h-7 w-7 place-items-center rounded-[6px] border-0 p-0 transition-colors duration-[120ms] ease-[cubic-bezier(.2,.8,.2,1)] outline-none hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)] focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:2px]"
                    onClick={onCollapse}
                    aria-label="Collapse sidebar"
                    data-testid="sidebar-collapse-button"
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

        {/* Search row — toggleable, matches AgentsSidebar inline search */}
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
                ref={searchInputRef}
                type="text"
                value={searchTerm}
                onChange={(e) => handleSearchChange(e.target.value)}
                placeholder="Search sessions..."
                aria-label="Search sessions"
                className="w-full h-7 pl-8 pr-8 text-[0.75rem] bg-transparent outline-none ring-0 focus:ring-0 focus:outline-none focus-visible:outline-none border-0"
                style={{
                  color: "var(--text-primary)",
                  caretColor: "var(--accent-primary)",
                }}
                autoFocus
              />
              <div className="absolute right-2 flex items-center">
                {isSearchLoading ? (
                  <Loader2
                    className="w-3.5 h-3.5 animate-spin"
                    style={{ color: "var(--text-muted)" }}
                  />
                ) : searchTerm !== "" ? (
                  <button
                    type="button"
                    aria-label="Clear search"
                    onClick={() => {
                      handleSearchClear();
                    searchInputRef.current?.focus();
                  }}
                  className="w-4 h-4 flex items-center justify-center rounded-sm transition-colors duration-100"
                  style={{ color: "var(--text-muted)" }}
                  onMouseEnter={(e) => {
                    e.currentTarget.style.color = "var(--text-primary)";
                  }}
                  onMouseLeave={(e) => {
                    e.currentTarget.style.color = "var(--text-muted)";
                  }}
                >
                    <X className="w-3.5 h-3.5" />
                  </button>
                ) : null}
              </div>
            </div>
          </div>
        )}

        {/* Accessible live region for result count */}
        <div
          aria-live="polite"
          aria-atomic="true"
          className="sr-only"
        >
          {resultCountText}
        </div>

        {/* Plan List */}
        <div className="flex-1 overflow-y-auto py-2">
          {!hasAnySessions && !isSearchActive ? (
            <EmptyState
              variant="neutral"
              icon={<MessageSquare />}
              title="No plans yet"
              description="Start your first brainstorm"
              className="h-full"
            />
          ) : isEmptySearchResult ? (
            <EmptyState
              variant="neutral"
              icon={<Search />}
              title="No sessions match"
              description="Try a different search term"
              className="h-full"
            />
          ) : (
            <>
              {GROUP_CONFIG.map(({ key, label, icon, accentColor }) => {
                const count = counts
                  ? key === "drafts"
                    ? counts.drafts
                    : key === "in-progress"
                      ? counts.inProgress
                      : key === "accepted"
                        ? counts.accepted
                        : key === "done"
                          ? counts.done
                          : counts.archived
                  : 0;

                return (
                  <GroupSection
                    key={key}
                    groupKey={key}
                    projectId={projectId}
                    isOpen={groupOpen[key]}
                    onToggle={(open) => handleGroupToggle(key, open)}
                    icon={icon}
                    label={label}
                    count={count}
                    search={debouncedSearch}
                    activePlanId={currentPlanId}
                    {...(accentColor != null && { accentColor })}
                    renderItem={renderPlanItem}
                  />
                );
              })}
            </>
          )}
        </div>
      </div>
    </div>
  );
}
