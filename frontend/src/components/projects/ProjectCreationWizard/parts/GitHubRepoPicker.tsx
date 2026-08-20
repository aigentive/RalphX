/**
 * GitHubRepoPicker - optional repo browser inside CloneStep. Only renders
 * when useGhAuthStatus reports authenticated; fetches on user expand, not on
 * step mount, per the first-paint rule. Any fetch failure collapses back to
 * plain URL entry silently - no error banner.
 *
 * The fetch-capable UI is split into a separate component so `useQuery` is
 * only ever mounted (and only ever needs a QueryClientProvider ancestor)
 * once the user is actually authenticated - unauthenticated renders never
 * touch React Query at all.
 */

import { useEffect, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { ChevronDown, GitFork, Loader2, Lock } from "lucide-react";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import { cn } from "@/lib/utils";
import { useGhAuthStatus } from "@/hooks/useGithubSettings";
import { projectsApi } from "@/api/projects";

export interface GitHubRepoPickerProps {
  onSelectRepo: (nameWithOwner: string) => void;
  disabled?: boolean | undefined;
}

export function GitHubRepoPicker({ onSelectRepo, disabled = false }: GitHubRepoPickerProps) {
  const authStatus = useGhAuthStatus();
  if (authStatus.data !== true) return null;
  return <GitHubRepoPickerExpanded onSelectRepo={onSelectRepo} disabled={disabled} />;
}

function GitHubRepoPickerExpanded({ onSelectRepo, disabled = false }: GitHubRepoPickerProps) {
  const [expanded, setExpanded] = useState(false);

  const query = useQuery({
    queryKey: ["github-repo-picker"],
    queryFn: () => projectsApi.listGithubRepositories(),
    enabled: expanded,
    staleTime: 60_000,
    retry: false,
  });

  useEffect(() => {
    // Silent fallback: a failed fetch just collapses back to plain URL entry.
    if (query.isError) setExpanded(false);
  }, [query.isError]);

  return (
    <Collapsible open={expanded} onOpenChange={setExpanded}>
      <CollapsibleTrigger
        data-testid="github-repo-picker-trigger"
        disabled={disabled}
        className="flex items-center gap-2 text-xs text-[var(--text-muted)] hover:text-[var(--text-secondary)] transition-colors disabled:opacity-50"
      >
        <GitFork className="h-3 w-3" />
        <span>Browse your GitHub repositories</span>
        <ChevronDown className={cn("h-3 w-3 transition-transform", expanded && "rotate-180")} />
      </CollapsibleTrigger>
      <CollapsibleContent
        data-testid="github-repo-picker"
        className="mt-2 space-y-1 animate-in slide-in-from-top-2 fade-in duration-200"
      >
        {query.isLoading && (
          <div className="flex items-center gap-2 text-xs text-[var(--text-muted)] px-1 py-1">
            <Loader2 className="h-3 w-3 animate-spin" />
            Loading your repositories...
          </div>
        )}
        {query.data?.map((repo) => (
          <button
            key={repo.nameWithOwner}
            type="button"
            data-testid="github-repo-picker-item"
            onClick={() => onSelectRepo(repo.nameWithOwner)}
            disabled={disabled}
            className="w-full flex items-center gap-1.5 px-3 py-2 rounded-lg text-left bg-[var(--bg-base)] hover:bg-[var(--bg-hover)] disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
          >
            <span className="text-sm truncate text-[var(--text-primary)]">{repo.nameWithOwner}</span>
            {repo.isPrivate && <Lock className="h-3 w-3 flex-shrink-0 text-[var(--text-muted)]" />}
          </button>
        ))}
      </CollapsibleContent>
    </Collapsible>
  );
}
