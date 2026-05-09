import { memo, useCallback, useEffect, useMemo, useState, type ElementType } from "react";
import { useQuery } from "@tanstack/react-query";
import {
  CheckCircle2,
  ChevronDown,
  ClipboardList,
  FileText,
  GitBranch,
  GitPullRequestArrow,
  Lightbulb,
  Loader2,
  Menu,
  MessageSquare,
  PanelRightClose,
  PanelRightOpen,
  ShieldCheck,
  Terminal as TerminalIcon,
} from "lucide-react";

import type { AgentConversationWorkspace } from "@/api/chat";
import * as chatApi from "@/api/chat";
import { ChatSessionChips } from "@/components/Chat/ChatSessionChips";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { formatBranchDisplay } from "@/lib/branch-utils";
import { withAlpha } from "@/lib/theme-colors";
import { cn } from "@/lib/utils";
import { useChatStore } from "@/stores/chatStore";
import type { AgentArtifactTab } from "@/stores/agentSessionStore";
import type { ModelDisplay } from "@/types/chat-conversation";

import { useAgentsSidebarVisibility } from "./useAgentsSidebarVisibility";
import {
  getAgentConversationStoreKey,
  type AgentConversation,
} from "./agentConversations";
import {
  type AgentsChatFocus,
  type AgentsChatFocusSwitchOption,
  type AgentsChatFocusTone,
  type AgentsChatFocusType,
} from "./agentChatFocus";
import type { IdeationArtifactTab } from "./agentArtifactTabs";
import { resolveConversationAgentMode } from "./agentConversationMode";
import {
  getAgentWorkspaceEffectiveBaseLabel,
  hasPublishedWorkspacePr,
  shouldShowAgentWorkspacePublishSurface,
} from "./agentWorkspacePublishState";
import {
  AGENT_WORKSPACE_FRESHNESS_STALE_MS,
  agentWorkspaceKeys,
} from "./agentWorkspaceQueries";

const HEADER_ARTIFACT_TABS: Array<{
  id: IdeationArtifactTab;
  label: string;
  icon: ElementType;
}> = [
  { id: "plan", label: "Plan", icon: FileText },
  { id: "verification", label: "Verification", icon: CheckCircle2 },
  { id: "proposal", label: "Proposals", icon: GitPullRequestArrow },
  { id: "tasks", label: "Tasks", icon: ClipboardList },
];

const FOCUS_TONE_STYLES: Record<
  AgentsChatFocusTone,
  { color: string; background: string; border: string }
> = {
  accent: {
    color: "var(--accent-primary)",
    background: "var(--accent-muted)",
    border: "var(--accent-border)",
  },
  warning: {
    color: "var(--status-warning)",
    background: "var(--status-warning-muted)",
    border: "var(--status-warning-border)",
  },
};

const FOCUS_TONE_ICONS: Record<AgentsChatFocusTone, ElementType> = {
  accent: Lightbulb,
  warning: ShieldCheck,
};

export interface AgentsChatHeaderProps {
  conversation: AgentConversation | null;
  workspace: AgentConversationWorkspace | null;
  chatFocus?: AgentsChatFocus | undefined;
  modelDisplay?: ModelDisplay | undefined;
  availableArtifactTabs?: readonly IdeationArtifactTab[] | undefined;
  artifactOpen: boolean;
  activeArtifactTab: AgentArtifactTab;
  terminalOpen?: boolean;
  terminalUnavailableReason?: string | null;
  onRenameConversation: (conversationId: string, title: string) => Promise<void>;
  onPublishWorkspace?: (conversationId: string) => Promise<void>;
  onOpenPublishPane?: () => void;
  onPreloadArtifacts?: () => void;
  publishShortcutLabel?: string;
  isPublishingWorkspace?: boolean;
  onToggleTerminal?: () => void;
  onPreloadTerminal?: () => void;
  onToggleArtifacts: () => void;
  onSelectArtifact: (tab: AgentArtifactTab) => void;
  showTitle?: boolean;
}

