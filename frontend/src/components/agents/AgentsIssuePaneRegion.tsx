import { lazy, memo, Suspense, useEffect, type ElementType } from "react";
import { Ticket, X } from "lucide-react";

import type { AgentConversationWorkspace } from "@/api/chat";
import { Button } from "@/components/ui/button";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";
import { withAlpha } from "@/lib/theme-colors";

import type { AgentConversation } from "./agentConversations";
import { EmptyArtifactState } from "./AgentsArtifactEmptyState";
import { type AgentIssueTab, useAgentIssueTabs } from "./agentIssueTabs";

const LazyAgentsJiraIssuePanel = lazy(() =>
  import("@/components/agents/AgentsJiraIssuePanel").then((module) => ({
    default: module.AgentsJiraIssuePanel,
  })),
);
const LazyAgentsLinearIssuePanel = lazy(() =>
  import("@/components/agents/AgentsLinearIssuePanel").then((module) => ({
    default: module.AgentsLinearIssuePanel,
  })),
);

const ISSUE_TABS: Array<{
  id: AgentIssueTab;
  label: string;
  icon: ElementType;
}> = [
  { id: "jira", label: "Jira", icon: Ticket },
  { id: "linear", label: "Linear", icon: Ticket },
];

interface AgentsIssuePaneRegionProps {
  conversation: AgentConversation | null;
  workspace: AgentConversationWorkspace | null;
  activeTab: AgentIssueTab;
  isOpen: boolean;
  onTabChange: (tab: AgentIssueTab) => void;
  onClose: () => void;
}

export const AgentsIssuePaneRegion = memo(function AgentsIssuePaneRegion({
  conversation,
  workspace,
  activeTab,
  isOpen,
  onTabChange,
  onClose,
}: AgentsIssuePaneRegionProps) {
  const issueTabs = useAgentIssueTabs(
    Boolean(conversation && conversation.contextType !== "ideation"),
  );
  const effectiveActiveTab =
    issueTabs.includes(activeTab) ? activeTab : issueTabs[0] ?? "linear";

  useEffect(() => {
    if (isOpen && issueTabs.length > 0 && effectiveActiveTab !== activeTab) {
      onTabChange(effectiveActiveTab);
    }
  }, [activeTab, effectiveActiveTab, isOpen, issueTabs.length, onTabChange]);

  if (!isOpen) {
    return null;
  }

  return (
    <div
      className="absolute inset-y-0 right-0 z-30 flex max-w-full flex-col overflow-hidden border-l shadow-2xl"
      style={{
        width: "min(680px, 100%)",
        background: "var(--bg-surface)",
        borderColor: "var(--overlay-faint)",
      }}
      data-testid="agents-issue-pane"
    >
      <div
        className="h-11 px-4 flex items-center gap-0 border-b shrink-0"
        style={{
          background: withAlpha("var(--bg-surface)", 72),
          backdropFilter: "blur(12px)",
          WebkitBackdropFilter: "blur(12px)",
          borderColor: "var(--overlay-faint)",
        }}
      >
        <div className="flex h-full items-stretch gap-0 min-w-0 self-stretch">
          {ISSUE_TABS.filter((tab) => issueTabs.includes(tab.id)).map(
            ({ id, label, icon: Icon }) => {
              const isActive = effectiveActiveTab === id;
              return (
                <button
                  key={id}
                  type="button"
                  onClick={() => onTabChange(id)}
                  className={cn(
                    "relative flex h-full self-stretch items-center gap-1.5 bg-transparent px-3 text-[0.75rem] font-medium transition-colors duration-150 rounded-none shadow-none outline-none ring-0 focus:ring-0 focus:outline-none focus-visible:outline-none focus-visible:ring-0 appearance-none",
                  )}
                  style={{
                    color: isActive ? "var(--text-primary)" : "var(--text-muted)",
                    background: "transparent",
                    boxShadow: "none",
                  }}
                  data-testid={`agents-issue-tab-${id}`}
                  data-theme-button-skip="true"
                >
                  <Icon className="w-4 h-4 shrink-0" />
                  <span>{label}</span>
                  {isActive && (
                    <span
                      className="absolute -bottom-px left-3 right-3 h-[2px] rounded-full"
                      style={{ background: "var(--accent-primary)" }}
                    />
                  )}
                </button>
              );
            },
          )}
        </div>

        <div className="ml-auto flex items-center gap-1">
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                type="button"
                variant="ghost"
                size="sm"
                onClick={onClose}
                className="h-8 w-8 p-0"
                aria-label="Close linked issues"
                data-testid="agents-issue-pane-close"
              >
                <X className="w-4 h-4" />
              </Button>
            </TooltipTrigger>
            <TooltipContent side="bottom" className="text-xs">
              Close linked issues
            </TooltipContent>
          </Tooltip>
        </div>
      </div>

      <div
        className="flex-1 min-h-0 overflow-y-auto"
        data-testid={`agents-issue-content-${effectiveActiveTab}`}
      >
        <IssueContent
          activeTab={effectiveActiveTab}
          conversationId={conversation?.id ?? null}
          projectId={conversation?.projectId ?? workspace?.projectId ?? null}
        />
      </div>
    </div>
  );
});

function IssueContent({
  activeTab,
  conversationId,
  projectId,
}: {
  activeTab: AgentIssueTab;
  conversationId: string | null;
  projectId: string | null;
}) {
  if (activeTab === "jira") {
    return (
      <Suspense fallback={<EmptyArtifactState title="Loading Jira..." />}>
        <LazyAgentsJiraIssuePanel
          conversationId={conversationId}
          projectId={projectId}
        />
      </Suspense>
    );
  }

  return (
    <Suspense fallback={<EmptyArtifactState title="Loading Linear..." />}>
      <LazyAgentsLinearIssuePanel
        conversationId={conversationId}
        projectId={projectId}
      />
    </Suspense>
  );
}
