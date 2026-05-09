/**
 * TaskRerunDialog - Modal for handling re-run workflow when dragging a Done
 * task back to Planned.
 *
 * Presents options for:
 * - Keep changes (recommended) - AI sees current state
 * - Revert commit - Undo the original work
 * - Create new task - Keep original completed, spawn new
 */

import { useState, useCallback, useEffect } from "react";
import type { Task } from "@/types/task";
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
  RefreshCw,
  AlertTriangle,
  Check,
  Undo,
  Plus,
  Loader2,
} from "lucide-react";
import { cn } from "@/lib/utils";

// ============================================================================
// Types
// ============================================================================

/**
 * Available re-run options
 */
export type RerunOption = "keep_changes" | "revert_commit" | "create_new";

/**
 * Commit information for the completed task
 */
export interface CommitInfo {
  /** Short SHA of the commit */
  sha: string;
  /** Commit message */
  message: string;
  /** Whether there are commits that depend on this one */
  hasDependentCommits: boolean;
}

/**
 * Result returned when re-run is confirmed
 */
export interface TaskRerunResult {
  option: RerunOption;
  task: Task;
  /** Optional note to communicate intent to the re-executing agent */
  note?: string;
}

// ============================================================================
// Props Interface
// ============================================================================

export interface TaskRerunDialogProps {
  /** Whether the dialog is open */
  isOpen: boolean;
  /** Callback when the dialog is closed/cancelled */
  onClose: () => void;
  /** Callback when a re-run option is confirmed */
  onConfirm: (result: TaskRerunResult) => void;
  /** Task being re-run */
  task: Task;
  /** Commit information associated with the task */
  commitInfo: CommitInfo;
  /** Whether re-run is in progress */
  isProcessing?: boolean;
  /** Error message to display */
  error?: string | null;
}

// ============================================================================
// Option Configuration
// ============================================================================

interface RerunOptionConfig {
  value: RerunOption;
  label: string;
  description: string;
  icon: React.ReactNode;
  recommended?: boolean;
  warning?: boolean;
}

const RERUN_OPTIONS: RerunOptionConfig[] = [
  {
    value: "keep_changes",
    label: "Keep changes, run task again",
    description:
      "AI will see current code state and make additional changes if needed",
    icon: <Check className="h-4 w-4" />,
    recommended: true,
  },
  {
    value: "revert_commit",
    label: "Revert commit, then run task",
    description: "Undo the previous work before re-executing",
    icon: <Undo className="h-4 w-4" />,
    warning: true,
  },
  {
    value: "create_new",
    label: "Create new task instead",
    description: "Original task stays completed, new task created",
    icon: <Plus className="h-4 w-4" />,
  },
];

// ============================================================================
// Sub-components
// ============================================================================

interface RadioOptionProps {
  config: RerunOptionConfig;
  selected: boolean;
  onSelect: () => void;
  disabled?: boolean;
  showWarning?: boolean;
}

function RadioOption({
  config,
  selected,
  onSelect,
  disabled,
  showWarning,
}: RadioOptionProps) {
  const isWarningOption = config.warning && selected && showWarning;

  const borderColor = selected
    ? isWarningOption
      ? "var(--status-warning)"
      : "var(--accent-primary)"
    : "var(--border-subtle)";

  return (
    <label
      data-testid={`rerun-option-${config.value}`}
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
        name="rerunOption"
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
            ? isWarningOption
              ? "var(--status-warning)"
              : "var(--accent-primary)"
            : "var(--border-subtle)",
        }}
      >
        {selected && (
          <span
            className="w-2 h-2 rounded-full"
            style={{
              backgroundColor: isWarningOption
                ? "var(--status-warning)"
                : "var(--accent-primary)",
            }}
          />
        )}
      </span>
      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-2">
          <span
            style={{
              color: isWarningOption
                ? "var(--status-warning)"
                : "var(--text-primary)",
            }}
          >
            {config.icon}
          </span>
          <span
            className="text-sm font-medium"
            style={{
              color: isWarningOption
                ? "var(--status-warning)"
                : "var(--text-primary)",
            }}
          >
            {config.label}
          </span>
          {config.recommended && (
            <span
              className="px-1.5 py-0.5 text-xs rounded"
              style={{
                backgroundColor: "var(--accent-muted)",
                color: "var(--accent-primary)",
              }}
            >
              Recommended
            </span>
          )}
        </div>
        <div className="text-xs mt-0.5 text-[var(--text-muted)]">
          {config.description}
        </div>
      </div>
    </label>
  );
}

