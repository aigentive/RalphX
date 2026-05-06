import { memo } from "react";
import { AlertTriangle } from "lucide-react";

import type {
  AgentConversationWorkspace,
  AgentConversationWorkspaceFreshness,
} from "@/api/chat";
import { BranchBasePicker } from "@/components/shared/BranchBasePicker";
import type { BranchBaseOption } from "@/components/shared/branchBaseOptions";

export const AgentConversationBaseLine = memo(function AgentConversationBaseLine({
  freshness,
  workspace,
}: {
  freshness?: AgentConversationWorkspaceFreshness;
  workspace: AgentConversationWorkspace | null;
}) {
  if (!workspace) {
    return null;
  }

  if (freshness?.baseStatus === "blocked") {
    return (
      <div
        className="flex min-w-0 justify-end"
        data-testid="agents-conversation-base"
      >
        <div
          className="inline-flex min-w-0 items-center gap-1.5 rounded-md border px-2.5 py-1 text-xs"
          style={{
            borderColor: "var(--status-warning-border)",
            color: "var(--status-warning)",
            background: "var(--bg-surface)",
          }}
        >
          <AlertTriangle className="h-3.5 w-3.5 shrink-0" aria-hidden="true" />
          <span className="truncate">Base unavailable</span>
        </div>
      </div>
    );
  }

  const baseLabel =
    freshness?.effectiveBaseDisplayName ??
    freshness?.baseDisplayName ??
    workspace.baseDisplayName ??
    workspace.baseRef;
  const baseRef = freshness?.effectiveBaseRef ?? freshness?.baseRef ?? workspace.baseRef;
  const baseKind = freshness?.baseStatus === "retargeted"
    ? "project_default"
    : workspace.baseRefKind;
  const option: BranchBaseOption = {
    key: `${baseKind}:${baseRef}`,
    label: baseLabel,
    detail: baseLabel !== baseRef ? baseRef : undefined,
    source: "local",
    selection: {
      kind:
        baseKind === "project_default" ||
        baseKind === "current_branch" ||
        baseKind === "local_branch"
          ? baseKind
          : "local_branch",
      ref: baseRef,
      displayName: baseLabel,
    },
  };

  return (
    <div
      className="flex min-w-0 justify-end"
      data-testid="agents-conversation-base"
    >
      <BranchBasePicker
        value={option.key}
        onValueChange={() => undefined}
        options={[option]}
        placeholder="Base branch"
        readOnly
      />
    </div>
  );
});
