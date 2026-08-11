import { type ReactNode } from "react";
import {
  Clock,
  GitBranch,
  GitCommit,
} from "lucide-react";
import { BranchBadge } from "@/components/shared/BranchBadge";
import type { InternalStatus } from "@/types/task";
import { DescriptionBlock } from "./DescriptionBlock";
import { DetailCard } from "./DetailCard";
import { SectionTitle } from "./SectionTitle";
import type {
  TaskDetailContextModel,
  TaskDetailViewMode,
} from "./TaskDetailContext";

const STATUS_LABELS: Partial<Record<InternalStatus, string>> = {
  pending_review: "Pending Review",
  qa_refining: "QA Refining",
  qa_testing: "QA Testing",
  qa_passed: "QA Passed",
  qa_failed: "QA Failed",
  review_passed: "AI Review Passed",
  waiting_on_pr: "Waiting on PR",
  merge_incomplete: "Merge Incomplete",
  merge_conflict: "Merge Conflict",
};

function titleCaseStatus(status: InternalStatus): string {
  const knownLabel = STATUS_LABELS[status];
  if (knownLabel) return knownLabel;

  return status
    .split("_")
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join(" ");
}

function formatTimestamp(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return new Intl.DateTimeFormat(undefined, {
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
  }).format(date);
}

function shortSha(value: string): string {
  return value.slice(0, 7);
}

function RailSection({
  title,
  children,
}: {
  title: ReactNode;
  children: ReactNode;
}) {
  return (
    <section className="space-y-2">
      <SectionTitle muted>{title}</SectionTitle>
      {children}
    </section>
  );
}

function HistoricalLensCard({ viewMode }: { viewMode: TaskDetailViewMode }) {
  if (viewMode.kind !== "historical") return null;

  return (
    <RailSection title="View">
      <DetailCard variant="info">
        <div className="space-y-2.5">
          <div className="flex items-center gap-2">
            <Clock className="w-4 h-4 text-text-primary/45 shrink-0" />
            <div className="min-w-0">
              <div className="text-[0.8125rem] font-medium text-text-primary/80">
                Historical State
              </div>
              <div className="text-[0.75rem] text-text-primary/45">
                {titleCaseStatus(viewMode.status)}
              </div>
              <div className="text-[0.75rem] text-text-primary/35">
                {formatTimestamp(viewMode.timestamp)}
              </div>
            </div>
          </div>
          <p className="text-[0.75rem] leading-relaxed text-text-primary/45">
            Branch and merge values show the latest task context.
          </p>
        </div>
      </DetailCard>
    </RailSection>
  );
}

function MergeCard({
  merge,
}: {
  merge: NonNullable<TaskDetailContextModel["merge"]>;
}) {
  return (
    <RailSection title="Merge">
      <DetailCard variant="success">
        <div className="space-y-2.5">
          {merge.target && (
            <div className="flex items-center gap-2">
              <GitBranch className="w-4 h-4 text-text-primary/40 shrink-0" />
              <div className="min-w-0">
                <div className="text-[0.6875rem] uppercase tracking-wider text-text-primary/35">
                  Target
                </div>
                <BranchBadge branch={merge.target} variant="target" size="sm" />
              </div>
            </div>
          )}
          {merge.commitSha && (
            <div className="flex items-center gap-2">
              <GitCommit className="w-4 h-4 text-text-primary/40 shrink-0" />
              <div>
                <div className="text-[0.6875rem] uppercase tracking-wider text-text-primary/35">
                  Commit
                </div>
                <span className="text-[0.75rem] font-mono text-text-primary/70">
                  {shortSha(merge.commitSha)}
                </span>
              </div>
            </div>
          )}
          {merge.mergedAt && (
            <div className="text-[0.75rem] text-text-primary/40">
              Merged {formatTimestamp(merge.mergedAt)}
            </div>
          )}
        </div>
      </DetailCard>
    </RailSection>
  );
}

export function TaskContextRail({
  model,
  fallbackDescription,
  showTask = true,
  showHistorical = true,
  showMerge = true,
}: {
  model: TaskDetailContextModel;
  fallbackDescription?: string | null | undefined;
  showTask?: boolean;
  showHistorical?: boolean;
  showMerge?: boolean;
}) {
  const description = model.task.description ?? fallbackDescription;

  if (!showTask && !showHistorical && !showMerge) return null;

  return (
    <div className="space-y-5">
      {showTask && (
        <RailSection title="Task">
          <DescriptionBlock description={description} />
        </RailSection>
      )}

      {showHistorical && <HistoricalLensCard viewMode={model.viewMode} />}
      {showMerge && model.merge && <MergeCard merge={model.merge} />}
    </div>
  );
}
