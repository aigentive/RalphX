/**
 * AcceptedSessionBanner - Shows acceptance status, live task counts, and "View Work" CTA
 *
 * Rendered at the top of PlanningView when session.status === "accepted".
 * Task counts are live/reactive via the existing useTasks query.
 */

import { useMemo } from "react";
import { useTasks } from "@/hooks/useTasks";
import type { TaskProposal } from "@/types/ideation";
import { getStatusCounts } from "@/types/status";
import { AcceptedPlanProgressBanner } from "./AcceptedPlanProgressBanner";

interface AcceptedSessionBannerProps {
  projectId: string;
  proposals: TaskProposal[];
  convertedAt: string | null;
  onViewWork: () => void;
  onRestartImplementation?: () => void;
  isRestartingImplementation?: boolean;
}

export function AcceptedSessionBanner({
  projectId,
  proposals,
  convertedAt,
  onViewWork,
  onRestartImplementation,
  isRestartingImplementation,
}: AcceptedSessionBannerProps) {
  const { data: allTasks } = useTasks(projectId);

  const createdTaskIds = useMemo(
    () => new Set(proposals.filter((p) => p.createdTaskId != null).map((p) => p.createdTaskId!)),
    [proposals]
  );

  const sessionTasks = useMemo(
    () => (allTasks ?? []).filter((t) => createdTaskIds.has(t.id)),
    [allTasks, createdTaskIds]
  );

  const counts = useMemo(() => getStatusCounts(sessionTasks), [sessionTasks]);

  if (createdTaskIds.size === 0) return null;

  return (
    <AcceptedPlanProgressBanner
      counts={counts}
      convertedAt={convertedAt}
      onViewWork={onViewWork}
      {...(onRestartImplementation && { onRestartImplementation })}
      {...(isRestartingImplementation !== undefined && { isRestartingImplementation })}
    />
  );
}
