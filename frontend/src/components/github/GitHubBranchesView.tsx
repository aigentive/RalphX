import { useMemo, useState, type CSSProperties, type KeyboardEvent } from "react";
import { useQuery } from "@tanstack/react-query";
import {
  Briefcase,
  ChevronDown,
  ChevronRight,
  CircleDashed,
  CircleX,
  GitBranch,
  GitMerge,
  GitPullRequest,
  RefreshCw,
  Search,
  Ticket,
} from "lucide-react";

import { githubApi, type GitHubBranchOverviewItem } from "@/api/github";
import type { TicketDeepLink } from "@/api/ticketing";
import { GitHubMarkIcon } from "@/components/github/GitHubMarkIcon";
import { PullRequestDetailSheet } from "@/components/pr/PullRequestDetailSheet";
import {
  pullRequestSelectorFromShell,
  type PullRequestShell,
} from "@/components/pr/PullRequestDetailShell";
import { TicketingStatePanel } from "@/components/ticketing/TicketingStatePanel";
import { openExternalTicketUrl } from "@/components/ticketing/ticketing-open-external";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { formatRelativeTime } from "@/lib/formatters";
import { cn } from "@/lib/utils";
import {
  DEFAULT_GITHUB_DASHBOARD_STATE,
  useIntegrationDashboardStore,
  type GitHubBranchAssociationFilter,
  type GitHubBranchPrStatusFilter,
} from "@/stores/integrationDashboardStore";
import type { Project } from "@/types/project";

import { githubBranchOverviewKeys } from "./githubBranchOverviewKeys";

type BranchAssociationFilter = GitHubBranchAssociationFilter;
type BranchPrStatusFilter = GitHubBranchPrStatusFilter;
type BranchStatusGroup = Exclude<BranchPrStatusFilter, "all">;

const EMPTY_BRANCH_OVERVIEW_ITEMS: GitHubBranchOverviewItem[] = [];
const BRANCH_GROUP_ORDER: BranchStatusGroup[] = ["open", "draft", "merged", "closed", "no_pr"];

const BRANCH_ASSOCIATION_FILTERS: Array<{ id: BranchAssociationFilter; label: string }> = [
  { id: "pull_requests", label: "PRs" },
  { id: "all", label: "Branches" },
  { id: "tickets", label: "Tickets" },
  { id: "rx", label: "RX" },
];

const BRANCH_PR_STATUS_FILTERS: Array<{ id: BranchPrStatusFilter; label: string }> = [
  { id: "all", label: "All PRs" },
  { id: "open", label: "Open" },
  { id: "draft", label: "Draft" },
  { id: "merged", label: "Merged" },
  { id: "closed", label: "Closed" },
];

function branchPullRequestShell(
  projectId: string,
  branch: GitHubBranchOverviewItem,
): PullRequestShell {
  return {
    projectId,
    prNumber: branch.prNumber,
    branch: branch.branchName,
    title: branch.prNumber != null ? branch.prTitle ?? `PR #${branch.prNumber}` : branch.branchName,
    url: branch.prUrl,
    status: branch.prStatus,
    rxConversations: branch.rxConversations,
    ticketLinks: branch.ticketLinks,
  };
}

function associatedTicketCount(branch: GitHubBranchOverviewItem): number {
  return Math.max(branch.ticketCount, branch.ticketLinks.length);
}

function associatedRxCount(branch: GitHubBranchOverviewItem): number {
  return Math.max(branch.rxConversationCount, branch.rxConversations.length);
}

function normalizedBranchStatus(branch: GitHubBranchOverviewItem): BranchStatusGroup {
  if (branch.prNumber == null) {
    return "no_pr";
  }
  const rawStatus = (branch.prStatus ?? "").toLowerCase();
  if (branch.prIsDraft || rawStatus === "draft") {
    return "draft";
  }
  if (rawStatus === "merged") {
    return "merged";
  }
  if (rawStatus === "closed") {
    return "closed";
  }
  return "open";
}

