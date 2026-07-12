import { useState } from "react";
import { Bot, User, Eye, Loader2 } from "lucide-react";
import { Card } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { cn } from "@/lib/utils";
import type { Task } from "@/types/task";
import type { InternalStatus } from "@/types/status";

// ============================================================================
// TaskReviewCard - Card component for displaying tasks in review panel
// ============================================================================

/** Map task status to display label */
function getStatusLabel(status: InternalStatus): string {
  const labels: Partial<Record<InternalStatus, string>> = {
    pending_review: "Pending Review",
    reviewing: "Reviewing",
    review_passed: "Review Passed",
    escalated: "Escalated",
  };
  return labels[status] ?? status.replace(/_/g, " ");
}

/** Map task status to badge variant styling */
function getStatusBadgeClass(status: InternalStatus): string {
  switch (status) {
    case "pending_review":
      return "bg-[var(--status-warning)]/20 text-[var(--status-warning)] border-[var(--status-warning)]/30";
    case "reviewing":
      return "bg-status-info/20 text-status-info border-status-info/30";
    case "review_passed":
      return "bg-[var(--status-success)]/20 text-[var(--status-success)] border-[var(--status-success)]/30";
    case "escalated":
      return "bg-[var(--status-error)]/20 text-[var(--status-error)] border-[var(--status-error)]/30";
    default:
      return "bg-[var(--bg-hover)] text-[var(--text-secondary)] border-[var(--border-subtle)]";
  }
}

/** Check if task is in AI review phase */
function isAiReviewPhase(status: InternalStatus): boolean {
  return status === "pending_review" || status === "reviewing";
}

interface TaskReviewCardProps {
  task: Task;
  onReview?: (taskId: string) => void;
  isLoading?: boolean;
  presentation?: "default" | "panel";
}

export function TaskReviewCard({
  task,
  onReview,
  isLoading = false,
  presentation = "default",
}: TaskReviewCardProps) {
  const [isHovered, setIsHovered] = useState(false);
  const isAiPhase = isAiReviewPhase(task.internalStatus);
  const panelPresentation = presentation === "panel";

  return (
    <Card
      data-testid={`task-review-card-${task.id}`}
      data-status={task.internalStatus}
      data-presentation={presentation}
      onMouseEnter={() => setIsHovered(true)}
      onMouseLeave={() => setIsHovered(false)}
      className={cn(
        "border transition-all duration-150 ease-out",
        "bg-[var(--bg-elevated)] border-[var(--border-subtle)]",
        "rounded-[var(--radius-md)]",
        panelPresentation
          ? "w-full max-w-full min-w-0 overflow-hidden p-3 shadow-none"
          : "p-5",
        !panelPresentation && isHovered && "shadow-[var(--shadow-xs)]",
        !panelPresentation && isHovered && "-translate-y-[1px]",
        isHovered && "border-[var(--border-default)]"
      )}
    >
      {/* Task Title */}
      <div
        data-testid="task-review-title"
        className={cn(
          "font-semibold text-sm text-[var(--text-primary)] leading-tight",
          panelPresentation ? "min-w-0 break-words [overflow-wrap:anywhere] line-clamp-2" : "truncate"
        )}
      >
        {task.title}
      </div>

      {/* Status Row */}
      <div className="mt-2 flex min-w-0 flex-wrap items-center gap-2">
        <Badge
          variant="outline"
          className={cn(
            "max-w-full text-xs font-medium border",
            getStatusBadgeClass(task.internalStatus)
          )}
        >
          {getStatusLabel(task.internalStatus)}
        </Badge>
        <span
          data-testid="review-type-indicator"
          className="inline-flex min-w-0 items-center gap-1 text-xs text-[var(--text-secondary)]"
        >
          {isAiPhase ? (
            <>
              <Bot className="w-4 h-4" />
              AI Review
            </>
          ) : (
            <>
              <User className="w-4 h-4" />
              Human Review
            </>
          )}
        </span>
      </div>

      {/* Description Preview */}
      {task.description && (
        <div className="mt-3">
          <div
            className={cn(
              "min-w-0 overflow-hidden p-2 rounded-[var(--radius-sm)]",
              "bg-[var(--bg-base)]",
              panelPresentation && "max-w-full"
            )}
          >
            <p
              data-testid="task-review-description"
              className={cn(
                "text-sm text-[var(--text-secondary)] italic line-clamp-2 leading-normal break-words",
                panelPresentation && "[overflow-wrap:anywhere]",
              )}
            >
              {task.description}
            </p>
          </div>
        </div>
      )}

      {/* Action Buttons */}
      {onReview && (
        <div className={cn("flex min-w-0 flex-wrap gap-2", panelPresentation ? "mt-3" : "mt-4")}>
          <Button
            data-testid={`review-button-${task.id}`}
            variant="ghost"
            size="sm"
            onClick={() => onReview(task.id)}
            disabled={isLoading}
            className={cn(
              "bg-[var(--accent-muted)] hover:bg-[var(--accent-primary)] hover:text-white text-[var(--accent-primary)]",
              panelPresentation && "w-full min-w-0 px-2"
            )}
          >
            {isLoading ? (
              <Loader2 className="w-4 h-4 mr-1.5 animate-spin" />
            ) : (
              <Eye className="w-4 h-4 mr-1.5" />
            )}
            <span className="truncate">Review</span>
          </Button>
        </div>
      )}
    </Card>
  );
}
