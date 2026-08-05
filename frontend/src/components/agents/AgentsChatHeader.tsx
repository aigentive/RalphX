import {
  memo,
  useCallback,
  useEffect,
  useMemo,
  useState,
  type ElementType,
  type ReactNode,
} from "react";
import { useQuery } from "@tanstack/react-query";
import {
  AlertCircle,
  ArrowLeft,
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
  Ticket,
  UsersRound,
} from "lucide-react";

import type { AgentConversationWorkspace, WorkspaceOpenTarget } from "@/api/chat";
import * as chatApi from "@/api/chat";
import { ChatSessionChips } from "@/components/Chat/ChatSessionChips";
import { useIsRemoteEnvironment } from "@/hooks/useActiveEnvironment";
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
import { useConversationTicket } from "@/hooks/useTicketing";
import { useAgentGate } from "@/hooks/useAgentGate";
import { useChatStore } from "@/stores/chatStore";
import type { AgentArtifactTab } from "@/stores/agentSessionStore";
import type { ModelDisplay } from "@/types/chat-conversation";

import { useAgentsSidebarVisibility } from "./useAgentsSidebarVisibility";
import { AgentsWorkspaceOpenControl } from "./AgentsWorkspaceOpenControl";
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
import { resolveConversationAgentMode } from "./agentConversationMode";
import {
  getAgentWorkspaceEffectiveBaseLabel,
  isAgentWorkspacePublishActive,
  shouldShowAgentWorkspacePublishSurface,
} from "./agentWorkspacePublishState";
import {
  AGENT_WORKSPACE_FRESHNESS_STALE_MS,
  agentWorkspaceKeys,
  canInspectAgentWorkspaceFreshness,
} from "./agentWorkspaceQueries";