function githubPrStatusTone(status: BranchStatusGroup): {
  label: string;
  style: CSSProperties;
} {
  switch (status) {
    case "draft":
      return {
        label: "Draft",
        style: { backgroundColor: "#6e7781", borderColor: "#6e7781", color: "#ffffff" },
      };
    case "merged":
      return {
        label: "Merged",
        style: { backgroundColor: "#8250df", borderColor: "#8250df", color: "#ffffff" },
      };
    case "closed":
      return {
        label: "Closed",
        style: { backgroundColor: "#cf222e", borderColor: "#cf222e", color: "#ffffff" },
      };
    case "no_pr":
      return {
        label: "No PR",
        style: {
          backgroundColor: "transparent",
          borderColor: "var(--border-subtle)",
          color: "var(--text-muted)",
        },
      };
    case "open":
      return {
        label: "Open",
        style: { backgroundColor: "#1a7f37", borderColor: "#1a7f37", color: "#ffffff" },
      };
  }
}

function BranchPrStatusBadge({ branch }: { branch: GitHubBranchOverviewItem }) {
  const tone = githubPrStatusTone(normalizedBranchStatus(branch));
  return (
    <span
      className="inline-flex h-5 shrink-0 items-center rounded-full border px-2 text-[11px] font-medium leading-none"
      style={{
        ...tone.style,
        borderStyle: "solid",
        borderWidth: "1px",
      }}
    >
      {tone.label}
    </span>
  );
}

function BranchStatusIcon({
  status,
  className,
}: {
  status: BranchStatusGroup;
  className?: string | undefined;
}) {
  const common = cn("h-4 w-4", className);
  switch (status) {
    case "draft":
      return <CircleDashed className={common} style={{ color: "#6e7781" }} aria-hidden="true" />;
    case "merged":
      return <GitMerge className={common} style={{ color: "#8250df" }} aria-hidden="true" />;
    case "closed":
      return <CircleX className={common} style={{ color: "#cf222e" }} aria-hidden="true" />;
    case "no_pr":
      return <GitBranch className={common} style={{ color: "var(--text-muted)" }} aria-hidden="true" />;
    case "open":
      return <GitPullRequest className={common} style={{ color: "#1a7f37" }} aria-hidden="true" />;
  }
}

function branchMatchesAssociationFilter(
  branch: GitHubBranchOverviewItem,
  filter: BranchAssociationFilter,
): boolean {
  switch (filter) {
    case "pull_requests":
      return branch.prNumber != null;
    case "tickets":
      return associatedTicketCount(branch) > 0;
    case "rx":
      return associatedRxCount(branch) > 0;
    case "all":
      return true;
  }
}

function branchMatchesSearch(branch: GitHubBranchOverviewItem, query: string): boolean {
  const trimmed = query.trim().toLowerCase();
  if (!trimmed) {
    return true;
  }
  const haystack = [
    branch.branchName,
    branch.prNumber != null ? `#${branch.prNumber}` : "",
    branch.prNumber != null ? String(branch.prNumber) : "",
    branch.prTitle ?? "",
    branch.prAuthorLogin ?? "",
    branch.prBaseRefName ?? "",
    ...branch.ticketLabels,
    ...branch.ticketLinks.flatMap((ticket) => [
      ticket.provider,
      ticket.label,
      ticket.title ?? "",
    ]),
    ...branch.rxConversations.map((conversation) => conversation.title ?? ""),
  ]
    .join(" ")
    .toLowerCase();
  return haystack.includes(trimmed);
}

function branchMatchesSelection(branch: GitHubBranchOverviewItem, selectedValue: string): boolean {
  const normalized = selectedValue.trim().toLowerCase();
  if (!normalized) {
    return false;
  }
  return (
    branch.branchName.toLowerCase() === normalized ||
    (branch.prNumber != null && String(branch.prNumber) === normalized) ||
    (branch.prNumber != null && `#${branch.prNumber}` === normalized)
  );
}

function filterCount(
  branches: GitHubBranchOverviewItem[],
  filter: BranchAssociationFilter,
): number {
  return branches.filter((branch) => branchMatchesAssociationFilter(branch, filter)).length;
}

