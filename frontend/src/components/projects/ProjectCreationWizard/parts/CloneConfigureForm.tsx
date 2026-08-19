/**
 * CloneConfigureForm - the "configure" phase of CloneStep (URL + destination
 * + optional GitHub picker + advanced options + failure/error surfaces +
 * footer). Extracted so CloneStep.tsx stays under the file-size cap; all
 * state and submission logic remains owned by CloneStep.
 */

import { DialogFooter } from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { cn } from "@/lib/utils";
import type { CloneTargetPlan } from "@/api/projects";
import type { CloneJobStatus } from "@/types/clone";
import { DestinationPicker } from "./DestinationPicker";
import { CloneAuthFailureCard } from "./CloneAuthFailureCard";
import { GitHubRepoPicker } from "./GitHubRepoPicker";
import { CloneAdvancedOptions } from "./CloneAdvancedOptions";
import { ErrorBanner } from "./CloneErrorBanner";
import { failureMessage } from "./cloneFailureMessage";

export interface CloneConfigureFormProps {
  url: string;
  onUrlChange: (value: string) => void;
  onSelectRepo: (nameWithOwner: string) => void;
  submitted: boolean;
  plan: CloneTargetPlan | null;
  parentDirectory: string;
  onParentDirectoryChange: (value: string) => void;
  folderName: string;
  onFolderNameChange: (value: string) => void;
  onBrowseParent?: (() => void) | undefined;
  depth: string;
  onDepthChange: (value: string) => void;
  singleBranch: boolean;
  onSingleBranchChange: (value: boolean) => void;
  recurseSubmodules: boolean;
  onRecurseSubmodulesChange: (value: boolean) => void;
  showAdvancedCloneOptions: boolean;
  onShowAdvancedCloneOptionsChange: (open: boolean) => void;
  jobStatus: CloneJobStatus | null;
  jobError: string | null;
  onUseSshUrl: () => void;
  onRetry: () => void;
  isCreating: boolean;
  isFirstRun: boolean;
  onClose: () => void;
  onStart: () => void;
  primaryDisabled: boolean;
  error?: string | null | undefined;
}

export function CloneConfigureForm({
  url,
  onUrlChange,
  onSelectRepo,
  submitted,
  plan,
  parentDirectory,
  onParentDirectoryChange,
  folderName,
  onFolderNameChange,
  onBrowseParent,
  depth,
  onDepthChange,
  singleBranch,
  onSingleBranchChange,
  recurseSubmodules,
  onRecurseSubmodulesChange,
  showAdvancedCloneOptions,
  onShowAdvancedCloneOptionsChange,
  jobStatus,
  jobError,
  onUseSshUrl,
  onRetry,
  isCreating,
  isFirstRun,
  onClose,
  onStart,
  primaryDisabled,
  error = null,
}: CloneConfigureFormProps) {
  const isAuthFailure = jobStatus?.state === "failed" && jobStatus.code === "CLONE_AUTH_FAILED";
  const otherFailure = jobStatus?.state === "failed" && jobStatus.code !== "CLONE_AUTH_FAILED";

  return (
    <>
      <div className="px-6 py-5 space-y-5">
        <div className="space-y-1.5">
          <Label htmlFor="clone-url-input" className="text-sm font-medium text-[var(--text-secondary)]">
            Repository URL <span className="text-[var(--status-error)]">*</span>
          </Label>
          <Input
            id="clone-url-input"
            data-testid="clone-url-input"
            type="text"
            value={url}
            onChange={(e) => onUrlChange(e.target.value)}
            placeholder="https://github.com/owner/repo or git@github.com:owner/repo.git"
            disabled={isCreating}
            className={cn(
              "h-10 px-3 py-2 rounded-lg text-sm bg-[var(--bg-base)] border text-[var(--text-primary)] placeholder:text-[var(--text-muted)] focus:ring-2 focus:ring-[var(--accent-primary)] focus:border-[var(--accent-primary)]",
              submitted && plan?.problem ? "border-[var(--status-error)]" : "border-[var(--border-subtle)]",
              isCreating && "opacity-50"
            )}
          />
          <GitHubRepoPicker onSelectRepo={onSelectRepo} disabled={isCreating} />
        </div>

        <DestinationPicker
          idPrefix="clone"
          parentDirectory={parentDirectory}
          onParentDirectoryChange={onParentDirectoryChange}
          folderName={folderName}
          onFolderNameChange={onFolderNameChange}
          onBrowseParent={onBrowseParent}
          isCreating={isCreating}
          parentError={submitted && !parentDirectory.trim() ? "Parent folder is required" : undefined}
        />

        <CloneAdvancedOptions
          depth={depth}
          onDepthChange={onDepthChange}
          singleBranch={singleBranch}
          onSingleBranchChange={onSingleBranchChange}
          recurseSubmodules={recurseSubmodules}
          onRecurseSubmodulesChange={onRecurseSubmodulesChange}
          isCreating={isCreating}
          open={showAdvancedCloneOptions}
          onOpenChange={onShowAdvancedCloneOptionsChange}
        />

        {isAuthFailure && (
          <CloneAuthFailureCard
            suggestedSshUrl={plan?.suggestedSshUrl ?? null}
            onUseSshUrl={onUseSshUrl}
            onRetry={onRetry}
            canRetry={Boolean(plan?.ready)}
          />
        )}

        {otherFailure && jobStatus?.state === "failed" && (
          <ErrorBanner text={failureMessage(jobStatus.code)} />
        )}

        {jobStatus?.state === "cancelled" && (
          <p className="text-xs text-[var(--text-muted)]">Clone cancelled.</p>
        )}

        {jobStatus?.state === "unknown" && jobError && <ErrorBanner text={jobError} />}

        {!isAuthFailure && submitted && plan?.problem && <ErrorBanner text={plan.problem} />}

        {error && <ErrorBanner testId="wizard-error" text={error} />}
      </div>

      <DialogFooter className="px-6 py-4 border-t border-[var(--border-subtle)] gap-3 sm:gap-3">
        {!isFirstRun && !isCreating && (
          <span className="mr-auto text-xs text-[var(--text-muted)]">
            Press <kbd className="px-1.5 py-0.5 rounded bg-[var(--bg-base)] border border-[var(--border-subtle)] font-mono text-[0.625rem]">ESC</kbd> to cancel
          </span>
        )}
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
          data-testid="clone-start-button"
          type="button"
          onClick={onStart}
          disabled={primaryDisabled}
          className={cn(
            "gap-2",
            primaryDisabled
              ? "bg-[var(--bg-hover)] text-[var(--text-muted)] cursor-not-allowed"
              : "bg-[var(--accent-primary)] text-white hover:bg-[var(--accent-primary)]/90"
          )}
        >
          Clone Repository
        </Button>
      </DialogFooter>
    </>
  );
}
