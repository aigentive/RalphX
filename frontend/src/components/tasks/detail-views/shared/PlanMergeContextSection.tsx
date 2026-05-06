import { useMemo } from "react";
import { useQuery } from "@tanstack/react-query";
import { FileText, GitPullRequest, Target } from "lucide-react";
import { usePlanBranchForTask } from "@/hooks/usePlanBranchForTask";
import { api } from "@/lib/tauri";
import type { ArtifactResponse } from "@/api/artifacts";
import { BranchBadge } from "@/components/shared/BranchBadge";
import { DetailCard } from "./DetailCard";
import { SectionTitle } from "./SectionTitle";

interface PlanMergeContextCardProps {
  taskId: string;
  compact?: boolean;
}

function cleanMarkdownLine(line: string): string {
  return line
    .replace(/^#{1,6}\s+/, "")
    .replace(/^[-*]\s+/, "")
    .replace(/\*\*/g, "")
    .replace(/\[(.*?)\]\([^)]+\)/g, "$1")
    .trim();
}

function getInlinePlanText(artifact: ArtifactResponse | null | undefined): string | null {
  if (!artifact || artifact.content_type !== "inline") return null;
  return artifact.content;
}

function getPlanTitle(artifact: ArtifactResponse | null | undefined): string {
  const text = getInlinePlanText(artifact);
  const heading = text
    ?.split(/\r?\n/)
    .map((line) => line.trim())
    .find((line) => /^#{1,3}\s+\S/.test(line));

  return cleanMarkdownLine(heading ?? artifact?.name ?? "Plan");
}

function getPlanExcerpt(artifact: ArtifactResponse | null | undefined): string | null {
  const text = getInlinePlanText(artifact);
  if (!text) return null;

  const lines = text
    .split(/\r?\n/)
    .map((line) => cleanMarkdownLine(line))
    .filter((line) => line.length > 0 && !line.startsWith("|") && !/^[-=]{3,}$/.test(line));
  const title = getPlanTitle(artifact);
  const excerpt = lines.find((line) => line !== title);

  if (!excerpt) return null;
  return excerpt.length > 180 ? `${excerpt.slice(0, 177)}...` : excerpt;
}

export function PlanMergeContextCard({
  taskId,
  compact = false,
}: PlanMergeContextCardProps) {
  const { data: planBranch, isLoading: isLoadingPlanBranch } = usePlanBranchForTask(taskId);
  const { data: planArtifact, isLoading: isLoadingPlanArtifact } = useQuery({
    queryKey: ["plan-artifact", planBranch?.planArtifactId] as const,
    queryFn: () => api.artifacts.getArtifact(planBranch!.planArtifactId),
    enabled: Boolean(planBranch?.planArtifactId),
    staleTime: 30_000,
  });

  const { planTitle, planExcerpt } = useMemo(
    () => ({
      planTitle: getPlanTitle(planArtifact),
      planExcerpt: getPlanExcerpt(planArtifact),
    }),
    [planArtifact]
  );

  if (isLoadingPlanBranch || isLoadingPlanArtifact) {
    return (
      <DetailCard>
        <div className="space-y-2">
          <div className="h-4 w-2/3 rounded animate-pulse bg-[var(--overlay-moderate)]" />
          <div className="h-3 w-full rounded animate-pulse bg-[var(--overlay-moderate)]" />
        </div>
      </DetailCard>
    );
  }

  if (!planBranch) return null;

  const targetBranch =
    planBranch.baseBranchOverride?.trim() || planBranch.sourceBranch.trim() || null;

  return (
    <DetailCard>
      <div className="space-y-3" data-testid="plan-merge-context-card">
        <div className="flex items-start gap-3">
          <div
            className="flex items-center justify-center rounded-lg shrink-0"
            style={{
              width: compact ? "28px" : "32px",
              height: compact ? "28px" : "32px",
              backgroundColor: "var(--overlay-weak)",
            }}
          >
            <FileText className="w-4 h-4 text-text-primary/45" />
          </div>
          <div className="min-w-0 flex-1">
            <span className="text-[0.6875rem] uppercase tracking-wider text-text-primary/40 block">
              Plan
            </span>
            <span className="text-[0.8125rem] text-text-primary/80 font-medium line-clamp-2">
              {planTitle}
            </span>
            {planExcerpt && !compact && (
              <p className="mt-1 text-[0.75rem] text-text-primary/50 line-clamp-2">
                {planExcerpt}
              </p>
            )}
          </div>
        </div>

        <div className="flex flex-wrap items-center gap-2 text-[0.6875rem] text-text-primary/45">
          {planArtifact && (
            <span className="px-2 py-1 rounded bg-[var(--overlay-faint)]">
              v{planArtifact.version}
            </span>
          )}
          <BranchBadge branch={planBranch.branchName} variant="muted" />
          {targetBranch && (
            <>
              <Target className="w-3.5 h-3.5 text-text-primary/35" />
              <BranchBadge branch={targetBranch} variant="muted" />
            </>
          )}
          {planBranch.prNumber != null && (
            <span className="inline-flex items-center gap-1 px-2 py-1 rounded bg-[var(--overlay-faint)] text-text-primary/55">
              <GitPullRequest className="w-3.5 h-3.5" />
              PR #{planBranch.prNumber}
            </span>
          )}
        </div>
      </div>
    </DetailCard>
  );
}

export function PlanMergeContextSection({ taskId }: { taskId: string }) {
  return (
    <section data-testid="plan-merge-context-section">
      <SectionTitle>Plan</SectionTitle>
      <PlanMergeContextCard taskId={taskId} />
    </section>
  );
}