function statusFilterCount(
  branches: GitHubBranchOverviewItem[],
  filter: BranchPrStatusFilter,
): number {
  if (filter === "all") {
    return branches.filter((branch) => branch.prNumber != null).length;
  }
  return branches.filter((branch) => normalizedBranchStatus(branch) === filter).length;
}

function groupedBranches(branches: GitHubBranchOverviewItem[]): Record<BranchStatusGroup, GitHubBranchOverviewItem[]> {
  return branches.reduce<Record<BranchStatusGroup, GitHubBranchOverviewItem[]>>(
    (groups, branch) => {
      groups[normalizedBranchStatus(branch)].push(branch);
      return groups;
    },
    {
      open: [],
      draft: [],
      merged: [],
      closed: [],
      no_pr: [],
    },
  );
}

function firstTicketUrl(branch: GitHubBranchOverviewItem): string | null {
  return branch.ticketLinks.find((ticket) => ticket.url)?.url ?? null;
}

function firstRxConversation(branch: GitHubBranchOverviewItem) {
  return branch.rxConversations[0] ?? null;
}

function branchUpdatedLabel(branch: GitHubBranchOverviewItem): string {
  return branch.prUpdatedAt ? formatRelativeTime(branch.prUpdatedAt) : "-";
}

function BranchFilterButton({
  active,
  children,
  onClick,
}: {
  active: boolean;
  children: React.ReactNode;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      className="inline-flex h-8 items-center gap-1 rounded-md px-3 text-xs font-medium transition-colors focus-visible:outline-none focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:2px]"
      style={{
        backgroundColor: active ? "var(--bg-elevated)" : "transparent",
        borderColor: active ? "var(--border-default)" : "var(--border-subtle)",
        borderStyle: "solid",
        borderWidth: "1px",
        color: active ? "var(--text-primary)" : "var(--text-muted)",
      }}
      onClick={onClick}
    >
      {children}
    </button>
  );
}

function BranchMetricButton({
  count,
  label,
  icon: Icon,
  disabled,
  onClick,
}: {
  count: number;
  label: string;
  icon: React.ElementType;
  disabled?: boolean | undefined;
  onClick?: (() => void) | undefined;
}) {
  const isDisabled = disabled || count === 0 || !onClick;
  const button = (
    <button
      type="button"
      aria-label={label}
      disabled={isDisabled}
      className="inline-flex h-7 min-w-[44px] items-center justify-center gap-1 rounded-md px-2 text-xs font-medium transition-colors focus-visible:outline-none focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:2px] disabled:cursor-default disabled:opacity-45"
      style={{
        backgroundColor: "var(--bg-surface)",
        borderColor: "var(--border-subtle)",
        borderStyle: "solid",
        borderWidth: "1px",
        color: count > 0 ? "var(--text-primary)" : "var(--text-muted)",
      }}
      onClick={(event) => {
        event.stopPropagation();
        onClick?.();
      }}
    >
      <Icon className="h-3.5 w-3.5" aria-hidden="true" />
      {count}
    </button>
  );

  if (isDisabled) {
    return button;
  }

  return (
    <Tooltip>
      <TooltipTrigger asChild>{button}</TooltipTrigger>
      <TooltipContent side="top" className="text-xs">
        {label}
      </TooltipContent>
    </Tooltip>
  );
}

