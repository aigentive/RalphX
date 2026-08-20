/**
 * GitSettingsSection - base branch + worktree location + advanced settings.
 * Shared by ExistingRepositoryStep (branch select fed by the probe),
 * NewRepositoryStep (free-text starting branch), and CloneStep's prefilled
 * settings phase. The worktree-parent field is checked read-only against
 * `validate_worktree_parent` on every change; a blocking verdict is reported
 * up so the step's Create action can be disabled in-dialog rather than
 * failing later at first task execution.
 */

import { useEffect } from "react";
import { Label } from "@/components/ui/label";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { GitBranch, ChevronDown, Settings, FolderOpen } from "lucide-react";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import { cn } from "@/lib/utils";
import { useWorktreeParentVerdict } from "./useWorktreeParentVerdict";
import { describeWorktreeParentVerdict, isWorktreeParentVerdictBlocking } from "@/types/worktree-parent";

export interface GitSettingsSectionProps {
  /** "select" for a repo's real branch list; "freeText" when the repo doesn't exist yet. */
  baseBranchMode: "select" | "freeText";
  baseBranchLabel: string;
  baseBranch: string;
  onBaseBranchChange: (value: string) => void;
  branches?: string[] | undefined;
  loadingBranches?: boolean | undefined;
  baseBranchError?: string | undefined;
  baseBranchTouched?: boolean | undefined;
  worktreePath: string;
  worktreeParentDirectory: string;
  onWorktreeParentDirectoryChange: (value: string) => void;
  /** The repository root the worktree parent must stay outside of, when known. */
  worktreeParentRepositoryRoot?: string | undefined;
  /** Opens a folder picker for the worktree parent; omitted hides the Browse button. */
  onBrowseWorktreeParent?: (() => void) | undefined;
  /** Reports whether the current worktree-parent verdict should block Create. */
  onWorktreeParentBlockingChange?: ((blocking: boolean) => void) | undefined;
  showAdvanced: boolean;
  onShowAdvancedChange: (open: boolean) => void;
  isCreating: boolean;
}

export function GitSettingsSection({
  baseBranchMode,
  baseBranchLabel,
  baseBranch,
  onBaseBranchChange,
  branches = [],
  loadingBranches = false,
  baseBranchError,
  baseBranchTouched,
  worktreePath,
  worktreeParentDirectory,
  onWorktreeParentDirectoryChange,
  worktreeParentRepositoryRoot,
  onBrowseWorktreeParent,
  onWorktreeParentBlockingChange,
  showAdvanced,
  onShowAdvancedChange,
  isCreating,
}: GitSettingsSectionProps) {
  const showBaseBranchError = Boolean(baseBranchTouched && baseBranchError);
  const worktreeParentVerdict = useWorktreeParentVerdict(
    worktreeParentDirectory,
    worktreeParentRepositoryRoot
  );
  const worktreeParentBlocking = isWorktreeParentVerdictBlocking(worktreeParentVerdict);

  useEffect(() => {
    onWorktreeParentBlockingChange?.(worktreeParentBlocking);
  }, [worktreeParentBlocking, onWorktreeParentBlockingChange]);

  return (
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
            {baseBranchLabel}
          </Label>
          {baseBranchMode === "select" ? (
            <Select
              value={baseBranch}
              onValueChange={onBaseBranchChange}
              disabled={isCreating || loadingBranches}
            >
              <SelectTrigger
                data-testid="base-branch-select"
                className={cn(
                  "h-10 px-3 py-2 rounded-lg text-sm bg-[var(--bg-base)] border text-[var(--text-primary)] focus:ring-2 focus:ring-[var(--accent-primary)] focus:border-[var(--accent-primary)]",
                  showBaseBranchError
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
          ) : (
            <Input
              id="base-branch-select"
              data-testid="base-branch-select"
              type="text"
              value={baseBranch}
              onChange={(e) => onBaseBranchChange(e.target.value)}
              placeholder="main"
              disabled={isCreating}
              className={cn(
                "h-10 px-3 py-2 rounded-lg text-sm bg-[var(--bg-base)] border text-[var(--text-primary)] placeholder:text-[var(--text-muted)] focus:ring-2 focus:ring-[var(--accent-primary)] focus:border-[var(--accent-primary)]",
                showBaseBranchError
                  ? "border-[var(--status-error)]"
                  : "border-[var(--border-subtle)]",
                isCreating && "opacity-50"
              )}
            />
          )}
          {showBaseBranchError && (
            <p
              data-testid="base-branch-select-error"
              className="text-xs text-[var(--status-error)]"
            >
              {baseBranchError}
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
        <Collapsible open={showAdvanced} onOpenChange={onShowAdvancedChange}>
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
              <div className="flex gap-2">
                <Input
                  id="worktree-parent-input"
                  data-testid="worktree-parent-input"
                  type="text"
                  value={worktreeParentDirectory}
                  onChange={(e) => onWorktreeParentDirectoryChange(e.target.value)}
                  placeholder="~/ralphx-worktrees"
                  disabled={isCreating}
                  className={cn(
                    "flex-1 h-10 px-3 py-2 rounded-lg text-sm bg-[var(--bg-base)] border text-[var(--text-primary)] placeholder:text-[var(--text-muted)] focus:ring-2 focus:ring-[var(--accent-primary)] focus:border-[var(--accent-primary)]",
                    worktreeParentBlocking ? "border-[var(--status-error)]" : "border-[var(--border-subtle)]",
                    isCreating && "opacity-50"
                  )}
                />
                {onBrowseWorktreeParent && (
                  <Button
                    data-testid="worktree-parent-browse-button"
                    type="button"
                    onClick={onBrowseWorktreeParent}
                    disabled={isCreating}
                    variant="secondary"
                    className="h-10 px-3 gap-2 bg-[var(--bg-elevated)] text-[var(--text-primary)] hover:bg-[var(--bg-hover)] border-0"
                  >
                    <FolderOpen className="h-4 w-4" />
                    Browse
                  </Button>
                )}
              </div>
              {worktreeParentVerdict ? (
                (() => {
                  const { tone, message } = describeWorktreeParentVerdict(worktreeParentVerdict);
                  return (
                    <p
                      data-testid="worktree-parent-verdict"
                      className={cn(
                        "text-xs",
                        tone === "error"
                          ? "text-[var(--status-error)]"
                          : tone === "warning"
                            ? "text-[var(--status-warning)]"
                            : "text-[var(--text-muted)]"
                      )}
                    >
                      {message}
                    </p>
                  );
                })()
              ) : (
                <p className="text-xs text-[var(--text-muted)]">
                  Default: ~/ralphx-worktrees. Task worktrees will be created inside this directory.
                </p>
              )}
            </div>
          </CollapsibleContent>
        </Collapsible>
      </div>
    </div>
  );
}
