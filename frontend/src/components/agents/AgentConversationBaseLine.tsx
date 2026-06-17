import { memo } from "react";
import { AlertTriangle } from "lucide-react";

import type {
  AgentConversationWorkspace,
  AgentConversationWorkspaceFreshness,
} from "@/api/chat";
import { BranchBasePicker } from "@/components/shared/BranchBasePicker";
import type { BranchBaseOption } from "@/components/shared/branchBaseOptions";

function mergeBranchBaseOptions(
  currentOption: BranchBaseOption,
  options: BranchBaseOption[] | undefined,
): BranchBaseOption[] {
  const mergedOptions = new Map<string, BranchBaseOption>();
  mergedOptions.set(currentOption.key, currentOption);
  for (const candidate of options ?? []) {
    mergedOptions.set(candidate.key, candidate);
  }
  return Array.from(mergedOptions.values());
}

export const AgentConversationBaseLine = memo(function AgentConversationBaseLine({
  disabled = false,
  editable = false,
  freshness,
  isLoading = false,
  isLoadingPullRequests = false,
  onIntent,
  onOpenChange,
  onPullRequestSearch,
  onValueChange,
  options,
  pullRequestMessage = null,
  pullRequestOptions,
  value,
  workspace,
}: {
  disabled?: boolean;
  editable?: boolean;
  freshness?: AgentConversationWorkspaceFreshness;
  isLoading?: boolean;
  isLoadingPullRequests?: boolean;
  onIntent?: () => void;
  onOpenChange?: (open: boolean) => void;
  onPullRequestSearch?: (query: string) => void;
  onValueChange?: (value: string) => void;
  options?: BranchBaseOption[];
  pullRequestMessage?: string | null;
  pullRequestOptions?: BranchBaseOption[];
  value?: string;
  workspace: AgentConversationWorkspace | null;
}) {
  if (!workspace) {
    return null;
  }

  if (freshness?.baseStatus === "blocked" && !editable) {
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
  const pickerOptions = mergeBranchBaseOptions(option, options);
  const resolvedValue =
    value && pickerOptions.some((candidate) => candidate.key === value)
      ? value
      : option.key;

  return (
    <div
      className="flex min-w-0 justify-end"
      data-testid="agents-conversation-base"
    >
      <BranchBasePicker
        value={resolvedValue}
        onValueChange={onValueChange ?? (() => undefined)}
        options={pickerOptions}
        placeholder="Base branch"
        readOnly={!editable}
        disabled={disabled}
        isLoading={isLoading}
        enablePullRequests={editable}
        pullRequestOptions={pullRequestOptions ?? []}
        isLoadingPullRequests={isLoadingPullRequests}
        pullRequestMessage={pullRequestMessage}
        {...(onPullRequestSearch ? { onPullRequestSearch } : {})}
        testId="agents-conversation-base-picker"
        ariaLabel={editable ? "Change workspace base branch" : "Workspace base branch"}
        {...(onIntent ? { onIntent } : {})}
        {...(onOpenChange ? { onOpenChange } : {})}
      />
    </div>
  );
});