function BranchRow({
  branch,
  primaryMode,
  projectId,
  onOpenDetails,
  onNavigateToAssociation,
}: {
  branch: GitHubBranchOverviewItem;
  primaryMode: "pull_request" | "branch";
  projectId: string;
  onOpenDetails: (branch: GitHubBranchOverviewItem) => void;
  onNavigateToAssociation?: ((deepLink: TicketDeepLink) => void) | undefined;
}) {
  const status = normalizedBranchStatus(branch);
  const ticketCount = associatedTicketCount(branch);
  const rxCount = associatedRxCount(branch);
  const ticketUrl = firstTicketUrl(branch);
  const rxConversation = firstRxConversation(branch);
  const ticketLabel =
    ticketCount === 1 ? "1 attached ticket" : `${ticketCount} attached tickets`;
  const rxLabel =
    rxCount === 1 ? "1 RalphX conversation" : `${rxCount} RalphX conversations`;
  const primaryTitle =
    primaryMode === "pull_request" && branch.prNumber != null
      ? branch.prTitle ?? `PR #${branch.prNumber}`
      : branch.branchName;
  const secondaryTitle =
    primaryMode === "pull_request" && branch.prNumber != null
      ? branch.branchName
      : branch.prTitle ?? branch.ticketLabels[0] ?? "Branch without a linked PR";

  function openDetails() {
    onOpenDetails(branch);
  }

  function handleKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    if (event.target !== event.currentTarget) {
      return;
    }
    if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      openDetails();
    }
  }

  return (
    <div
      role="button"
      tabIndex={0}
      data-testid={`github-branch-row-${branch.branchName}`}
      aria-label={`Open pull request details for ${primaryTitle}`}
      className="grid min-h-[56px] grid-cols-[28px_minmax(220px,1fr)_128px_92px_56px_56px] items-center gap-3 border-b px-4 py-2 text-left transition-colors hover:bg-[var(--bg-sunken)] focus-visible:outline-none focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:-2px]"
      style={{
        backgroundColor: "var(--bg-surface)",
        borderBottomColor: "var(--border-subtle)",
        borderBottomStyle: "solid",
        borderBottomWidth: "1px",
        color: "var(--text-primary)",
      }}
      onClick={openDetails}
      onKeyDown={handleKeyDown}
    >
      <BranchStatusIcon status={status} />
      <div className="min-w-0">
        <div className="flex min-w-0 items-center gap-2">
          <span className="truncate text-sm font-medium">{primaryTitle}</span>
          {branch.isCurrent ? (
            <Badge
              variant="outline"
              className="h-5 shrink-0 rounded-full px-2 text-[11px] font-medium"
              style={{
                backgroundColor: "var(--accent-muted)",
                borderColor: "var(--accent-border)",
                borderStyle: "solid",
                borderWidth: "1px",
                color: "var(--accent-primary)",
              }}
            >
              Current
            </Badge>
          ) : null}
        </div>
        <p className="mt-0.5 truncate text-xs text-[var(--text-muted)]">
          {secondaryTitle}
        </p>
      </div>
      <div className="min-w-0 text-xs text-[var(--text-muted)]">
        <div className="flex min-w-0 items-center gap-1">
          {branch.prNumber != null ? (
            <span className="font-medium text-[var(--text-primary)]">#{branch.prNumber}</span>
          ) : null}
          <BranchPrStatusBadge branch={branch} />
        </div>
        {branch.prBaseRefName ? (
          <span className="block truncate">into {branch.prBaseRefName}</span>
        ) : null}
      </div>
      <div className="min-w-0 text-xs text-[var(--text-muted)]">
        <span className="truncate">{branch.prAuthorLogin ?? "-"}</span>
        <span className="block truncate">{branchUpdatedLabel(branch)}</span>
      </div>
      <BranchMetricButton
        count={ticketCount}
        label={ticketLabel}
        icon={Ticket}
        onClick={() => {
          if (ticketUrl) {
            void openExternalTicketUrl(ticketUrl);
            return;
          }
          openDetails();
        }}
      />
      <BranchMetricButton
        count={rxCount}
        label={rxLabel}
        icon={Briefcase}
        onClick={() => {
          if (rxConversation && onNavigateToAssociation) {
            onNavigateToAssociation({
              view: "agents",
              id: rxConversation.conversationId,
              projectId,
            });
            return;
          }
          openDetails();
        }}
      />
    </div>
  );
}

