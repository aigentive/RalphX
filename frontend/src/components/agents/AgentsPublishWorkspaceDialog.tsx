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

export type PublishWorkspaceDialogPhase = "confirm" | "publishing";

export function PublishWorkspaceDialog({
  base,
  branch,
  confirmDisabled,
  isPublishing,
  onConfirm,
  onOpenChange,
  open,
  phase,
  status,
}: {
  base: string;
  branch: string;
  confirmDisabled: boolean;
  isPublishing: boolean;
  onConfirm: () => void;
  onOpenChange: (open: boolean) => void;
  open: boolean;
  phase: PublishWorkspaceDialogPhase;
  status: string | null;
}) {
  const isProgress = phase === "publishing";

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        className="w-[min(460px,calc(100vw-2rem))] p-4"
        data-testid="agents-publish-workspace-dialog"
        style={{
          backgroundColor: "var(--bg-surface)",
          border: "1px solid var(--border-subtle)",
        }}
      >
        <DialogHeader className="block space-y-1.5">
          <DialogTitle>
            {isProgress ? "Publishing workspace" : "Commit and publish workspace?"}
          </DialogTitle>
          <DialogDescription>
            {isProgress
              ? "Progress is also available in Commit & Publish."
              : `This will commit workspace changes on ${branch} and push them to a pull request against ${base}.`}
          </DialogDescription>
        </DialogHeader>

        {isProgress && (
          <PublishPipelineSteps
            status={status}
            isPublishing={isPublishing}
            testIdPrefix="agents-publish-dialog"
          />
        )}

        <DialogFooter className="gap-2 sm:gap-2">
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