export const AgentsChatFocusBar = memo(function AgentsChatFocusBar({
  activeType,
  options,
  onSelectFocus,
  workspace = null,
  surfaceBackground = false,
}: {
  activeType: AgentsChatFocusType;
  options: readonly AgentsChatFocusSwitchOption[];
  onSelectFocus: (type: AgentsChatFocusType) => void;
  workspace?: AgentConversationWorkspace | null;
  surfaceBackground?: boolean;
}) {
  const showFocusSwitcher = options.length > 1;
  const [open, setOpen] = useState(false);

  const activeOption = options.find((o) => o.type === activeType) ?? options[0];
  const activeToneStyle = activeOption?.tone
    ? FOCUS_TONE_STYLES[activeOption.tone]
    : null;
  const ActiveIcon = activeOption
    ? activeOption.type === "workspace"
      ? MessageSquare
      : activeOption.tone
        ? FOCUS_TONE_ICONS[activeOption.tone]
        : null
    : null;

  return (
    <div
      className="flex h-9 shrink-0 items-center gap-3 overflow-hidden px-3"
      data-testid="agents-chat-focus-bar"
      style={surfaceBackground ? { backgroundColor: "var(--bg-base)" } : undefined}
    >
      {showFocusSwitcher && activeOption ? (
        <div className="flex min-w-0 flex-1 items-center gap-2 overflow-hidden">
          <span
            className="shrink-0 text-[0.6875rem] font-medium uppercase tracking-[0.08em]"
            style={{ color: "var(--text-muted)" }}
          >
            Chat
          </span>
          <Popover open={open} onOpenChange={setOpen}>
            <PopoverTrigger asChild>
              <button
                type="button"
                aria-label={`Chat focus: ${activeOption.label}. Click to switch.`}
                data-testid="agents-chat-focus-trigger"
                className="inline-flex h-6 max-w-[200px] shrink-0 items-center gap-1.5 rounded-full border px-2 text-[0.75rem] font-medium transition-colors"
                style={
                  activeToneStyle
                    ? {
                        color: activeToneStyle.color,
                        background: activeToneStyle.background,
                        borderColor: activeToneStyle.border,
                      }
                    : {
                        color: "var(--text-primary)",
                        background: "var(--bg-surface)",
                        borderColor: "var(--overlay-moderate)",
                      }
                }
              >
                {ActiveIcon ? <ActiveIcon className="h-3.5 w-3.5 shrink-0" /> : null}
                <span className="truncate">{activeOption.label}</span>
                <ChevronDown className="h-3 w-3 shrink-0 opacity-60" />
              </button>
            </PopoverTrigger>
            <PopoverContent
              align="start"
              sideOffset={4}
              className="w-auto min-w-[160px] p-1"
              style={{
                background: "var(--bg-elevated)",
                border: "1px solid var(--border-subtle)",
              }}
            >
              {options.map((option) => {
                const selected = option.type === activeType;
                const toneStyle = option.tone ? FOCUS_TONE_STYLES[option.tone] : null;
                const Icon =
                  option.type === "workspace"
                    ? MessageSquare
                    : option.tone
                      ? FOCUS_TONE_ICONS[option.tone]
                      : null;

                return (
                  <button
                    key={option.type}
                    type="button"
                    aria-label={option.description}
                    data-testid={
                      option.type === "workspace"
                        ? "agents-chat-focus-return"
                        : `agents-chat-focus-option-${option.type}`
                    }
                    data-active={selected ? "true" : "false"}
                    className={cn(
                      "flex w-full items-center gap-2 rounded-md px-2.5 py-1.5 text-[0.75rem] font-medium transition-colors",
                      selected ? "cursor-default" : "cursor-pointer",
                    )}
                    style={
                      selected
                        ? toneStyle
                          ? {
                              color: toneStyle.color,
                              background: toneStyle.background,
                            }
                          : {
                              color: "var(--text-primary)",
                              background: "var(--bg-surface)",
                            }
                        : {
                            color: "var(--text-secondary)",
                            background: "transparent",
                          }
                    }
                    onMouseEnter={(e) => {
                      if (!selected) {
                        e.currentTarget.style.background = "var(--overlay-faint)";
                      }
                    }}
                    onMouseLeave={(e) => {
                      if (!selected) {
                        e.currentTarget.style.background = "transparent";
                      }
                    }}
                    onClick={() => {
                      onSelectFocus(option.type);
                      setOpen(false);
                    }}
                  >
                    {Icon ? <Icon className="h-3.5 w-3.5 shrink-0" /> : null}
                    <span>{option.label}</span>
                  </button>
                );
              })}
            </PopoverContent>
          </Popover>
        </div>
      ) : (
        <div className="min-w-0 flex-1" />
      )}
      {workspace ? <AgentsWorkspaceStatusPill workspace={workspace} /> : null}
    </div>
  );
});

