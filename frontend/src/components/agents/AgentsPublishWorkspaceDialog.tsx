import { GitPullRequestArrow } from "lucide-react";

import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";

import { PublishPipelineSteps } from "./AgentsPublishPipelineSteps";
import type { AgentWorkspacePrAutofixFingerprintSpendPresentation } from "./agentWorkspacePublishState";

export type PublishWorkspaceDialogPhase = "confirm" | "publishing";

export function PublishWorkspaceDialog({
  autoMergeCurrent = null,
  autoMergeDesired = false,
  base,
  branch,
  confirmDisabled,
  fingerprintSpend = null,
  isPublishing,
  onConfirm,
  onOpenChange,
  open,
  phase,
  targetPullRequestLabel = null,
  prSupervisionStatus = null,
  status,
}: {
  autoMergeCurrent?: boolean | null;
  autoMergeDesired?: boolean;
  base: string;
  branch: string;
  confirmDisabled: boolean;
  fingerprintSpend?: AgentWorkspacePrAutofixFingerprintSpendPresentation | null;
  isPublishing: boolean;
  onConfirm: () => void;
  onOpenChange: (open: boolean) => void;
  open: boolean;
  phase: PublishWorkspaceDialogPhase;
  targetPullRequestLabel?: string | null;
  prSupervisionStatus?: string | null;
  status: string | null;
}) {
  const isProgress = phase === "publishing";
  const publishDescription = targetPullRequestLabel
    ? `This will commit workspace changes on ${branch} and push updates to ${targetPullRequestLabel}.`
    : `This will commit workspace changes on ${branch} and push them to a pull request against ${base}.`;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        className="w-[min(520px,calc(100vw-2rem))] overflow-hidden p-0"
        data-testid="agents-publish-workspace-dialog"
        style={{
          backgroundColor: "var(--bg-surface)",
          borderColor: "var(--border-subtle)",
          borderStyle: "solid",
          borderWidth: "1px",
        }}
      >
        <DialogHeader
          className={
            isProgress
              ? "block border-b-0 px-5 pb-0 pt-5"
              : "block border-b-0 px-5 pb-3 pt-5"
          }
        >
          <DialogTitle className="pr-8 text-base leading-6 tracking-normal">
            {isProgress ? "Publishing workspace" : "Commit and publish workspace?"}
          </DialogTitle>
          {isProgress ? (
            <DialogDescription className="sr-only">
              Workspace publishing is in progress.
            </DialogDescription>
          ) : (
            <DialogDescription className="mt-1.5 max-w-[28rem] text-sm leading-5 text-[var(--text-secondary)]">
              {publishDescription}
            </DialogDescription>
          )}
        </DialogHeader>

        {fingerprintSpend && (
          <div
            className="mx-5 rounded-md px-3 py-2 text-xs"
            data-testid="agents-publish-fingerprint-spend"
            style={{
              backgroundColor: "var(--bg-subtle)",
              borderColor: fingerprintSpend.exhausted
                ? "var(--status-error-border)"
                : "var(--border-subtle)",
              borderStyle: "solid",
              borderWidth: "1px",
              color: "var(--text-secondary)",
            }}
          >
            <div>{fingerprintSpend.summary}</div>
            {fingerprintSpend.exhausted && (
              <div className="mt-0.5 font-medium text-[var(--status-error)]">
                Repair budget exhausted
              </div>
            )}
          </div>
        )}

        {isProgress && (
          <div className="px-5 pb-4">
            <PublishPipelineSteps
              autoMergeCurrent={autoMergeCurrent}
              autoMergeDesired={autoMergeDesired}
              className="mt-3"
              prSupervisionStatus={prSupervisionStatus}
              targetPullRequestLabel={targetPullRequestLabel}
              status={status}
              isPublishing={isPublishing}
              testIdPrefix="agents-publish-dialog"
            />
          </div>
        )}

        <DialogFooter className="gap-2 border-t-0 px-5 pb-5 pt-0 sm:gap-2">
          {isProgress ? (
            <Button
              type="button"
              variant="secondary"
              data-testid="agents-publish-dialog-close"
              onClick={() => onOpenChange(false)}
            >
              Close
            </Button>
          ) : (
            <>
              <Button
                type="button"
                variant="ghost"
                onClick={() => onOpenChange(false)}
              >
                Cancel
              </Button>
              <Button
                type="button"
                className="gap-2"
                onClick={onConfirm}
                disabled={confirmDisabled}
              >
                <GitPullRequestArrow className="h-3.5 w-3.5" />
                Commit & Publish
              </Button>
            </>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
