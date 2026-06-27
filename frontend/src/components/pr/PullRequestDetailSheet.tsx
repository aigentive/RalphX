import { X } from "lucide-react";

import type { TicketAssociations } from "@/api/ticketing";
import type { PullRequestDetailSelector } from "@/hooks/usePullRequestDetail";
import { RalphxAssociationPanel } from "@/components/associations/RalphxAssociationPanel";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";

import {
  PullRequestDetailBody,
} from "./PullRequestDetailBody";
import type { PullRequestShell } from "./PullRequestDetailShell";

const EMPTY_ASSOCIATIONS: TicketAssociations = {
  tasks: [],
  proposals: [],
  sessions: [],
  conversations: [],
  pullRequests: [],
  checks: [],
  qa: [],
  specs: [],
  fetchedAt: null,
};

export function PullRequestDetailSheet({
  open,
  selector,
  shell,
  onClose,
}: {
  open: boolean;
  selector: PullRequestDetailSelector | null;
  shell: PullRequestShell | null;
  onClose: () => void;
}) {
  return (
    <Dialog
      open={open}
      onOpenChange={(nextOpen) => {
        if (!nextOpen) {
          onClose();
        }
      }}
    >
      <DialogContent
        hideCloseButton
        className="left-auto right-0 top-12 h-[calc(100vh-3rem)] w-[64vw] min-w-[820px] max-w-[1180px] translate-x-0 translate-y-0 rounded-none p-0"
        style={{
          backgroundColor: "var(--bg-elevated)",
          borderColor: "var(--border-subtle)",
          borderStyle: "solid",
          borderWidth: "1px",
          boxShadow: "var(--shadow-lg)",
        }}
      >
        <div className="grid h-full min-h-0 grid-cols-[minmax(0,1fr)_320px]">
          <div className="flex min-h-0 flex-col">
            <DialogHeader className="shrink-0 px-5 py-4">
              <div className="min-w-0">
                <DialogTitle className="truncate text-base">
                  {shell?.prNumber ? `PR #${shell.prNumber}` : "Pull request"}
                </DialogTitle>
                <DialogDescription className="mt-1 truncate">
                  {shell?.title ?? shell?.branch ?? "GitHub pull request"}
                </DialogDescription>
              </div>
              <Button type="button" variant="ghost" size="sm" onClick={onClose}>
                <X className="h-4 w-4" aria-hidden="true" />
                Close
              </Button>
            </DialogHeader>
            <div className="min-h-0 flex-1 overflow-auto">
              <PullRequestDetailBody selector={selector} shell={shell} />
            </div>
          </div>
          <RalphxAssociationPanel
            ticket={null}
            associations={EMPTY_ASSOCIATIONS}
            isLoading={false}
            showStartWork={false}
            showConversationBinding={false}
          />
        </div>
      </DialogContent>
    </Dialog>
  );
}