function GroupHeader({
  status,
  count,
  collapsed,
  onToggle,
}: {
  status: BranchStatusGroup;
  count: number;
  collapsed: boolean;
  onToggle: () => void;
}) {
  const tone = githubPrStatusTone(status);
  const Chevron = collapsed ? ChevronRight : ChevronDown;
  return (
    <button
      type="button"
      className="sticky top-0 z-10 grid h-9 w-full grid-cols-[18px_18px_minmax(0,1fr)_auto] items-center gap-2 border-b px-4 text-left text-xs font-semibold uppercase tracking-[0.08em] focus-visible:outline-none focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:-2px]"
      style={{
        backgroundColor: "var(--bg-elevated)",
        borderBottomColor: "var(--border-subtle)",
        borderBottomStyle: "solid",
        borderBottomWidth: "1px",
        color: "var(--text-muted)",
      }}
      onClick={onToggle}
    >
      <Chevron className="h-4 w-4" aria-hidden="true" />
      <BranchStatusIcon status={status} />
      <span>{tone.label}</span>
      <span>{count}</span>
    </button>
  );
}

function EmptyFilteredState({ onReset }: { onReset: () => void }) {
  return (
    <div className="grid h-full min-h-[320px] place-items-center px-6 text-center">
      <div>
        <p className="text-sm font-semibold text-[var(--text-primary)]">
          No branches match these filters.
        </p>
        <p className="mt-1 text-sm text-[var(--text-muted)]">
          Clear the filters to return to the default pull request list.
        </p>
        <Button type="button" variant="outline" size="sm" className="mt-4" onClick={onReset}>
          Reset filters
        </Button>
      </div>
    </div>
  );
}

