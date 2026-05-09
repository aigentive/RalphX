/**
 * StatusDropdown - Dropdown showing valid status transitions for a task
 * Fetches valid transitions from the backend state machine
 */

import { useQuery } from "@tanstack/react-query";
import { Loader2 } from "lucide-react";
import { api } from "@/lib/tauri";
import type { InternalStatus } from "@/types/task";
import type { StatusTransition } from "@/types/task";
import { useConfirmation } from "@/hooks/useConfirmation";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

interface StatusDropdownProps {
  /** Task ID to fetch transitions for */
  taskId: string;
  /** Current status of the task */
  currentStatus: InternalStatus;
  /** Callback when a transition is selected */
  onTransition: (newStatus: string) => void;
  /** Whether the dropdown is disabled */
  disabled?: boolean;
}

/**
 * Map internal status to color variable
 */
function getStatusColor(status: string): string {
  // Terminal statuses
  if (status === "approved" || status === "review_passed") return "var(--status-success)";
  if (status === "failed" || status === "cancelled") return "var(--status-error)";

  // Active/working statuses
  if (status === "executing" || status === "qa_testing" || status === "reviewing") return "var(--status-info)";
  if (status === "pending_review" || status === "revision_needed") return "var(--status-warning)";
  if (status === "re_executing") return "var(--accent-primary)";

  // Idle statuses
  if (status === "backlog" || status === "ready") return "var(--text-muted)";
  if (status === "blocked") return "var(--status-warning)";

  // QA statuses
  if (status === "qa_passed") return "var(--status-success)";
  if (status === "qa_failed") return "var(--status-error)";
  if (status === "qa_refining") return "var(--status-info)";

  // Default
  return "var(--text-muted)";
}

/**
 * Get user-friendly display label for status
 */
function getStatusLabel(status: string): string {
  const labels: Record<string, string> = {
    backlog: "Backlog",
    ready: "Ready",
    blocked: "Blocked",
    executing: "Executing",
    qa_refining: "QA Refining",
    qa_testing: "QA Testing",
    qa_passed: "QA Passed",
    qa_failed: "QA Failed",
    pending_review: "Pending Review",
    revision_needed: "Needs Revision",
    approved: "Approved",
    failed: "Failed",
    cancelled: "Cancelled",
    reviewing: "AI Review in Progress",
    review_passed: "AI Review Passed",
    re_executing: "Re-executing",
  };
  return labels[status] || status;
}

export function StatusDropdown({
  taskId,
  currentStatus,
  onTransition,
  disabled = false,
}: StatusDropdownProps) {
  const { confirm, confirmationDialogProps, ConfirmationDialog } = useConfirmation();

  // Handler for status transition with confirmation
  const handleTransition = async (newStatus: string, label: string) => {
    const confirmed = await confirm({
      title: `Change status to ${label}?`,
      description: `This will move the task to ${label}.`,
      confirmText: "Change Status",
    });
    if (!confirmed) return;
    onTransition(newStatus);
  };

  // Fetch valid transitions from backend
  const { data: validTransitions, isLoading, isError } = useQuery({
    queryKey: ["valid-transitions", taskId],
    queryFn: () => api.tasks.getValidTransitions(taskId),
    staleTime: 60 * 1000, // 1 minute - transitions don't change often
  });

  // Show loading state
  if (isLoading) {
    return (
      <Button variant="outline" size="sm" disabled>
        <Loader2 className="h-4 w-4 animate-spin" />
      </Button>
    );
  }

  // Show error state
  if (isError || !validTransitions) {
    return (
      <Button variant="outline" size="sm" disabled>
        Error
      </Button>
    );
  }

  // If no transitions available, show read-only badge
  if (validTransitions.length === 0) {
    return (
      <div
        className="inline-flex items-center rounded-md px-2.5 py-1 text-xs font-medium"
        style={{
          backgroundColor: getStatusColor(currentStatus),
          color: "var(--bg-base)",
        }}
      >
        {getStatusLabel(currentStatus)}
      </div>
    );
  }

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          variant="outline"
          size="sm"
          disabled={disabled}
          className={cn(
            "gap-1.5",
            disabled && "opacity-50 cursor-not-allowed"
          )}
        >
          <span
            className="inline-flex items-center gap-1.5"
            style={{ color: getStatusColor(currentStatus) }}
          >
            <span
              className="h-2 w-2 rounded-full"
              style={{ backgroundColor: getStatusColor(currentStatus) }}
            />
            {getStatusLabel(currentStatus)}
          </span>
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="min-w-[160px]">
        {validTransitions.map((transition: StatusTransition) => (
          <DropdownMenuItem
            key={transition.status}
            onClick={() => handleTransition(transition.status, transition.label)}
            className="cursor-pointer"
          >
            <span
              className="h-2 w-2 rounded-full mr-2"
              style={{ backgroundColor: getStatusColor(transition.status) }}
            />
            <span>{transition.label}</span>
          </DropdownMenuItem>
        ))}
      </DropdownMenuContent>
      <ConfirmationDialog {...confirmationDialogProps} />
    </DropdownMenu>
  );
}