// ============================================================================
// Main Component
// ============================================================================

export function TaskRerunDialog({
  isOpen,
  onClose,
  onConfirm,
  task,
  commitInfo,
  isProcessing = false,
  error = null,
}: TaskRerunDialogProps) {
  const [selectedOption, setSelectedOption] =
    useState<RerunOption>("keep_changes");
  const [note, setNote] = useState("");

  // Reset state when dialog opens
  useEffect(() => {
    if (isOpen) {
      setSelectedOption("keep_changes");
      setNote("");
    }
  }, [isOpen]);

  // Handle confirm
  const handleConfirm = useCallback(() => {
    const trimmedNote = note.trim();
    onConfirm({
      option: selectedOption,
      task,
      ...(trimmedNote ? { note: trimmedNote } : {}),
    });
  }, [selectedOption, onConfirm, task, note]);

  // Handle option selection
  const handleOptionSelect = useCallback((option: RerunOption) => {
    setSelectedOption(option);
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

  const isRevertSelected = selectedOption === "revert_commit";
  const showDependentWarning = isRevertSelected && commitInfo.hasDependentCommits;

  return (
    <Dialog open={isOpen} onOpenChange={handleOpenChange}>
      <DialogContent
        data-testid="task-rerun-dialog"
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
            <RefreshCw className="h-5 w-5 text-[var(--accent-primary)]" />
            <DialogTitle
              data-testid="dialog-title"
              className="text-lg font-semibold text-[var(--text-primary)] tracking-tight"
            >
              Re-run Task
            </DialogTitle>
          </div>
        </DialogHeader>

        {/* Content */}
        <div className="px-6 py-5 space-y-5">
          {/* Task Info */}
          <div>
            <div
              data-testid="task-title"
              className="text-base font-medium text-[var(--text-primary)]"
            >
              "{task.title}"
            </div>
          </div>

          {/* Commit Info */}
          <div className="text-sm text-[var(--text-secondary)]">
            <span>This task was completed with commit: </span>
            <span
              data-testid="commit-sha"
              className="font-mono font-medium text-[var(--accent-primary)]"
            >
              {commitInfo.sha}
            </span>
            <div
              data-testid="commit-message"
              className="mt-1 italic text-[var(--text-muted)]"
            >
              "{commitInfo.message}"
            </div>
          </div>

          <Separator className="bg-[var(--border-subtle)]" />

          {/* Question */}
          <div className="text-sm font-medium text-[var(--text-secondary)]">
            How should we handle the previous work?
          </div>

          {/* Options */}
          <div className="space-y-2">
            {RERUN_OPTIONS.map((config) => (
              <RadioOption
                key={config.value}
                config={config}
                selected={selectedOption === config.value}
                onSelect={() => handleOptionSelect(config.value)}
                disabled={isProcessing}
                showWarning={commitInfo.hasDependentCommits}
              />
            ))}
          </div>

          {/* Note textarea */}
          <div>
            <textarea
              data-testid="rerun-note-textarea"
              value={note}
              onChange={(e) => setNote(e.target.value)}
              disabled={isProcessing}
              placeholder="Optional: tell the agent what to do differently..."
              rows={3}
              className="w-full resize-none rounded-md px-3 py-2 text-sm transition-colors disabled:opacity-40 outline-none ring-0 focus:ring-0 focus:outline-none focus-visible:outline-none border border-[var(--border-subtle)]"
              style={{
                backgroundColor: "var(--bg-surface)",
                color: "var(--text-primary)",
                boxShadow: "none",
                outline: "none",
              }}
            />
          </div>

          {/* Dependent Commits Warning */}
          {showDependentWarning && (
            <div
              data-testid="dependent-commits-warning"
              className="flex items-start gap-2 px-3 py-2.5 rounded-lg bg-[var(--status-warning-muted)] text-[var(--status-warning)]"
            >
              <AlertTriangle className="h-3.5 w-3.5 mt-0.5 flex-shrink-0" />
              <span className="text-sm">
                Warning: Other commits depend on this one. Reverting may cause
                conflicts or break code that was built on top of these changes.
              </span>
            </div>
          )}

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
                : "bg-[var(--accent-primary)] text-white hover:bg-[var(--accent-primary)]/90"
            )}
          >
            {isProcessing && <Loader2 className="h-4 w-4 animate-spin" />}
            {isProcessing ? "Processing..." : "Confirm Re-run"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
