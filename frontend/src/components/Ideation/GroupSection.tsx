/**
 * GroupSection - collapsible session group in PlanBrowser sidebar
 *
 * Extracted from PlanBrowser.tsx to keep that file under 500 LOC.
 * Uses infinite scroll with IntersectionObserver for lazy pagination.
 */

import { useRef, useEffect } from "react";
import type { IdeationSessionWithProgress } from "@/types/ideation";
import { SessionGroupHeader } from "./SessionGroupHeader";
import { SessionGroupSkeleton } from "./SessionGroupSkeleton";
import { GROUP_KEY_TO_API, type SessionGroup } from "./planBrowserUtils";
import { useInfiniteSessionsQuery, flattenSessionPages } from "@/hooks/useInfiniteSessionsQuery";
import type { Pencil } from "lucide-react";

// ============================================================================
// Types
// ============================================================================

export interface GroupSectionProps {
  groupKey: SessionGroup;
  projectId: string;
  isOpen: boolean;
  onToggle: (open: boolean) => void;
  icon: typeof Pencil;
  label: string;
  accentColor?: string;
  count: number;
  search: string;
  activePlanId?: string | null;
  renderItem: (plan: IdeationSessionWithProgress, group: SessionGroup) => React.ReactNode;
}

// ============================================================================
// Component
// ============================================================================

export function GroupSection({
  groupKey,
  projectId,
  isOpen,
  onToggle,
  icon,
  label,
  accentColor,
  count,
  search,
  activePlanId,
  renderItem,
}: GroupSectionProps) {
  const apiKey = GROUP_KEY_TO_API[groupKey];
  const {
    data,
    fetchNextPage,
    hasNextPage,
    isFetchingNextPage,
    isLoading,
  } = useInfiniteSessionsQuery(projectId, apiKey, { enabled: isOpen, search });

  const sessions = flattenSessionPages(data);

  // Intersection observer for infinite scroll
  const sentinelRef = useRef<HTMLDivElement | null>(null);
  const fetchNextPageRef = useRef(fetchNextPage);
  const hasNextPageRef = useRef(hasNextPage);
  useEffect(() => {
    fetchNextPageRef.current = fetchNextPage;
    hasNextPageRef.current = hasNextPage;
  }, [fetchNextPage, hasNextPage]);

  useEffect(() => {
    if (!sentinelRef.current || !isOpen) return;

    const observer = new IntersectionObserver(
      (entries) => {
        const first = entries[0];
        // Guard: skip if sidebar container is hidden (display: none) — avoids burst
        // fetchNextPage() calls when display toggles from none to visible
        if (!first?.isIntersecting || !hasNextPageRef.current || isFetchingNextPage) return;
        const parentContainer = sentinelRef.current?.closest("[data-testid='plan-browser']");
        if (parentContainer) {
          const style = window.getComputedStyle(parentContainer);
          if (style.display === "none") return;
        }
        fetchNextPageRef.current();
      },
      { threshold: 0.1 }
    );

    observer.observe(sentinelRef.current);
    return () => observer.disconnect();
  }, [isOpen, isFetchingNextPage]);

  if (count === 0) return null;

  // Drafts group renders flat (no collapsible header)
  if (groupKey === "drafts") {
    return (
      <div className="space-y-1">
        {isLoading ? (
          <SessionGroupSkeleton count={Math.min(count, 3)} />
        ) : (
          <>
            {sessions.map((plan) => renderItem(plan, groupKey))}
            {hasNextPage && (
              <div ref={sentinelRef} className="h-2" />
            )}
            {isFetchingNextPage && (
              <SessionGroupSkeleton count={1} />
            )}
          </>
        )}
      </div>
    );
  }

  const containsActive =
    activePlanId != null && sessions.some((s) => s.id === activePlanId);

  return (
    <SessionGroupHeader
      icon={icon}
      label={label}
      count={count}
      isOpen={isOpen}
      onToggle={onToggle}
      isActive={containsActive}
      {...(accentColor != null && { accentColor })}
    >
      {isOpen && (
        <>
          {isLoading ? (
            <SessionGroupSkeleton count={Math.min(count, 3)} />
          ) : (
            <>
              {sessions.map((plan) => renderItem(plan, groupKey))}
              {hasNextPage && (
                <div ref={sentinelRef} className="h-2" />
              )}
              {isFetchingNextPage && (
                <SessionGroupSkeleton count={1} />
              )}
            </>
          )}
        </>
      )}
    </SessionGroupHeader>
  );
}