export const AgentsChatHeader = memo(function AgentsChatHeader({
  conversation,
  workspace,
  chatFocus = { type: "workspace" },
  modelDisplay,
  availableArtifactTabs = [],
  artifactOpen,
  activeArtifactTab,
  terminalOpen = false,
  terminalUnavailableReason = null,
  onRenameConversation,
  onPublishWorkspace,
  onOpenPublishPane,
  onPreloadArtifacts,
  publishShortcutLabel = "Commit & Publish",
  isPublishingWorkspace = false,
  onToggleTerminal,
  onPreloadTerminal,
  onToggleArtifacts,
  onSelectArtifact,
  showTitle = true,
}: AgentsChatHeaderProps) {
  const title = conversation?.title || "Untitled agent";
  const conversationMode = conversation
    ? resolveConversationAgentMode(conversation, workspace)
    : null;
  const visibleHeaderArtifactTabs = useMemo(
    () =>
      HEADER_ARTIFACT_TABS.filter((tab) =>
        availableArtifactTabs.includes(tab.id),
      ),
    [availableArtifactTabs],
  );
  const showIdeationArtifacts =
    conversationMode === "ideation" && visibleHeaderArtifactTabs.length > 0;
  const showArtifactToggle = conversationMode === "ideation" || artifactOpen;
  // Hide the publish shortcut whenever any artifact pane is open — the user
  // can already reach Commit & Publish via the artifact tab bar, so the
  // header CTA is redundant (and visually crowds the Update-from-base label).
  const showPublishShortcut = Boolean(
    conversation &&
      shouldShowAgentWorkspacePublishSurface(workspace) &&
      !artifactOpen,
  );
  const [isEditing, setIsEditing] = useState(false);
  const [draftTitle, setDraftTitle] = useState(title);
  const conversationStoreKey = useMemo(
    () => (conversation ? getAgentConversationStoreKey(conversation) : null),
    [conversation],
  );
  const agentStatus = useChatStore((state) =>
    conversationStoreKey
      ? state.agentStatus[conversationStoreKey] ?? "idle"
      : "idle",
  );
  const isSending = useChatStore((state) =>
    conversationStoreKey ? state.isSending[conversationStoreKey] ?? false : false,
  );
  const isAgentActive = isSending || agentStatus === "generating";
  const sidebarVisibility = useAgentsSidebarVisibility();
  const showOpenSidebarButton =
    sidebarVisibility !== null && sidebarVisibility.isCollapsed;

  useEffect(() => {
    if (!isEditing) {
      setDraftTitle(title);
    }
  }, [isEditing, title]);

  const commitTitle = useCallback(async () => {
    if (!conversation) {
      setIsEditing(false);
      return;
    }
    const trimmed = draftTitle.trim();
    if (!trimmed || trimmed === title) {
      setDraftTitle(title);
      setIsEditing(false);
      return;
    }
    await onRenameConversation(conversation.id, trimmed);
    setIsEditing(false);
  }, [conversation, draftTitle, onRenameConversation, title]);

  return (
    <div
      className="flex w-full flex-1 items-center justify-between gap-3 min-w-0 overflow-hidden"
      data-testid="agents-chat-header"
      data-focus-type={chatFocus.type}
    >
      <div
        className="flex min-w-0 flex-1 items-center gap-2 overflow-hidden"
        data-testid="agents-chat-title-group"
      >
        {showOpenSidebarButton && (
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                type="button"
                variant="ghost"
                size="sm"
                className="h-8 w-8 shrink-0 p-0"
                onClick={sidebarVisibility.onToggle}
                aria-label="Open sidebar"
                data-testid="agents-chat-header-open-sidebar"
                style={{ color: "var(--text-muted)" }}
              >
                <Menu className="h-4 w-4" />
              </Button>
            </TooltipTrigger>
            <TooltipContent side="bottom" className="text-xs">
              Open sidebar
            </TooltipContent>
          </Tooltip>
        )}
        {workspace ? <AgentsWorkspaceStatusPill workspace={workspace} /> : null}
        {showTitle ? (
          <div className="min-w-0 flex-1">
            {isEditing ? (
              <Input
                value={draftTitle}
                onChange={(event) => setDraftTitle(event.target.value)}
                onBlur={() => void commitTitle()}
                onKeyDown={(event) => {
                  if (event.key === "Enter") {
                    event.preventDefault();
                    void commitTitle();
                  }
                  if (event.key === "Escape") {
                    event.preventDefault();
                    setDraftTitle(title);
                    setIsEditing(false);
                  }
                }}
                className="h-7 max-w-[260px] text-sm font-semibold"
                autoFocus
                aria-label="Agent title"
              />
            ) : (
              <button
                type="button"
                className="block w-full max-w-full text-left text-sm font-semibold truncate"
                style={{ color: "var(--text-primary)" }}
                onClick={() => conversation && setIsEditing(true)}
                aria-label="Edit agent title"
                data-testid="agents-chat-title-button"
                data-theme-button-skip="true"
              >
                {title}
              </button>
            )}
          </div>
        ) : null}
      </div>

      <div className="hidden md:flex items-center gap-1 ml-auto shrink-0">
        {conversation && (
          <ChatSessionChips
            contextType={conversation.contextType}
            contextId={conversation.contextId}
            isAgentActive={isAgentActive}
            conversationId={conversation.id}
            providerHarness={conversation.providerHarness ?? null}
            providerSessionId={conversation.providerSessionId ?? null}
            upstreamProvider={conversation.upstreamProvider ?? null}
            providerProfile={conversation.providerProfile ?? null}
            fallbackConversation={conversation}
            showProviderModel={false}
            showStats
            {...(modelDisplay !== undefined ? { modelDisplay } : {})}
          />
        )}

        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              type="button"
              variant="ghost"
              size="sm"
              className="h-8 w-8 p-0"
              onClick={onToggleTerminal}
              onPointerEnter={onPreloadTerminal}
              onFocus={onPreloadTerminal}
              disabled={!onToggleTerminal || Boolean(terminalUnavailableReason)}
              aria-label={terminalOpen ? "Close terminal" : "Open terminal"}
              data-testid="agents-terminal-toggle"
            >
              <TerminalIcon className="w-4 h-4" />
            </Button>
          </TooltipTrigger>
          <TooltipContent side="bottom" className="max-w-[280px] text-xs">
            {terminalUnavailableReason ??
              (terminalOpen ? "Close terminal" : "Open terminal")}
          </TooltipContent>
        </Tooltip>

        {showPublishShortcut && (
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                type="button"
                variant="ghost"
                size="sm"
                className="h-8 gap-1.5 px-2.5 text-xs"
                onClick={onOpenPublishPane}
                onPointerEnter={onPreloadArtifacts}
                onFocus={onPreloadArtifacts}
                disabled={
                  !onPublishWorkspace ||
                  !onOpenPublishPane ||
                  isPublishingWorkspace ||
                  (workspace?.mode === "edit" && workspace?.status === "missing")
                }
                aria-label={`Open workspace publish panel: ${publishShortcutLabel}`}
                data-testid="agents-publish-workspace"
              >
                {isPublishingWorkspace ? (
                  <Loader2 className="h-3.5 w-3.5 animate-spin" />
                ) : (
                  <GitPullRequestArrow className="h-3.5 w-3.5" />
                )}
                <span>{publishShortcutLabel}</span>
              </Button>
            </TooltipTrigger>
            <TooltipContent side="bottom" className="text-xs">
              Open the workspace publish panel
            </TooltipContent>
          </Tooltip>
        )}

        {showIdeationArtifacts && !artifactOpen &&
          visibleHeaderArtifactTabs.map(({ id, label, icon: Icon }) => {
            const isActive = activeArtifactTab === id && artifactOpen;
            return (
              <Tooltip key={id}>
                <TooltipTrigger asChild>
                  <Button
                    type="button"
                    variant="ghost"
                    size="sm"
                    className={cn("h-8 w-8 p-0", isActive ? "" : "opacity-80")}
                    onClick={() => onSelectArtifact(id)}
                    onPointerEnter={onPreloadArtifacts}
                    onFocus={onPreloadArtifacts}
                    style={{
                      color: isActive ? "var(--accent-primary)" : "var(--text-muted)",
                      background: isActive ? withAlpha("var(--accent-primary)", 12) : "transparent",
                      border: isActive
                        ? "1px solid var(--accent-border)"
                        : "1px solid var(--overlay-faint)",
                      boxShadow: "none",
                    }}
                    aria-label={label}
                  >
                    <Icon className="w-4 h-4" />
                  </Button>
                </TooltipTrigger>
                <TooltipContent side="bottom" className="text-xs">
                  {label}
                </TooltipContent>
              </Tooltip>
            );
          })}

        {showArtifactToggle ? (
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                type="button"
                variant="ghost"
                size="sm"
                className="h-8 w-8 p-0"
                onClick={onToggleArtifacts}
                onPointerEnter={onPreloadArtifacts}
                onFocus={onPreloadArtifacts}
                aria-label={artifactOpen ? "Close panel" : "Open artifacts"}
              >
                {artifactOpen ? (
                  <PanelRightClose className="w-4 h-4" />
                ) : (
                  <PanelRightOpen className="w-4 h-4" />
                )}
              </Button>
            </TooltipTrigger>
            <TooltipContent side="bottom" className="text-xs">
              {artifactOpen ? "Close panel" : "Open artifacts"}
            </TooltipContent>
          </Tooltip>
        ) : null}
      </div>
    </div>
  );
});

