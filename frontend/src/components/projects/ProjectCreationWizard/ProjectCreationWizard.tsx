/**
 * ProjectCreationWizard - Modal for creating a new project
 *
 * Uses Isolated Worktrees: each task gets a separate working directory.
 */

import { useState, useEffect, useCallback, useMemo } from "react";
import type { CreateProject } from "@/types/project";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Separator } from "@/components/ui/separator";
import {
  FolderOpen,
  AlertTriangle,
  GitBranch,
  Loader2,
  ChevronDown,
  Settings,
} from "lucide-react";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import { cn } from "@/lib/utils";
import {
  type FormState,
  generateWorktreePath,
  extractFolderName,
  validateForm,
} from "./ProjectCreationWizard.helpers";

// ============================================================================
// Props Interface
// ============================================================================

export interface ProjectCreationWizardProps {
  /** Whether the modal is open */
  isOpen: boolean;
  /** Callback when the modal is closed */
  onClose: () => void;
  /** Callback when a project is created */
  onCreate: (project: CreateProject) => void;
  /** Callback to open folder picker, returns selected path or null if cancelled */
  onBrowseFolder?: () => Promise<string | null>;
  /** Callback to fetch available branches for base branch dropdown */
  onFetchBranches?: (workingDirectory: string) => Promise<string[]>;
  /** Callback to detect the default branch for a repository */
  onDetectDefaultBranch?: (workingDirectory: string) => Promise<string>;
  /** Whether creation is in progress */
  isCreating?: boolean;
  /** Error message to display */
  error?: string | null;
  /** Whether this is the first-run mode (no existing projects) - disables close/cancel */
  isFirstRun?: boolean;
}

// ============================================================================
// Main Component
// ============================================================================