const HEADER_ARTIFACT_TABS: Array<{
  id: AgentArtifactTab;
  label: string;
  icon: ElementType;
}> = [
  { id: "review", label: "Review", icon: FileText },
  { id: "issues", label: "Issues", icon: AlertCircle },
  { id: "plan", label: "Plan", icon: FileText },
  { id: "verification", label: "Verification", icon: CheckCircle2 },
  { id: "tasks", label: "Tasks", icon: ClipboardList },
  { id: "team", label: "Team", icon: UsersRound },
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
  availableArtifactTabs?: readonly AgentArtifactTab[] | undefined;
  artifactOpen: boolean;
  activeArtifactTab: AgentArtifactTab;
  terminalOpen?: boolean;
  terminalArchivedReason?: string | null;
  terminalUnavailableReason?: string | null;
  onRenameConversation: (conversationId: string, title: string) => Promise<void>;
  onPublishWorkspace?: (conversationId: string) => Promise<void>;
  onOpenPublishPane?: () => void;
  workspaceOpenTargets?: readonly WorkspaceOpenTarget[] | undefined;
  openingWorkspaceTargetId?: string | null | undefined;
  onOpenWorkspaceTarget?: (targetId: string) => void;
  onPreloadArtifacts?: () => void;
  publishShortcutLabel?: string;
  publishShortcutWorkspace?: AgentConversationWorkspace | null;
  promotePublishShortcut?: boolean;
  isPublishingWorkspace?: boolean;
  onToggleTerminal?: () => void;
  onPreloadTerminal?: () => void;
  onToggleArtifacts: () => void;
  onSelectArtifact: (tab: AgentArtifactTab) => void;
  onBackToWorkspaceChat?: () => void;
  showTitle?: boolean;
  workspaceControl?: ReactNode;
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
  terminalArchivedReason = null,
  terminalUnavailableReason = null,
  onRenameConversation,
  onPublishWorkspace,
  onOpenPublishPane,
  workspaceOpenTargets = [],
  openingWorkspaceTargetId = null,
  onOpenWorkspaceTarget,
  onPreloadArtifacts,
  publishShortcutLabel = "Commit & Publish",
  publishShortcutWorkspace = null,
  promotePublishShortcut = false,
  isPublishingWorkspace = false,
  onToggleTerminal,
  onPreloadTerminal,
  onToggleArtifacts,
  onSelectArtifact,
  onBackToWorkspaceChat,
  showTitle = true,
  workspaceControl,
}: AgentsChatHeaderProps) {
  const isRemoteEnvironment = useIsRemoteEnvironment();
  const terminalTooltip =
    terminalUnavailableReason ??
    terminalArchivedReason ??
    (terminalOpen ? "Collapse terminal" : "Expand terminal");
  const terminalAriaLabel = terminalArchivedReason
    ? terminalOpen
      ? "Hide archived terminal"
      : "Show archived terminal"
    : terminalOpen
      ? "Collapse terminal"
      : "Expand terminal";
  const terminalPreloadHandler = terminalArchivedReason ? undefined : onPreloadTerminal;
  const builtInTerminalAction = useMemo(
    () => ({
      open: terminalOpen,
      unavailableReason: terminalUnavailableReason,
      onToggle: onToggleTerminal,
      onPreload: terminalPreloadHandler,
    }),
    [
      onToggleTerminal,
      terminalOpen,
      terminalPreloadHandler,
      terminalUnavailableReason,
    ],
  );
  const title = conversation?.title || "Untitled agent";
  const conversationMode = conversation
    ? resolveConversationAgentMode(conversation, workspace)
    : null;
  const isLinkedPlanEditWorkspace =
    conversationMode === "edit" &&
    Boolean(workspace?.linkedIdeationSessionId || workspace?.linkedPlanBranchId);
  const visibleHeaderArtifactTabs = useMemo(
    () => {
      const tabs = HEADER_ARTIFACT_TABS.filter((tab) =>
        availableArtifactTabs.includes(tab.id),
      );
      return isLinkedPlanEditWorkspace
        ? tabs.filter((tab) => tab.id === "plan" || tab.id === "issues")
        : tabs;
    },
    [availableArtifactTabs, isLinkedPlanEditWorkspace],
  );
  const showHeaderArtifactShortcuts =
    visibleHeaderArtifactTabs.length > 0 &&
    (conversationMode === "ideation" ||
      conversationMode === "plan" ||
      conversationMode === "review_pr" ||
      conversation?.contextType === "project" ||
      isLinkedPlanEditWorkspace);
  const showArtifactToggle =
    artifactOpen ||
    conversationMode === "ideation" ||
    (conversation?.contextType === "project" &&
      visibleHeaderArtifactTabs.length > 0) ||
    (conversationMode === "plan" && visibleHeaderArtifactTabs.length > 0) ||
    (conversationMode === "review_pr" && visibleHeaderArtifactTabs.length > 0);
  const showPromotedPublishShortcut =
    promotePublishShortcut && artifactOpen && activeArtifactTab === "review";
  const effectivePublishWorkspace = publishShortcutWorkspace ?? workspace;
  const isPublishShortcutPublishing =
    isPublishingWorkspace || isAgentWorkspacePublishActive(effectivePublishWorkspace);
  const publishShortcutDisplayLabel = isPublishShortcutPublishing
    ? "Publishing"
    : publishShortcutLabel;
  const publishShortcutAriaLabel = isPublishShortcutPublishing
    ? "Publishing"
    : `Open workspace publish panel: ${publishShortcutDisplayLabel}`;
  const publishShortcutTooltip = isPublishShortcutPublishing
    ? "Publishing"
    : "Open the workspace publish panel";
  // Hide the publish shortcut whenever most artifact panes are open because the
  // tab bar already exposes it. A current passed Review promotes publishing so
  // the user does not have to switch tabs just to reach the publish flow.
  const showPublishShortcut = Boolean(
    conversation &&
      shouldShowAgentWorkspacePublishSurface(effectivePublishWorkspace) &&
      (!artifactOpen || showPromotedPublishShortcut),
  );
  const showWorkspaceOpenControl = Boolean(
    workspace && workspaceOpenTargets.length > 0 && onOpenWorkspaceTarget,
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
  const conversationTicketGate = useAgentGate("ticketingConversationRead");
  const conversationTicketQuery = useConversationTicket(conversation?.id, {
    enabled: Boolean(conversation) && !conversationTicketGate.gated,
  });
  const linkedTicket = conversationTicketQuery.data ?? null;
  const isAgentActive = isSending || agentStatus === "generating";
  const sidebarVisibility = useAgentsSidebarVisibility();
  const showOpenSidebarButton =
    sidebarVisibility !== null && sidebarVisibility.isCollapsed;
  const showBackToWorkspaceChat =
    chatFocus.type !== "workspace" && Boolean(onBackToWorkspaceChat);

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

  const openLinkedTicket = useCallback(() => {
    if (!linkedTicket) {
      return;
    }
    // Open the linked issue in the provider-specific right-hand artifact tab
    // rather than navigating away to the ticketing dashboard.
    onSelectArtifact(linkedTicket.ticketRef.provider);
  }, [linkedTicket, onSelectArtifact]);

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
        {workspaceControl ??
          (workspace ? <AgentsWorkspaceStatusPill workspace={workspace} /> : null)}
        {showBackToWorkspaceChat && (
          <Button
            type="button"
            variant="outline"
            size="sm"
            className="h-7 shrink-0 gap-1.5 px-2.5 text-xs"
            onClick={onBackToWorkspaceChat}
            data-testid="agents-chat-header-back-to-workspace"
          >
            <ArrowLeft className="h-3.5 w-3.5" aria-hidden="true" />
            <span>Back to Workspace Chat</span>
          </Button>
        )}
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

      <div
        className="flex items-center gap-1 ml-auto shrink-0"
        data-testid="agents-chat-header-toolbar"
      >
        {conversation && (
          <div className="hidden lg:block">
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
          </div>
        )}

        {linkedTicket && (
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                type="button"
                variant="ghost"
                size="sm"
                className="h-8 w-8 p-0"
                onClick={openLinkedTicket}
                aria-label={`Open ticket ${linkedTicket.ticketRef.key ?? linkedTicket.ticketRef.id}`}
                data-testid="agents-linked-ticket-button"
              >
                <Ticket className="w-4 h-4" />
              </Button>
            </TooltipTrigger>
            <TooltipContent side="bottom" className="max-w-[280px] text-xs">
              {linkedTicket.title ?? linkedTicket.ticketRef.key ?? linkedTicket.ticketRef.id}
            </TooltipContent>
          </Tooltip>
        )}

        {/* The built-in terminal is module-excluded from v1 remoting (2.6-a): it
            drives a PTY on the machine running the app, so a remote client has
            nothing to attach to. Hidden rather than disabled — there is no host
            action that would turn it on. */}
        {!isRemoteEnvironment && (
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              type="button"
              variant="ghost"
              size="sm"
              className="h-8 w-8 p-0"
              onClick={onToggleTerminal}
              onPointerEnter={terminalPreloadHandler}
              onFocus={terminalPreloadHandler}
              disabled={!onToggleTerminal || Boolean(terminalUnavailableReason)}
              aria-label={terminalAriaLabel}
              data-testid="agents-terminal-toggle"
            >
              <TerminalIcon className="w-4 h-4" />
            </Button>
          </TooltipTrigger>
          <TooltipContent side="bottom" className="max-w-[280px] text-xs">
            {terminalTooltip}
          </TooltipContent>
        </Tooltip>
        )}

        {showWorkspaceOpenControl && onOpenWorkspaceTarget && (
          <AgentsWorkspaceOpenControl
            targets={workspaceOpenTargets}
            openingTargetId={openingWorkspaceTargetId}
            onOpenTarget={onOpenWorkspaceTarget}
            builtInTerminal={builtInTerminalAction}
          />
        )}

        {showPublishShortcut && (
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                type="button"
                variant="ghost"
                size="sm"
                className="h-8 gap-1.5 px-2 text-xs xl:px-2.5"
                onClick={onOpenPublishPane}
                onPointerEnter={onPreloadArtifacts}
                onFocus={onPreloadArtifacts}
                disabled={
                  !onPublishWorkspace ||
                  !onOpenPublishPane ||
                  isPublishShortcutPublishing ||
                  (effectivePublishWorkspace?.mode === "edit" &&
                    effectivePublishWorkspace?.status === "missing")
                }
                aria-label={publishShortcutAriaLabel}
                data-testid="agents-publish-workspace"
              >
                {isPublishShortcutPublishing ? (
                  <Loader2 className="h-3.5 w-3.5 animate-spin" />
                ) : (
                  <GitPullRequestArrow className="h-3.5 w-3.5" />
                )}
                <span className="hidden xl:inline">
                  {publishShortcutDisplayLabel}
                </span>
              </Button>
            </TooltipTrigger>
            <TooltipContent side="bottom" className="text-xs">
              {publishShortcutTooltip}
            </TooltipContent>
          </Tooltip>
        )}

        {showHeaderArtifactShortcuts && !artifactOpen &&
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
    queryKey: agentWorkspaceKeys.scopedFreshness(workspace.conversationId, "local"),
    queryFn: () =>
      chatApi.getAgentConversationWorkspaceFreshness(workspace.conversationId, {
        scope: "local",
      }),
    enabled:
      !terminalStatus &&
      canInspectAgentWorkspaceFreshness(workspace),
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