export function GitHubBranchesView({
  projectId,
  project,
  onNavigateToAssociation,
}: {
  projectId: string;
  project: Project | null;
  onNavigateToAssociation?: ((deepLink: TicketDeepLink) => void) | undefined;
}) {
  const persistedState = useIntegrationDashboardStore((state) => state.githubByProject[projectId]);
  const setGitHubState = useIntegrationDashboardStore((state) => state.setGitHubState);
  const resetGitHubFilters = useIntegrationDashboardStore((state) => state.resetGitHubFilters);
  const associationFilter =
    persistedState?.associationFilter ?? DEFAULT_GITHUB_DASHBOARD_STATE.associationFilter;
  const statusFilter = persistedState?.statusFilter ?? DEFAULT_GITHUB_DASHBOARD_STATE.statusFilter;
  const searchQuery = persistedState?.searchQuery ?? DEFAULT_GITHUB_DASHBOARD_STATE.searchQuery;
  const selectedBranchName = persistedState?.selectedBranchName ?? null;
  const [collapsedGroups, setCollapsedGroups] = useState<Record<string, boolean>>({});

  const overviewQuery = useQuery({
    queryKey: githubBranchOverviewKeys.project(projectId),
    queryFn: () => githubApi.getBranchOverview({ projectId }),
    enabled: Boolean(projectId && project),
    staleTime: 15_000,
  });

  const branches = overviewQuery.data?.branches ?? EMPTY_BRANCH_OVERVIEW_ITEMS;
  const filteredBranches = useMemo(() => {
    return branches.filter((branch) => {
      if (!branchMatchesSearch(branch, searchQuery)) {
        return false;
      }
      if (!branchMatchesAssociationFilter(branch, associationFilter)) {
        return false;
      }
      if (associationFilter === "pull_requests" && statusFilter !== "all") {
        return normalizedBranchStatus(branch) === statusFilter;
      }
      return true;
    });
  }, [associationFilter, branches, searchQuery, statusFilter]);

  const groups = useMemo(() => groupedBranches(filteredBranches), [filteredBranches]);
  const pullRequestBranches = useMemo(
    () => branches.filter((branch) => branch.prNumber != null),
    [branches],
  );
  const ticketBranchCount = filterCount(branches, "tickets");
  const rxBranchCount = filterCount(branches, "rx");
  const currentBranch = overviewQuery.data?.currentBranch ?? null;
  const githubUnavailable = overviewQuery.data?.sourcesUnavailable.includes("githubPullRequests");
  const selectedBranch = selectedBranchName
    ? branches.find((branch) => branchMatchesSelection(branch, selectedBranchName)) ?? null
    : null;
  const selectedPullRequestShell = selectedBranch
    ? branchPullRequestShell(projectId, selectedBranch)
    : null;
  const selectedSelector = pullRequestSelectorFromShell(selectedPullRequestShell);

  function resetFilters() {
    resetGitHubFilters(projectId);
  }

  function openBranchDetails(branch: GitHubBranchOverviewItem) {
    setGitHubState(projectId, { selectedBranchName: branch.branchName });
  }

  if (!project) {
    return (
      <TicketingStatePanel
        state="empty"
        title="Project unavailable"
        description="Select a project to inspect its branches."
      />
    );
  }

  if (overviewQuery.isLoading) {
    return (
      <TicketingStatePanel
        state="loading"
        title="Loading branches"
        description="Reading repository branches and GitHub pull requests."
      />
    );
  }

  if (overviewQuery.isError) {
    return (
      <TicketingStatePanel
        state="error"
        title="Branches failed to load"
        description={
          overviewQuery.error instanceof Error
            ? overviewQuery.error.message
            : "RalphX could not read repository branches."
        }
      />
    );
  }

  if (branches.length === 0) {
    return (
      <TicketingStatePanel
        state="empty"
        title="No branches found"
        description="The active project repository did not report any branches."
      />
    );
  }

  return (
    <div
      data-testid="github-branches-view"
      className="flex h-full min-h-0 flex-col"
      style={{ backgroundColor: "var(--app-content-bg)", color: "var(--text-primary)" }}
    >
      <header
        className="shrink-0 border-b px-5 py-4"
        style={{
          backgroundColor: "var(--bg-surface)",
          borderBottomColor: "var(--border-subtle)",
          borderBottomStyle: "solid",
          borderBottomWidth: "1px",
        }}
      >
        <div className="flex flex-wrap items-start justify-between gap-4">
          <div className="min-w-0">
            <div className="flex min-w-0 items-center gap-2">
              <GitHubMarkIcon className="h-5 w-5 shrink-0 text-[var(--text-muted)]" />
              <h1 className="truncate text-lg font-semibold">Branches and pull requests</h1>
            </div>
            <p className="mt-1 truncate text-sm text-[var(--text-muted)]">
              {project.name} · current branch {currentBranch ?? "unknown"}
            </p>
          </div>
          <div className="flex flex-wrap items-center gap-2">
            <Badge
              variant="outline"
              className="h-6 rounded-full px-2 text-xs font-medium"
              style={{
                backgroundColor: "var(--bg-elevated)",
                borderColor: "var(--border-subtle)",
                borderStyle: "solid",
                borderWidth: "1px",
                color: "var(--text-muted)",
              }}
            >
              {branches.length} branches
            </Badge>
            <Badge
              variant="outline"
              className="h-6 rounded-full px-2 text-xs font-medium"
              style={{
                backgroundColor: "var(--bg-elevated)",
                borderColor: "var(--border-subtle)",
                borderStyle: "solid",
                borderWidth: "1px",
                color: "var(--text-muted)",
              }}
            >
              {pullRequestBranches.length} PRs
            </Badge>
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={() => void overviewQuery.refetch()}
            >
              <RefreshCw className="h-4 w-4" aria-hidden="true" />
              Refresh
            </Button>
          </div>
        </div>

        <div className="mt-4 flex flex-wrap items-center gap-3">
          <div className="relative min-w-[240px] flex-1">
            <Search
              className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-[var(--text-muted)]"
              aria-hidden="true"
            />
            <Input
              value={searchQuery}
              onChange={(event) => setGitHubState(projectId, { searchQuery: event.target.value })}
              placeholder="Search branches, PRs, tickets, or authors"
              className="h-8 pl-9 text-sm"
              style={{
                backgroundColor: "var(--bg-elevated)",
                borderColor: "var(--border-subtle)",
                color: "var(--text-primary)",
              }}
            />
          </div>
          <div className="flex flex-wrap items-center gap-1">
            {BRANCH_ASSOCIATION_FILTERS.map((filter) => (
              <BranchFilterButton
                key={filter.id}
                active={associationFilter === filter.id}
                onClick={() => {
                  setGitHubState(projectId, {
                    associationFilter: filter.id,
                    ...(filter.id !== "pull_requests" ? { statusFilter: "all" } : {}),
                  });
                }}
              >
                {filter.label} {filterCount(branches, filter.id)}
              </BranchFilterButton>
            ))}
          </div>
        </div>

        {associationFilter === "pull_requests" ? (
          <div className="mt-3 flex flex-wrap items-center gap-1">
            {BRANCH_PR_STATUS_FILTERS.map((filter) => (
              <BranchFilterButton
                key={filter.id}
                active={statusFilter === filter.id}
                onClick={() => setGitHubState(projectId, { statusFilter: filter.id })}
              >
                {filter.label} {statusFilterCount(branches, filter.id)}
              </BranchFilterButton>
            ))}
          </div>
        ) : null}

        {githubUnavailable ? (
          <div
            className="mt-3 flex items-center gap-2 rounded-md px-3 py-2 text-xs"
            style={{
              backgroundColor: "var(--status-warning-bg)",
              borderColor: "var(--status-warning-border)",
              borderStyle: "solid",
              borderWidth: "1px",
              color: "var(--status-warning)",
            }}
          >
            <CircleDashed className="h-4 w-4" aria-hidden="true" />
            GitHub PR search is unavailable; local branch and RalphX indicators may still be shown.
          </div>
        ) : null}
      </header>

      <div className="grid min-h-0 flex-1 grid-rows-[auto_1fr] overflow-hidden">
        <div
          className="grid h-9 grid-cols-[28px_minmax(220px,1fr)_128px_92px_56px_56px] items-center gap-3 border-b px-4 text-xs font-semibold uppercase tracking-[0.08em] text-[var(--text-muted)]"
          style={{
            backgroundColor: "var(--bg-surface)",
            borderBottomColor: "var(--border-subtle)",
            borderBottomStyle: "solid",
            borderBottomWidth: "1px",
          }}
        >
          <span />
          <span>{associationFilter === "pull_requests" ? "Pull request" : "Branch"}</span>
          <span>PR</span>
          <span>Owner</span>
          <span>Ticket</span>
          <span>RX</span>
        </div>
        <div className="min-h-0 overflow-y-auto" aria-label="Repository branches">
          {filteredBranches.length === 0 ? (
            <EmptyFilteredState onReset={resetFilters} />
          ) : (
            BRANCH_GROUP_ORDER.map((status) => {
              const group = groups[status];
              if (group.length === 0) {
                return null;
              }
              const collapsed = Boolean(collapsedGroups[status]);
              return (
                <section key={status} aria-label={`${githubPrStatusTone(status).label} branches`}>
                  <GroupHeader
                    status={status}
                    count={group.length}
                    collapsed={collapsed}
                    onToggle={() => {
                      setCollapsedGroups((current) => ({
                        ...current,
                        [status]: !current[status],
                      }));
                    }}
                  />
                  {!collapsed
                    ? group.map((branch) => (
                        <BranchRow
                          key={branch.branchName}
                          branch={branch}
                          primaryMode={
                            associationFilter === "pull_requests" ? "pull_request" : "branch"
                          }
                          projectId={projectId}
                          onOpenDetails={openBranchDetails}
                          onNavigateToAssociation={onNavigateToAssociation}
                        />
                      ))
                    : null}
                </section>
              );
            })
          )}
        </div>
      </div>

      <PullRequestDetailSheet
        open={selectedPullRequestShell !== null}
        selector={selectedSelector}
        shell={selectedPullRequestShell}
        onNavigateToAssociation={onNavigateToAssociation}
        onClose={() => setGitHubState(projectId, { selectedBranchName: null })}
      />

      <span className="sr-only" data-testid="github-branch-association-counts">
        {ticketBranchCount} branches with tickets, {rxBranchCount} branches with RalphX conversations.
      </span>
    </div>
  );
}
