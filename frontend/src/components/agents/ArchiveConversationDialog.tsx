import { useEffect, useState } from "react";

import type { AgentConversationWorkspace } from "@/api/chat";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Checkbox } from "@/components/ui/checkbox";

import type {
  AgentConversation,
  AgentConversationArchiveOptions,
} from "./agentConversations";
import { hasPotentialOpenPullRequest } from "./bulkConversationArchive";

export interface ArchiveConversationDialogTarget {
  conversation: AgentConversation;
  workspace: AgentConversationWorkspace | null;
}

interface ArchiveConversationDialogProps {
  target: ArchiveConversationDialogTarget | null;
  onArchive: (
    conversation: AgentConversation,
    options: AgentConversationArchiveOptions
  ) => void;
  onClose: () => void;
}

export function ArchiveConversationDialog({
  target,
  onArchive,
  onClose,
}: ArchiveConversationDialogProps) {
  const [closePullRequest, setClosePullRequest] = useState(false);
  const isReviewPr = target?.workspace?.mode === "review_pr";
  const canClosePullRequest =
    !isReviewPr && hasPotentialOpenPullRequest(target?.workspace ?? null);

  useEffect(() => {
    setClosePullRequest(false);
  }, [target?.conversation.id]);

  return (
    <AlertDialog
      open={target !== null}
      onOpenChange={(open) => {
        if (!open) {
          onClose();
        }
      }}
    >
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>Archive session?</AlertDialogTitle>
          <AlertDialogDescription asChild>
            <div>
              <p>
                This hides{" "}
                <span className="font-medium">
                  {target?.conversation.title || "Untitled agent"}
                </span>{" "}
                from the active conversation list. You can restore it later from archived
                sessions.
              </p>
              {target?.workspace && (
                <p className="mt-2 text-text-primary">
                  Archiving permanently deletes the local RalphX workspace and local branch,
                  including uncommitted changes and ignored build or test artifacts. These local
                  files cannot be restored.
                </p>
              )}
              {canClosePullRequest && (
                <>
                  <p className="mt-2 text-text-muted">
                    Archiving leaves this pull request open unless you choose to close it.
                  </p>
                  <label className="mt-3 flex cursor-pointer items-center gap-2 text-text-primary">
                    <Checkbox
                      aria-label="Close pull request"
                      checked={closePullRequest}
                      onCheckedChange={(checked) => setClosePullRequest(checked === true)}
                    />
                    <span>Close pull request</span>
                  </label>
                </>
              )}
              {isReviewPr && (
                <p className="mt-2 text-text-muted">
                  The reviewed pull request will remain open.
                </p>
              )}
            </div>
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>Cancel</AlertDialogCancel>
          <AlertDialogAction
            onClick={() => {
              if (target) {
                onArchive(target.conversation, { closePullRequest });
              }
              onClose();
            }}
            variant="destructive"
          >
            Archive session
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