export function ProjectCreationWizard({
  isOpen,
  onClose,
  onCreate,
  onBrowseFolder,
  onFetchBranches,
  onDetectDefaultBranch,
  isCreating = false,
  error = null,
  isFirstRun = false,
}: ProjectCreationWizardProps) {
  // Form state
  const [form, setForm] = useState<FormState>({
    name: "",
    workingDirectory: "",
    gitMode: "worktree",
    baseBranch: "main",
    worktreeParentDirectory: "",
  });

  // Available branches for dropdown
  const [branches, setBranches] = useState<string[]>(["main", "master"]);
  const [loadingBranches, setLoadingBranches] = useState(false);

  // Touched fields for validation display
  const [touched, setTouched] = useState<Record<string, boolean>>({});

  // Track if form was submitted (to show all errors)
  const [submitted, setSubmitted] = useState(false);

  // Track if user has manually typed a custom name (to preserve override)
  const [isNameManuallySet, setIsNameManuallySet] = useState(false);
  // Track previously inferred name to compare against
  const [lastInferredName, setLastInferredName] = useState("");

  // Validate form
  const errors = useMemo(() => validateForm(form), [form]);
  const hasErrors = Object.keys(errors).length > 0;

  // Generate worktree path (uses custom parent directory if provided)
  const worktreePath = useMemo(
    () => generateWorktreePath(form.workingDirectory, form.worktreeParentDirectory),
    [form.workingDirectory, form.worktreeParentDirectory]
  );

  // Track advanced settings visibility
  const [showAdvanced, setShowAdvanced] = useState(false);

  // Fetch branches and detect default branch when working directory changes
  useEffect(() => {
    if (!form.workingDirectory) return;

    const fetchData = async () => {
      setLoadingBranches(true);

      try {
        // Fetch branches and detect default branch in parallel
        const [fetchedBranches, detectedDefault] = await Promise.all([
          onFetchBranches?.(form.workingDirectory) ?? Promise.resolve([]),
          onDetectDefaultBranch?.(form.workingDirectory).catch(() => null) ?? Promise.resolve(null),
        ]);

        if (fetchedBranches.length > 0) {
          setBranches(fetchedBranches);

          // Priority: detected default > main > master > first in list
          let baseBranch: string | undefined;

          if (detectedDefault && fetchedBranches.includes(detectedDefault)) {
            // Use detected default branch if it exists in the branch list
            baseBranch = detectedDefault;
          } else {
            // Fall back to main/master if detection failed or branch not in list
            baseBranch = fetchedBranches.find((b) => b === "main" || b === "master");
          }

          if (baseBranch) {
            setForm((prev) => ({ ...prev, baseBranch }));
          }
        }
      } finally {
        setLoadingBranches(false);
      }
    };

    fetchData();
  }, [form.workingDirectory, onFetchBranches, onDetectDefaultBranch]);

  // Reset form when modal opens - defaults to Worktree mode (recommended)
  useEffect(() => {
    if (isOpen) {
      setForm({
        name: "",
        workingDirectory: "",
        gitMode: "worktree",
        baseBranch: "main",
        worktreeParentDirectory: "",
      });
      setTouched({});
      setSubmitted(false);
      setBranches(["main", "master"]);
      setIsNameManuallySet(false);
      setLastInferredName("");
      setShowAdvanced(false);
    }
  }, [isOpen]);

  // Handle folder browse - also auto-infer project name from folder
  const handleBrowse = useCallback(async () => {
    if (onBrowseFolder) {
      const path = await onBrowseFolder();
      if (path) {
        const inferredName = extractFolderName(path);
        setForm((prev) => {
          // Only auto-fill name if:
          // 1. User hasn't manually typed a custom name, OR
          // 2. Current name matches the last inferred name (not overridden)
          const shouldAutoFill = !isNameManuallySet || prev.name === lastInferredName || prev.name === "";
          return {
            ...prev,
            workingDirectory: path,
            name: shouldAutoFill ? inferredName : prev.name,
          };
        });
        setLastInferredName(inferredName);
        setTouched((prev) => ({ ...prev, workingDirectory: true }));
      }
    }
  }, [onBrowseFolder, isNameManuallySet, lastInferredName]);

  // Handle project name change - track if user manually set it
  const handleNameChange = useCallback((value: string) => {
    setForm((prev) => ({ ...prev, name: value }));
    // Mark as manually set only if user typed something different from inferred
    if (value !== lastInferredName) {
      setIsNameManuallySet(true);
    }
  }, [lastInferredName]);

  // Handle form submission
  const handleSubmit = useCallback(() => {
    // Mark as submitted to show all errors
    setSubmitted(true);

    // Mark all fields as touched to show errors
    setTouched({
      name: true,
      workingDirectory: true,
      baseBranch: true,
    });

    // Check validation - use the errors object directly instead of hasErrors
    // because hasErrors might not reflect the current form state
    const currentErrors = validateForm(form);
    if (Object.keys(currentErrors).length > 0) return;

    // Use provided name or infer from folder path
    const projectName = form.name.trim() || extractFolderName(form.workingDirectory);

    const project: CreateProject = {
      name: projectName,
      workingDirectory: form.workingDirectory.trim(),
      gitMode: "worktree",
      baseBranch: form.baseBranch.trim(),
      worktreeParentDirectory:
        form.worktreeParentDirectory.trim() || "~/ralphx-worktrees",
    };

    onCreate(project);
  }, [form, onCreate]);

  // Handle dialog close - disabled in first-run mode
  const handleOpenChange = useCallback((open: boolean) => {
    if (!open && !isFirstRun && !isCreating) {
      onClose();
    }
  }, [isFirstRun, isCreating, onClose]);

  return (
    <Dialog open={isOpen} onOpenChange={handleOpenChange}>
      <DialogContent
        data-testid="project-creation-wizard"
        hideCloseButton={isFirstRun}
        className="max-w-lg p-0"
        aria-describedby={undefined}
        onPointerDownOutside={(e) => {
          // Prevent closing on backdrop click in first-run mode
          if (isFirstRun || isCreating) {
            e.preventDefault();
          }
        }}
        onEscapeKeyDown={(e) => {
          // Prevent closing on Escape in first-run mode
          if (isFirstRun || isCreating) {
            e.preventDefault();
          }
        }}
      >
        {/* Header */}
        <DialogHeader className="px-6 py-4 border-b border-[var(--border-subtle)]">
          <DialogTitle className="text-lg font-semibold text-[var(--text-primary)] tracking-tight">
            Create New Project
          </DialogTitle>
        </DialogHeader>

        {/* Content */}
        <div className="px-6 py-5 space-y-5">
          {/* Location (Folder) - FIRST */}
          <div className="space-y-1.5">
            <Label
              htmlFor="folder-input"
              className="text-sm font-medium text-[var(--text-secondary)]"
            >
              Location <span className="text-[var(--status-error)]">*</span>
            </Label>
            <div className="flex gap-2">
              <Input
                id="folder-input"
                data-testid="folder-input"
                type="text"
                value={form.workingDirectory}
                onChange={(e) =>
                  setForm((prev) => ({ ...prev, workingDirectory: e.target.value }))
                }
                placeholder="Select a folder..."
                readOnly
                disabled={isCreating}
                className={cn(
                  "flex-1 h-10 px-3 py-2 rounded-lg text-sm bg-[var(--bg-base)] border text-[var(--text-primary)] placeholder:text-[var(--text-muted)] focus:ring-2 focus:ring-[var(--accent-primary)] focus:border-[var(--accent-primary)]",
                  (touched.workingDirectory || submitted) && errors.workingDirectory
                    ? "border-[var(--status-error)]"
                    : "border-[var(--border-subtle)]",
                  isCreating && "opacity-50"
                )}
              />
              {onBrowseFolder && (
                <Button
                  data-testid="browse-button"
                  type="button"
                  onClick={handleBrowse}
                  disabled={isCreating}
                  variant="secondary"
                  className="h-10 px-3 gap-2 bg-[var(--bg-elevated)] text-[var(--text-primary)] hover:bg-[var(--bg-hover)] border-0"
                >
                  <FolderOpen className="h-4 w-4" />
                  Browse
                </Button>
              )}
            </div>
            {(touched.workingDirectory || submitted) && errors.workingDirectory && (
              <p
                data-testid="folder-input-error"
                className="text-xs text-[var(--status-error)]"
              >
                {errors.workingDirectory}
              </p>
            )}
          </div>

          {/* Project Name - SECOND (optional, auto-inferred from folder) */}
          <div className="space-y-1.5">
            <Label
              htmlFor="project-name-input"
              className="text-sm font-medium text-[var(--text-secondary)]"
            >
              Project Name{" "}
              <span className="text-[var(--text-muted)]">(optional)</span>
            </Label>
            <Input
              id="project-name-input"
              data-testid="project-name-input"
              type="text"
              value={form.name}
              onChange={(e) => handleNameChange(e.target.value)}
              placeholder={
                form.workingDirectory
                  ? extractFolderName(form.workingDirectory) || "my-app"
                  : "Auto-inferred from folder"
              }
              disabled={isCreating}
              className={cn(
                "h-10 px-3 py-2 rounded-lg text-sm bg-[var(--bg-base)] border border-[var(--border-subtle)] text-[var(--text-primary)] placeholder:text-[var(--text-muted)] focus:ring-2 focus:ring-[var(--accent-primary)] focus:border-[var(--accent-primary)]",
                isCreating && "opacity-50"
              )}
            />
            <p className="text-xs text-[var(--text-muted)]">
              RalphX initializes local Git in this folder. No remote repository is created; you can connect GitHub later.
            </p>
          </div>

          <Separator className="bg-[var(--border-subtle)]" />

          {/* Git Settings */}
          <div className="space-y-3">
            <Label className="text-sm font-medium text-[var(--text-secondary)]">
              Git Settings
            </Label>

            <div className="space-y-3">
              <div className="space-y-1.5">
                <Label
                  htmlFor="base-branch-select"
                  className="text-sm font-medium text-[var(--text-secondary)]"
                >
                  Base branch
                </Label>
                <Select
                  value={form.baseBranch}
                  onValueChange={(value) =>
                    setForm((prev) => ({ ...prev, baseBranch: value }))
                  }
                  disabled={isCreating || loadingBranches}
                >
                  <SelectTrigger
                    data-testid="base-branch-select"
                    className={cn(
                      "h-10 px-3 py-2 rounded-lg text-sm bg-[var(--bg-base)] border text-[var(--text-primary)] focus:ring-2 focus:ring-[var(--accent-primary)] focus:border-[var(--accent-primary)]",
                      (touched.baseBranch || submitted) && errors.baseBranch
                        ? "border-[var(--status-error)]"
                        : "border-[var(--border-subtle)]",
                      (isCreating || loadingBranches) && "opacity-50"
                    )}
                  >
                    <SelectValue
                      placeholder={
                        loadingBranches ? "Loading branches..." : "Select base branch"
                      }
                    />
                  </SelectTrigger>
                  <SelectContent className="bg-[var(--bg-elevated)] border-[var(--border-subtle)]">
                    {branches.length === 0 ? (
                      <SelectItem value="_none" disabled>
                        No branches available
                      </SelectItem>
                    ) : (
                      branches.map((branch) => (
                        <SelectItem key={branch} value={branch}>
                          {branch}
                        </SelectItem>
                      ))
                    )}
                  </SelectContent>
                </Select>
                {(touched.baseBranch || submitted) && errors.baseBranch && (
                  <p
                    data-testid="base-branch-select-error"
                    className="text-xs text-[var(--status-error)]"
                  >
                    {errors.baseBranch}
                  </p>
                )}
              </div>

              {/* Worktree Path Display */}
              <div
                data-testid="worktree-path-display"
                className="flex items-center gap-2 px-3 py-2 rounded-lg bg-[var(--bg-base)]"
              >
                <GitBranch className="h-3.5 w-3.5 text-[var(--text-muted)]" />
                <div className="flex-1 min-w-0">
                  <div className="text-xs font-medium text-[var(--text-muted)]">
                    Worktree location
                  </div>
                  <div className="text-sm truncate text-[var(--text-primary)]">
                    {worktreePath}
                  </div>
                </div>
              </div>

              {/* Advanced Settings */}
              <Collapsible open={showAdvanced} onOpenChange={setShowAdvanced}>
                <CollapsibleTrigger
                  data-testid="advanced-settings-trigger"
                  className="flex items-center gap-2 text-xs text-[var(--text-muted)] hover:text-[var(--text-secondary)] transition-colors"
                >
                  <Settings className="h-3 w-3" />
                  <span>Advanced Settings</span>
                  <ChevronDown
                    className={cn(
                      "h-3 w-3 transition-transform",
                      showAdvanced && "rotate-180"
                    )}
                  />
                </CollapsibleTrigger>
                <CollapsibleContent className="mt-3 space-y-3 animate-in slide-in-from-top-2 fade-in duration-200">
                  <div className="space-y-1.5">
                    <Label
                      htmlFor="worktree-parent-input"
                      className="text-sm font-medium text-[var(--text-secondary)]"
                    >
                      Worktree Parent Directory
                    </Label>
                    <Input
                      id="worktree-parent-input"
                      data-testid="worktree-parent-input"
                      type="text"
                      value={form.worktreeParentDirectory}
                      onChange={(e) =>
                        setForm((prev) => ({ ...prev, worktreeParentDirectory: e.target.value }))
                      }
                      placeholder="~/ralphx-worktrees"
                      disabled={isCreating}
                      className={cn(
                        "h-10 px-3 py-2 rounded-lg text-sm bg-[var(--bg-base)] border border-[var(--border-subtle)] text-[var(--text-primary)] placeholder:text-[var(--text-muted)] focus:ring-2 focus:ring-[var(--accent-primary)] focus:border-[var(--accent-primary)]",
                        isCreating && "opacity-50"
                      )}
                    />
                    <p className="text-xs text-[var(--text-muted)]">
                      Default: ~/ralphx-worktrees. Task worktrees will be created inside this directory.
                    </p>
                  </div>
                </CollapsibleContent>
              </Collapsible>
            </div>
          </div>

          {/* Error Message */}
          {error && (
            <div
              data-testid="wizard-error"
              className="flex items-center gap-2 px-3 py-2 rounded-lg bg-[var(--status-error-muted)] text-[var(--status-error)]"
            >
              <AlertTriangle className="h-3.5 w-3.5" />
              <span className="text-sm">{error}</span>
            </div>
          )}
        </div>

        {/* Footer */}
        <DialogFooter className="px-6 py-4 border-t border-[var(--border-subtle)] gap-3 sm:gap-3">
          {/* ESC key hint when modal can be closed */}
          {!isFirstRun && !isCreating && (
            <span className="mr-auto text-xs text-[var(--text-muted)]">
              Press <kbd className="px-1.5 py-0.5 rounded bg-[var(--bg-base)] border border-[var(--border-subtle)] font-mono text-[0.625rem]">ESC</kbd> to cancel
            </span>
          )}
          {/* Cancel button hidden in first-run mode */}
          {!isFirstRun && (
            <Button
              data-testid="cancel-button"
              type="button"
              onClick={onClose}
              disabled={isCreating}
              variant="ghost"
              className="bg-[var(--bg-elevated)] text-[var(--text-primary)] hover:bg-[var(--bg-hover)]"
            >
              Cancel
            </Button>
          )}
          <Button
            data-testid="create-button"
            type="button"
            onClick={handleSubmit}
            disabled={isCreating || (submitted && hasErrors)}
            className={cn(
              "gap-2",
              isCreating || (submitted && hasErrors)
                ? "bg-[var(--bg-hover)] text-[var(--text-muted)] cursor-not-allowed"
                : "bg-[var(--accent-primary)] text-white hover:bg-[var(--accent-primary)]/90"
            )}
          >
            {isCreating && <Loader2 className="h-4 w-4 animate-spin" />}
            {isCreating ? "Creating..." : "Create Project"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