const AgentsWorkspaceStatusPill = memo(function AgentsWorkspaceStatusPill({
  workspace,
}: {
  workspace: AgentConversationWorkspace;
}) {
  const branch = formatBranchDisplay(workspace.branchName);
  const terminalStatus = workspace.publicationPrStatus === "merged" || workspace.publicationPrStatus === "closed"
    ? workspace.publicationPrStatus
    : null;
  const { data: freshness } = useQuery({
    queryKey: agentWorkspaceKeys.freshness(workspace.conversationId),
    queryFn: () => chatApi.getAgentConversationWorkspaceFreshness(workspace.conversationId),
    enabled:
      !terminalStatus &&
      (workspace.mode === "edit" || hasPublishedWorkspacePr(workspace)),
    staleTime: AGENT_WORKSPACE_FRESHNESS_STALE_MS,
  });
  const isBaseBlocked = freshness?.baseStatus === "blocked";
  const isBehindBase = !isBaseBlocked && !terminalStatus && Boolean(freshness?.isBaseAhead);
  const statusLabel = terminalStatus
    ? terminalStatus.replace(/_/g, " ")
    : isBaseBlocked
      ? "Base unavailable"
    : isBehindBase
      ? "Behind base"
      : (workspace.publicationPushStatus ?? workspace.status).replace(/_/g, " ");
  const baseLabel = getAgentWorkspaceEffectiveBaseLabel(workspace, freshness);
  const statusColor = isBaseBlocked || isBehindBase
    ? "var(--status-warning)"
    : "var(--text-secondary)";

  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <div
          tabIndex={0}
          className="inline-flex min-w-0 max-w-[180px] items-center gap-1.5 rounded-full px-2.5 py-1 text-[0.6875rem] font-medium sm:max-w-[300px]"
          style={{
            color: statusColor,
            background: "transparent",
          }}
          data-testid="agents-workspace-status"
        >
          <GitBranch className="h-3.5 w-3.5 shrink-0" />
          <span className="truncate font-mono">{branch.short}</span>
          <span
            className="h-1 w-1 shrink-0 rounded-full"
            style={{
              background:
                isBaseBlocked || isBehindBase
                  ? "var(--status-warning)"
                  : "var(--accent-primary)",
            }}
          />
          <span className="shrink-0 capitalize">{statusLabel}</span>
        </div>
      </TooltipTrigger>
      <TooltipContent side="bottom" className="max-w-[360px] text-xs">
        <div className="space-y-1">
          <div>Branch: {branch.full}</div>
          <div>Base: {baseLabel}</div>
          {freshness?.baseBlockReason && <div>{freshness.baseBlockReason}</div>}
          {workspace.publicationPrUrl && (
            <div>
              PR:{" "}
              {workspace.publicationPrNumber
                ? `#${workspace.publicationPrNumber}`
                : workspace.publicationPrUrl}
            </div>
          )}
        </div>
      </TooltipContent>
    </Tooltip>
  );
});
