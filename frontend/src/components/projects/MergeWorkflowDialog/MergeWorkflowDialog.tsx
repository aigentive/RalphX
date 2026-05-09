/**
 * MergeWorkflowDialog - Modal for handling post-completion workflow when a project
 * finishes in worktree mode.
 *
 * Presents options for:
 * - Merge to main (creates merge commit)
 * - Rebase onto main (linear history)
 * - Create Pull Request (review first)
 * - Keep worktree (merge manually later)
 * - Discard changes (delete worktree and branch)
 */

import { useState, useCallback, useEffect, useMemo } from "react";
import type { Project } from "@/types/project";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Separator } from "@/components/ui/separator";
import {
  CheckCircle,
  AlertTriangle,
  GitMerge,
  GitPullRequest,
  Trash2,
  FileDiff,
  GitCommit,
  Loader2,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { BranchBadge } from "@/components/shared/BranchBadge";

// ============================================================================
// Types
// ============================================================================

/**
 * Available merge workflow options
 */
export type MergeOption =
  | "merge"
  | "rebase"
  | "create_pr"
  | "keep_worktree"
  | "discard";

/**
 * Completion data passed to the dialog
 */
export interface CompletionData {
  /** Number of commits made by RalphX */
  commitCount: number;
  /** Branch name created by RalphX */
  branchName: string;
}

/**
 * Result returned when workflow is confirmed
 */
export interface MergeWorkflowResult {
  option: MergeOption;
  project: Project;
}

// ============================================================================
// Props Interface
// ============================================================================

export interface MergeWorkflowDialogProps {
  /** Whether the dialog is open */
  isOpen: boolean;
  /** Callback when the dialog is closed/cancelled */
  onClose: () => void;
  /** Callback when a workflow option is confirmed */
  onConfirm: (result: MergeWorkflowResult) => void;
  /** Project that completed */
  project: Project;
  /** Completion data (commit count, branch name) */
  completionData: CompletionData;
  /** Callback to view diff (opens diff viewer) */
  onViewDiff?: () => void;
  /** Callback to view commits (opens commit history) */
  onViewCommits?: () => void;
  /** Whether workflow is in progress */
  isProcessing?: boolean;
  /** Error message to display */
  error?: string | null;
}

// ============================================================================
// Icons for custom options
// ============================================================================

function RebaseIcon({ className }: { className?: string }) {
  return (
    <svg
      className={className}
      width="16"
      height="16"
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.2"
    >
      <circle cx="4" cy="3" r="1.5" />
      <circle cx="4" cy="8" r="1.5" />
      <circle cx="4" cy="13" r="1.5" />
      <path d="M4 4.5V6.5M4 9.5V11.5" />
      <path d="M8 3h4M8 8h4M8 13h4" strokeLinecap="round" />
    </svg>
  );
}

function WorktreeIcon({ className }: { className?: string }) {
  return (
    <svg
      className={className}
      width="16"
      height="16"
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.2"
    >
      <rect x="2" y="2" width="5" height="5" rx="1" />
      <rect x="9" y="9" width="5" height="5" rx="1" />
      <path d="M7 4.5h2M11.5 7v2" strokeLinecap="round" />
    </svg>
  );
}

// ============================================================================
// Option Configuration
// ============================================================================

interface MergeOptionConfig {
  value: MergeOption;
  label: string;
  description: string;
  icon: React.ReactNode;
  destructive?: boolean;
  warning?: string;
}

function getMergeOptions(baseBranch: string): MergeOptionConfig[] {
  return [
    {
      value: "merge",
      label: `Merge to ${baseBranch}`,
      description: "Creates a merge commit preserving branch history",
      icon: <GitMerge className="h-4 w-4" />,
    },
    {
      value: "rebase",
      label: `Rebase onto ${baseBranch}`,
      description: `Replays commits on top of ${baseBranch} for linear history`,
      icon: <RebaseIcon className="h-4 w-4" />,
    },
    {
      value: "create_pr",
      label: "Create Pull Request",
      description: "Opens GitHub to create a PR for code review",
      icon: <GitPullRequest className="h-4 w-4" />,
    },
    {
      value: "keep_worktree",
      label: "Keep worktree",
      description: "Leave as-is and merge manually later",
      icon: <WorktreeIcon className="h-4 w-4" />,
    },
    {
      value: "discard",
      label: "Discard changes",
      description: "Delete the worktree and branch permanently",
      icon: <Trash2 className="h-4 w-4" />,
      destructive: true,
      warning: "This cannot be undone. All commits will be lost.",
    },
  ];
}

// ============================================================================
// Sub-components
// ============================================================================

interface RadioOptionProps {
  config: MergeOptionConfig;
  selected: boolean;
  onSelect: () => void;
  disabled?: boolean;
}

function RadioOption({
  config,
  selected,
  onSelect,
  disabled,
}: RadioOptionProps) {
  const borderColor = selected
    ? config.destructive
      ? "var(--status-error)"
      : "var(--accent-primary)"
    : "var(--border-subtle)";

  return (
    <label
      data-testid={`merge-option-${config.value}`}
      data-selected={selected ? "true" : "false"}
      className={cn(
        "flex gap-3 p-3 rounded-lg transition-colors",
        disabled ? "cursor-not-allowed opacity-50" : "cursor-pointer",
        !selected && !disabled && "hover:bg-[var(--bg-hover)]"
      )}
      style={{
        backgroundColor: selected ? "var(--bg-elevated)" : "transparent",
        border: `1px solid ${borderColor}`,
      }}
    >
      <input
        type="radio"
        name="mergeOption"
        value={config.value}
        checked={selected}
        onChange={onSelect}
        disabled={disabled}
        className="sr-only"
      />
      <span
        className="mt-0.5 w-4 h-4 rounded-full border-2 flex items-center justify-center flex-shrink-0"
        style={{
          borderColor: selected
            ? config.destructive
              ? "var(--status-error)"
              : "var(--accent-primary)"
            : "var(--border-subtle)",
        }}
      >
        {selected && (
          <span
            className="w-2 h-2 rounded-full"
            style={{
              backgroundColor: config.destructive
                ? "var(--status-error)"
                : "var(--accent-primary)",
            }}
          />
        )}
      </span>
      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-2">
          <span
            style={{
              color: config.destructive
                ? "var(--status-error)"
                : "var(--text-primary)",
            }}
          >
            {config.icon}
          </span>
          <span
            className="text-sm font-medium"
            style={{
              color: config.destructive
                ? "var(--status-error)"
                : "var(--text-primary)",
            }}
          >
            {config.label}
          </span>
        </div>
        <div className="text-xs mt-0.5 text-[var(--text-muted)]">
          {config.description}
        </div>
        {selected && config.warning && (
          <div className="flex items-center gap-1.5 text-xs mt-2 text-[var(--status-warning)]">
            <AlertTriangle className="h-3.5 w-3.5" />
            <span>{config.warning}</span>
          </div>
        )}
      </div>
    </label>
  );
}

// ============================================================================
// Main Component
// ============================================================================

export function MergeWorkflowDialog({
  isOpen,
  onClose,
  onConfirm,
  project,
  completionData,
  onViewDiff,
  onViewCommits,
  isProcessing = false,
  error = null,
}: MergeWorkflowDialogProps) {
  const [selectedOption, setSelectedOption] = useState<MergeOption>("merge");
  const [showDiscardConfirm, setShowDiscardConfirm] = useState(false);

  const mergeOptions = useMemo(
    () => getMergeOptions(project.baseBranch ?? "main"),
    [project.baseBranch]
  );

  // Reset state when dialog opens
  useEffect(() => {
    if (isOpen) {
      setSelectedOption("merge");
      setShowDiscardConfirm(false);
    }
  }, [isOpen]);

  // Handle confirm
  const handleConfirm = useCallback(() => {
    // If discard is selected and not yet confirmed, show confirmation
    if (selectedOption === "discard" && !showDiscardConfirm) {
      setShowDiscardConfirm(true);
      return;
    }

    onConfirm({
      option: selectedOption,
      project,
    });
  }, [selectedOption, showDiscardConfirm, onConfirm, project]);

  // Handle option selection
  const handleOptionSelect = useCallback((option: MergeOption) => {
    setSelectedOption(option);
    setShowDiscardConfirm(false);
  }, []);

  // Handle dialog close
  const handleOpenChange = useCallback(
    (open: boolean) => {
      if (!open && !isProcessing) {
        onClose();
      }
    },
    [isProcessing, onClose]
  );

  const isDiscardSelected = selectedOption === "discard";
  const buttonLabel = isProcessing
    ? "Processing..."
    : showDiscardConfirm
      ? "Confirm Discard"
      : "Continue";

  return (
    <Dialog open={isOpen} onOpenChange={handleOpenChange}>
      <DialogContent
        data-testid="merge-workflow-dialog"
        className="max-w-lg p-0"
        aria-describedby={undefined}
        onEscapeKeyDown={(e) => {
          if (isProcessing) {
            e.preventDefault();
          }
        }}
      >
        {/* Header */}
        <DialogHeader className="px-6 py-4 border-b border-[var(--border-subtle)]">
          <div className="flex items-center gap-3">
            <CheckCircle className="h-5 w-5 text-[var(--status-success)]" />
            <DialogTitle className="text-lg font-semibold text-[var(--text-primary)] tracking-tight">
              Project Complete: {project.name}
            </DialogTitle>
          </div>
        </DialogHeader>

        {/* Content */}
        <div className="px-6 py-5 space-y-5">
          {/* Summary */}
          <div className="text-sm text-[var(--text-secondary)]">
            RalphX made{" "}
            <span
              data-testid="commit-count"
              className="font-medium text-[var(--text-primary)]"
            >
              {completionData.commitCount} commit
              {completionData.commitCount !== 1 ? "s" : ""}
            </span>{" "}
            on branch:{" "}
            <span data-testid="branch-name">
              <BranchBadge branch={completionData.branchName} variant="default" />
            </span>
          </div>

          {/* Action Buttons */}
          <div className="flex gap-2">
            {onViewDiff && (
              <Button
                data-testid="view-diff-button"
                type="button"
                onClick={onViewDiff}
                disabled={isProcessing}
                variant="secondary"
                className="gap-2 bg-[var(--bg-elevated)] text-[var(--text-primary)] hover:bg-[var(--bg-hover)] border-0"
              >
                <FileDiff className="h-3.5 w-3.5" />
                View Diff
              </Button>
            )}
            {onViewCommits && (
              <Button
                data-testid="view-commits-button"
                type="button"
                onClick={onViewCommits}
                disabled={isProcessing}
                variant="secondary"
                className="gap-2 bg-[var(--bg-elevated)] text-[var(--text-primary)] hover:bg-[var(--bg-hover)] border-0"
              >
                <GitCommit className="h-3.5 w-3.5" />
                View Commits
              </Button>
            )}
          </div>

          <Separator className="bg-[var(--border-subtle)]" />

          {/* Question */}
          <div className="text-sm font-medium text-[var(--text-secondary)]">
            What would you like to do?
          </div>

          {/* Options */}
          <div className="space-y-2">
            {mergeOptions.map((config) => (
              <RadioOption
                key={config.value}
                config={config}
                selected={selectedOption === config.value}
                onSelect={() => handleOptionSelect(config.value)}
                disabled={isProcessing}
              />
            ))}
          </div>

          {/* Error Message */}
          {error && (
            <div
              data-testid="dialog-error"
              className="flex items-center gap-2 px-3 py-2 rounded-lg bg-[var(--status-error-muted)] text-[var(--status-error)]"
            >
              <AlertTriangle className="h-3.5 w-3.5" />
              <span className="text-sm">{error}</span>
            </div>
          )}

          {/* Discard Confirmation */}
          {showDiscardConfirm && (
            <div
              data-testid="discard-confirmation"
              className="flex items-center gap-2 px-3 py-2 rounded-lg bg-[var(--status-error-muted)] text-[var(--status-error)]"
            >
              <AlertTriangle className="h-3.5 w-3.5" />
              <span className="text-sm">
                Are you sure? Click "Confirm Discard" to permanently delete the
                worktree and branch.
              </span>
            </div>
          )}
        </div>

        {/* Footer */}
        <DialogFooter className="px-6 py-4 border-t border-[var(--border-subtle)] gap-3 sm:gap-3">
          <Button
            data-testid="cancel-button"
            type="button"
            onClick={onClose}
            disabled={isProcessing}
            variant="ghost"
            className="bg-[var(--bg-elevated)] text-[var(--text-primary)] hover:bg-[var(--bg-hover)]"
          >
            Cancel
          </Button>
          <Button
            data-testid="confirm-button"
            type="button"
            onClick={handleConfirm}
            disabled={isProcessing}
            className={cn(
              "gap-2",
              isProcessing
                ? "bg-[var(--bg-hover)] text-[var(--text-muted)] cursor-not-allowed"
                : isDiscardSelected
                  ? "bg-[var(--status-error)] text-white hover:bg-[var(--status-error)]/90"
                  : "bg-[var(--accent-primary)] text-white hover:bg-[var(--accent-primary)]/90"
            )}
          >
            {isProcessing && <Loader2 className="h-4 w-4 animate-spin" />}
            {buttonLabel}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
