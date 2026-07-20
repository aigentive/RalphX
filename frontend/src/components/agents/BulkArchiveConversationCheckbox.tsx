import { useMemo } from "react";

import type { AgentConversationWorkspace } from "@/api/chat";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";

import type { AgentConversation } from "./agentConversations";
import { isBulkArchiveConversationEligible } from "./bulkConversationArchive";
import { useBulkArchiveSelection } from "./bulkConversationArchiveSelectionContext";

export function BulkArchiveConversationCheckbox({
  conversation,
  workspace,
}: {
  conversation: AgentConversation;
  workspace: AgentConversationWorkspace | null;
}) {
  const bulkArchiveSelection = useBulkArchiveSelection();
  const target = useMemo(
    () => ({ conversation, workspace }),
    [conversation, workspace]
  );
  if (!bulkArchiveSelection.active) {
    return null;
  }

  const title = conversation.title || "Untitled agent";
  const eligible = isBulkArchiveConversationEligible(target);
  const blockedReason = eligible ? null : "This session is already archived";
  const descriptionId = `agents-bulk-archive-description-${conversation.id}`;
  const checkbox = (
    <Checkbox
      aria-label={`Select ${title} for bulk archive`}
      {...(blockedReason ? { "aria-describedby": descriptionId } : {})}
      checked={bulkArchiveSelection.selectedIds.has(conversation.id)}
      disabled={Boolean(blockedReason) || bulkArchiveSelection.pending}
      onCheckedChange={() => bulkArchiveSelection.toggleTarget(target)}
    />
  );

  return (
    <div
      className="absolute left-2 top-1/2 z-10 -translate-y-1/2"
      onClick={(event) => event.stopPropagation()}
    >
      {blockedReason ? (
        <Tooltip>
          <TooltipTrigger asChild>
            <span className="inline-flex">{checkbox}</span>
          </TooltipTrigger>
          <TooltipContent side="right" className="text-xs">
            {blockedReason}
          </TooltipContent>
        </Tooltip>
      ) : (
        checkbox
      )}
      {blockedReason && (
        <span id={descriptionId} className="sr-only">
          {blockedReason}
        </span>
      )}
    </div>
  );
}
