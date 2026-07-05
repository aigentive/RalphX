import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  CheckCircle2,
  Clock,
  GitPullRequestArrow,
  Lightbulb,
  Loader2,
  MessageSquare,
  PanelRightOpen,
  Play,
  ShieldCheck,
  type LucideIcon,
} from "lucide-react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";

import type {
  AgentConversationRuntimeItem,
  AgentConversationRuntimeStatus,
  AgentConversationWorkspace,
  AgentConversationWorkspaceFreshness,
  AgentConversationWorkspaceMode,
  ComposerIntegrationReference,
  ForkAgentConversationResult,
} from "@/api/chat";
import { chatApi } from "@/api/chat";
import { artifactApi } from "@/api/artifact";
import { verificationApi } from "@/api/verification";
import {
  IntegratedChatPanel,
  type IntegratedChatComposerRenderProps,
} from "@/components/Chat/IntegratedChatPanel";
import type {
  AskUserQuestionPayload,
  AskUserQuestionResponse,
} from "@/types/ask-user-question";
import type { AgentRunCompletedPayload } from "@/types/events";
import { Button } from "@/components/ui/button";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import {
  fallbackBranchBaseOptions,
  loadBranchBaseOptions,
  loadPullRequestBaseOptions,
  type BranchBaseOption,
} from "@/components/shared/branchBaseOptions";
import { buildStoreKey } from "@/lib/chat-context-registry";
import {
  CODEX_FAST_MODE_DESCRIPTION,
  codexFastModeAvailabilityForProvider,
} from "@/lib/codex-fast-mode";
import { formatQueuedMessageExcerpt } from "@/lib/queuedMessageExcerpt";
import { useAgentModels } from "@/hooks/useAgentModels";
import { useConfirmation } from "@/hooks/useConfirmation";
import { useHarnessProviders } from "@/hooks/useHarnessProviders";
import { useAutomationDetail } from "@/hooks/useAutomations";
import type { SubmitQuestionAnswerResult } from "@/hooks/useAskUserQuestion";
import { ideationKeys } from "@/hooks/useIdeation";
import { useVerificationStatus, verificationStatusKey } from "@/hooks/useVerificationStatus";
import { useEventBus } from "@/providers/EventProvider";
import { selectQueuedMessages, useChatStore } from "@/stores/chatStore";
import { useUiStore } from "@/stores/uiStore";
import type {
  AgentArtifactTab,
  AgentProvider,
  AgentRuntimeSelection,
} from "@/stores/agentSessionStore";

import {
  getAgentConversationStoreKey,
  type AgentConversation,
} from "./agentConversations";
import {
  AgentComposerProjectLine,
  AgentComposerSurface,
  type AgentComposerSendOptions,
  type ChatFocusFieldConfig,
} from "./AgentComposerSurface";
import { AgentConversationBaseLine } from "./AgentConversationBaseLine";
import { AgentConversationWorkspaceLine } from "./AgentConversationWorkspaceLine";
import { AgentWorkspacePrReviewCard } from "./AgentWorkspacePrReviewCard";
import { useAgentConversationRuntimeStatus } from "./useAgentConversationRuntimeStatus";
import { AgentsComposerWorkspaceChangesCard } from "./AgentsComposerWorkspaceChangesCard";
import { AgentsChatHeaderController } from "./AgentsChatHeaderController";
import { AgentWorkspaceFileLinkProvider } from "./AgentWorkspaceFileLinkProvider";
import { useResolvedAgentArtifactState } from "./agentArtifactState";
import { AGENT_CONVERSATION_MODE_OPTIONS } from "./agentConversationMode";
import type { DiffFilterMode } from "./AgentsPublishDiffFilter";
import {
  AGENT_PROVIDER_OPTIONS,
  agentEffortOptions,
  agentModelOptions,
  normalizeRuntimeSelection,
} from "./agentOptions";
import { AgentProviderSettingsButton } from "./AgentProviderSettingsButton";
import {
  buildAgentProviderAvailabilityOptions,
  getProviderAvailabilityMessage,
  supportedEffortsForProvider,
  supportedModelAliasesForProvider,
} from "./agentProviderAvailability";
import { AgentsTerminalDockHost } from "./AgentsTerminalRegion";
import { AGENTS_CHAT_MIN_WIDTH } from "./AgentsArtifactPaneRegion";
import {
  getAgentQueueHaltState,
  type AgentQueueHaltState,
} from "./agentExecutionPause";
import type { IdeationArtifactTab } from "./agentArtifactTabs";
import {
  getFocusedChatSessionId,
  getFocusedWorkspaceReviewConversationId,
  type AgentsChatFocus,
  type AgentsChatFocusSwitchOption,
  type AgentsChatFocusType,
} from "./agentChatFocus";
import {
  isTaskRuntimeContextType,
  type AgentTaskRuntimeContextType,
} from "./agentTaskRuntimeContext";
import {
  buildPlanActionHint,
  isPlanRecommendationCheckPending,
  PLAN_IMPLEMENT_DIRECTLY_REQUEST,
  PLAN_TO_PROPOSALS_REQUEST,
} from "./agentPlanModeActions";
import {
  agentWorkspaceKeys,
  invalidateWorkspaceQueries,
  prReviewContextForConversation,
} from "./agentWorkspaceQueries";
import { getAgentWorkspaceTerminalPublicationStatus } from "./agentWorkspacePublishState";
import { useAgentWorkspaceBaseUpdate } from "./useAgentWorkspaceBaseUpdate";

const AGENTS_CHAT_CONTENT_WIDTH_CLASS = "max-w-[980px]";
const PLAN_MODE_PROPOSAL_KIND = "plan_mode_proposal";
const PLAN_MODE_PROPOSAL_ACCEPT_VALUE = "switch_to_plan";
const PLAN_MODE_SWITCH_EVENT_RETRY_DELAY_MS = 150;
const PLAN_MODE_SWITCH_FALLBACK_RETRY_DELAY_MS = 750;
const PLAN_MODE_SWITCH_MAX_RETRY_ATTEMPTS = 40;
const TERMINAL_AUTOMATION_RUN_STATUSES = new Set([
  "merged",
  "pr_closed",
  "agent_failed",
  "cancelled",
]);

function getWorkspaceBasePickerKey(
  workspace: AgentConversationWorkspace | null,
  freshness: AgentConversationWorkspaceFreshness | undefined,
): string {
  if (!workspace) {
    return "";
  }
  const baseRef =
    freshness?.effectiveBaseRef ?? freshness?.baseRef ?? workspace.baseRef;
  const baseKind =
    freshness?.baseStatus === "retargeted"
      ? "project_default"
      : workspace.baseRefKind;
  return `${baseKind}:${baseRef}`;
}

interface PendingPlanModeSwitch {
  conversationId: string;
  attempt: number;
  autoContinueMessage: string | null;
}

function getPlanModeProposalConversationId(
  question: AskUserQuestionPayload,
): string | null {
  const metadata = question.metadata;
  if (!metadata || metadata.kind !== PLAN_MODE_PROPOSAL_KIND) {
    return null;
  }
  const conversationId = metadata.conversation_id;
  return typeof conversationId === "string" && conversationId.trim()
    ? conversationId.trim()
    : question.sessionId ?? null;
}

function acceptsPlanModeProposal(response: AskUserQuestionResponse): boolean {
  return (
    response.skipped !== true &&
    response.selectedOptions.includes(PLAN_MODE_PROPOSAL_ACCEPT_VALUE)
  );
}

function getPlanModeProposalReason(question: AskUserQuestionPayload): string | null {
  const reason = question.metadata?.reason;
  return typeof reason === "string" && reason.trim() ? reason.trim() : null;
}

function buildPlanModeProposalContinuationMessage(
  question: AskUserQuestionPayload,
): string {
  const reason = getPlanModeProposalReason(question);
  const base =
    "Continue in Plan mode from the accepted proposal. Work with me on a concrete plan before implementation.";
  return reason ? `${base}\n\nPlanning focus: ${reason}` : base;
}

function isRunningModeSwitchError(err: unknown): boolean {
  const message = err instanceof Error ? err.message : String(err);
  return message.includes("Cannot change mode while the agent is running");
}

function isRuntimeItemOwnedByFocus(
  item: AgentConversationRuntimeItem,
  chatFocus: AgentsChatFocus,
): boolean {
  if (chatFocus.type === "workspace") {
    return item.source === "workspace";
  }
  if (chatFocus.type === "ideation") {
    return item.source === "ideation" && item.contextId === chatFocus.sessionId;
  }
  if (chatFocus.type === "verification") {
    return (
      item.source === "verification" &&
      item.parentSessionId === chatFocus.parentSessionId &&
      (item.childSessionId ?? item.contextId) === chatFocus.childSessionId
    );
  }
  if (chatFocus.type === "workspace_review") {
    return (
      item.source === "workspace_review" &&
      (item.conversationId ?? item.contextId) === chatFocus.conversationId
    );
  }
  return (
    item.taskId === chatFocus.taskId &&
    item.contextType === chatFocus.contextType &&
    isTaskRuntimeContextType(item.contextType)
  );
}

function runtimeStatusForChatFocus(
  status: AgentConversationRuntimeStatus | null | undefined,
  chatFocus: AgentsChatFocus,
): AgentConversationRuntimeStatus | null | undefined {
  if (!status?.items.length) {
    return status;
  }

  const focusedItems = status.items.filter((item) =>
    isRuntimeItemOwnedByFocus(item, chatFocus),
  );
  if (focusedItems.length === status.items.length) {
    return status;
  }
  if (focusedItems.length === 0) {
    return {
      ...status,
      isRunning: false,
      agentStatus: "idle",
      primarySource: null,
      summaryLabel: null,
      items: [],
    };
  }

  const hasGeneratingItem = focusedItems.some(
    (item) => item.agentStatus === "generating",
  );
  const primarySource = focusedItems.some(
    (item) => item.source === status.primarySource,
  )
    ? status.primarySource
    : focusedItems[0]?.source ?? status.primarySource;

  return {
    ...status,
    isRunning: true,
    agentStatus: hasGeneratingItem ? "generating" : "waiting_for_input",
    primarySource,
    summaryLabel: focusedItems[0]?.label ?? status.summaryLabel,
    items: focusedItems,
  };
}

function parseForkCommand(message: string): string | null {
  const trimmed = message.trim();
  if (trimmed === "/fork") {
    return "";
  }
  if (/^\/fork\s/.test(trimmed)) {
    return trimmed.slice("/fork".length).trimStart();
  }
  return null;
}

interface AgentComposerOption {
  id: string;
  label: string;
  description?: string;
  disabled?: boolean;
  disabledReason?: string;
}

interface PlanComposerCtaAction {
  id: string;
  label: string;
  icon: LucideIcon;
  isPrimary: boolean;
  isPending: boolean;
  disabled: boolean;
  onClick: () => void;
}

interface PlanComposerViewPlanAction {
  available: boolean;
  conversationId: string;
  hasAutoOpenArtifacts: boolean;
  isPlanVisible: boolean;
  onClick: () => void;
}

function getPlanComposerCompactHint(
  hint: string,
  actions: PlanComposerCtaAction[],
): string {
  const trimmedHint = hint.trim();
  const recommendedMatch = /^Recommended:\s*([^.]*)\./.exec(trimmedHint);
  if (recommendedMatch?.[1]) {
    return `Recommended: ${recommendedMatch[1]}`;
  }
  if (trimmedHint.startsWith("Assessing plan complexity")) {
    return "Assessing plan complexity";
  }
  if (trimmedHint.startsWith("Checking recommended next action")) {
    return "Checking recommended next action";
  }

  const primaryAction = actions.find((action) => action.isPrimary) ?? actions[0];
  if (primaryAction?.id === "approve") {
    return "Approve draft plan";
  }
  if (primaryAction) {
    return `Recommended: ${primaryAction.label}`;
  }
  return trimmedHint;
}

function getPlanComposerHintDetails(hint: string, compactHint: string): string | null {
  const trimmedHint = hint.trim();
  if (!trimmedHint || trimmedHint === compactHint) {
    return null;
  }

  const compactPrefix = `${compactHint}.`;
  if (trimmedHint.startsWith(compactPrefix)) {
    const details = trimmedHint.slice(compactPrefix.length).trim();
    return details.length > 0 ? details : null;
  }

  return trimmedHint;
}

function PlanComposerCtaRow({
  hint,
  actions,
  viewPlanAction,
}: {
  hint: string;
  actions: PlanComposerCtaAction[];
  viewPlanAction?: PlanComposerViewPlanAction | undefined;
}) {
  const { artifactState } = useResolvedAgentArtifactState(
    viewPlanAction?.conversationId ?? null,
    viewPlanAction?.hasAutoOpenArtifacts ?? false,
  );
  const isPlanVisible =
    viewPlanAction?.isPlanVisible ??
    (artifactState.isOpen && artifactState.activeTab === "plan");
  const resolvedActions = useMemo<PlanComposerCtaAction[]>(() => {
    if (!viewPlanAction?.available || isPlanVisible) {
      return actions;
    }

    return [
      {
        id: "view-plan",
        label: "View Plan",
        icon: PanelRightOpen,
        isPrimary: false,
        isPending: false,
        disabled: false,
        onClick: viewPlanAction.onClick,
      },
    ];
  }, [actions, isPlanVisible, viewPlanAction]);

  if (resolvedActions.length === 0) {
    return null;
  }
  const compactHint = getPlanComposerCompactHint(hint, resolvedActions);
  const hintDetails = getPlanComposerHintDetails(hint, compactHint);
  const isRecommendation = compactHint.startsWith("Recommended:");
  const isRecommendationCheckPending = compactHint.startsWith(
    "Checking recommended next action",
  );
  const detailsButton = hintDetails ? (
    <Tooltip>
      <TooltipTrigger asChild>
        <button
          type="button"
          className="inline-flex h-7 shrink-0 items-center justify-center rounded-md border px-2 text-[0.6875rem] font-medium outline-none transition-colors hover:bg-[var(--bg-surface-hover)] focus-visible:ring-2 focus-visible:ring-[var(--accent-primary)]"
          style={{
            borderColor: "var(--border-subtle)",
            borderStyle: "solid",
            borderWidth: "1px",
            color: "var(--text-muted)",
          }}
          data-testid="agents-plan-composer-cta-details"
        >
          why?
        </button>
      </TooltipTrigger>
      <TooltipContent
        side="top"
        align="start"
        className="max-w-[19rem] text-xs font-normal leading-5"
      >
        {hintDetails}
      </TooltipContent>
    </Tooltip>
  ) : null;
  const renderActionButton = (action: PlanComposerCtaAction) => {
    const Icon = action.isPending ? Loader2 : action.icon;
    return (
      <Button
        key={action.id}
        type="button"
        size="sm"
        variant={action.isPrimary ? "default" : "outline"}
        onClick={action.onClick}
        disabled={action.disabled || action.isPending}
        data-testid={`agents-plan-composer-cta-${action.id}`}
      >
        <Icon
          className={
            action.isPending ? "h-3.5 w-3.5 animate-spin" : "h-3.5 w-3.5"
          }
          aria-hidden="true"
        />
        <span>{action.label}</span>
      </Button>
    );
  };

  const isSingleAction = resolvedActions.length === 1;

  return (
    <div
      className="mx-2 mb-2 rounded-md border px-3 py-2"
      style={{
        backgroundColor: "var(--bg-surface)",
        borderColor: "var(--border-subtle)",
        borderStyle: "solid",
        borderWidth: "1px",
      }}
      data-testid="agents-plan-composer-cta-row"
    >
      {isSingleAction ? (
        <div className="flex items-center gap-2">
          <div
            className="flex min-w-0 flex-1 items-center gap-2"
            data-testid="agents-plan-composer-cta-copy"
          >
            {isRecommendation && (
              <Lightbulb
                className="h-4 w-4 shrink-0"
                style={{ color: "var(--accent-primary)" }}
                aria-hidden="true"
              />
            )}
            {isRecommendationCheckPending && (
              <Loader2
                className="h-4 w-4 shrink-0 animate-spin"
                style={{ color: "var(--accent-primary)" }}
                aria-hidden="true"
              />
            )}
            <p
              className={
                isRecommendation
                  ? "min-w-0 text-[0.6875rem] font-semibold uppercase leading-5 tracking-[0.12em]"
                  : "min-w-0 truncate text-[0.8125rem] font-medium leading-5"
              }
              style={{ color: "var(--text-primary)" }}
              data-testid="agents-plan-composer-cta-hint"
            >
              {compactHint}
            </p>
            {detailsButton}
          </div>
          <div
            className="flex shrink-0 items-center"
            role="group"
            aria-label="Plan actions"
            data-testid="agents-plan-composer-cta-actions"
          >
            {renderActionButton(resolvedActions[0]!)}
          </div>
        </div>
      ) : (
        <>
          <div
            className="flex flex-wrap items-center gap-2 border-b pb-2"
            style={{
              borderColor: "var(--border-subtle)",
              borderStyle: "solid",
              borderWidth: "0 0 1px",
            }}
          >
            <div
              className="flex min-w-0 items-center gap-2 pr-1"
              data-testid="agents-plan-composer-cta-copy"
            >
              {isRecommendation && (
                <Lightbulb
                  className="h-4 w-4 shrink-0"
                  style={{ color: "var(--accent-primary)" }}
                  aria-hidden="true"
                />
              )}
              {isRecommendationCheckPending && (
                <Loader2
                  className="h-4 w-4 shrink-0 animate-spin"
                  style={{ color: "var(--accent-primary)" }}
                  aria-hidden="true"
                />
              )}
              <p
                className={
                  isRecommendation
                    ? "min-w-0 text-[0.6875rem] font-semibold uppercase leading-5 tracking-[0.12em]"
                    : "min-w-0 truncate text-[0.8125rem] font-medium leading-5"
                }
                style={{ color: "var(--text-primary)" }}
                data-testid="agents-plan-composer-cta-hint"
              >
                {compactHint}
              </p>
              {detailsButton}
            </div>
          </div>
          <div
            className="mt-2 flex flex-wrap items-center gap-2"
            role="group"
            aria-label="Plan actions"
            data-testid="agents-plan-composer-cta-actions"
          >
            {resolvedActions.map(renderActionButton)}
          </div>
        </>
      )}
    </div>
  );
}

interface AgentsActiveConversationPanelProps {
  activeConversation: AgentConversation;
  activeConversationMode: AgentConversationWorkspaceMode | null;
  activeConversationModeLocked: boolean;
  activeProjectId: string;
  activeProjectOptions: AgentComposerOption[];
  activeWorkspace: AgentConversationWorkspace | null;
  activeWorkspaceFreshness: AgentConversationWorkspaceFreshness | undefined;
  attachedIdeationSessionId: string | null;
  availableArtifactTabs: readonly IdeationArtifactTab[];
  chatFocus: AgentsChatFocus;
  chatFocusOptions: readonly AgentsChatFocusSwitchOption[];
  hasAutoOpenArtifacts: boolean;
  normalizedActiveRuntime: AgentRuntimeSelection;
  onActiveConversationModeChange: (mode: AgentConversationWorkspaceMode) => void;
  onActiveConversationModeMenuOpen: () => void;
  onActiveEffortChange: (
    effort: string,
    providerSupportedEfforts?: readonly string[] | null,
    providerSupportedModelAliases?: readonly string[] | null
  ) => void;
  onActiveModelChange: (
    modelId: string,
    providerSupportedEfforts?: readonly string[] | null,
    providerSupportedModelAliases?: readonly string[] | null
  ) => void;
  onActiveProviderChange: (
    provider: AgentProvider,
    providerSupportedEfforts?: readonly string[] | null
  ) => void;
  onAgentUserMessageSent: (event: {
    content: string;
    result: { conversationId: string };
    composerIntegrationReferences?: ComposerIntegrationReference[];
  }) => void;
  onConversationModeSwitched: (
    conversationId: string,
    mode: AgentConversationWorkspaceMode,
    workspace: AgentConversationWorkspace | null
  ) => void;
  onFocusIdeationSession: (sessionId: string) => void;
  onFocusWorkspaceReview: (conversationId: string) => void;
  onFocusVerificationSession: (
    parentSessionId: string,
    childSessionId: string
  ) => void;
  onFocusTaskRuntime: (
    taskId: string,
    contextType: AgentTaskRuntimeContextType
  ) => void;
  onOpenTaskArtifact: (taskId: string) => void;
  onForkConversation: (
    conversationId: string
  ) => Promise<ForkAgentConversationResult>;
  onOpenPlanArtifact: () => void;
  onOpenPublishPane: () => void;
  onOpenPublishFile: (filePath: string, mode: DiffFilterMode) => void;
  onPreloadArtifacts: () => void;
  onPublishWorkspace: (conversationId: string) => Promise<void>;
  onRenameConversation: (conversationId: string, title: string) => Promise<void>;
  onSelectArtifact: (tab: AgentArtifactTab) => void;
  onToggleArtifacts: (conversationId: string) => void;
  onSelectChatFocus: (type: AgentsChatFocusType) => void;
  publishShortcutLabel: string;
  promotePublishShortcut?: boolean;
  publishingConversationId: string | null;
  selectedConversationId: string;
  selectedTaskArtifactId: string | null;
  setTerminalChatDockElement: (element: HTMLDivElement | null) => void;
  switchingConversationModeId: string | null;
  terminalArchivedReason: string | null;
  terminalUnavailableReason: string | null;
}

export const AgentsActiveConversationPanel = memo(function AgentsActiveConversationPanel({
  activeConversation,
  activeConversationMode,
  activeConversationModeLocked,
  activeProjectId,
  activeProjectOptions,
  activeWorkspace,
  activeWorkspaceFreshness,
  attachedIdeationSessionId,
  availableArtifactTabs,
  chatFocus,
  chatFocusOptions,
  hasAutoOpenArtifacts,
  normalizedActiveRuntime,
  onActiveConversationModeChange,
  onActiveConversationModeMenuOpen,
  onActiveEffortChange,
  onActiveModelChange,
  onActiveProviderChange,
  onAgentUserMessageSent,
  onConversationModeSwitched,
  onFocusIdeationSession,
  onFocusWorkspaceReview,
  onFocusVerificationSession,
  onFocusTaskRuntime,
  onOpenTaskArtifact,
  onForkConversation,
  onOpenPlanArtifact,
  onOpenPublishPane,
  onOpenPublishFile,
  onPreloadArtifacts,
  onPublishWorkspace,
  onRenameConversation,
  onSelectArtifact,
  onToggleArtifacts,
  onSelectChatFocus,
  publishShortcutLabel,
  promotePublishShortcut = false,
  publishingConversationId,
  selectedConversationId,
  selectedTaskArtifactId,
  setTerminalChatDockElement,
  switchingConversationModeId,
  terminalArchivedReason,
  terminalUnavailableReason,
}: AgentsActiveConversationPanelProps) {
  const queryClient = useQueryClient();
  const bus = useEventBus();
  const focusedChatSessionId = getFocusedChatSessionId(chatFocus);
  const focusedWorkspaceReviewConversationId =
    getFocusedWorkspaceReviewConversationId(chatFocus);
  const runtimeControlConversationId =
    focusedWorkspaceReviewConversationId ?? selectedConversationId;
  const { registry: modelRegistry } = useAgentModels();
  const { confirm, confirmationDialogProps, ConfirmationDialog } = useConfirmation();
  const openModal = useUiStore((s) => s.openModal);
  const {
    providers: configuredProviders,
    isLoading: isLoadingProviderSettings,
    isPlaceholderData: isPlaceholderProviderSettings,
  } = useHarnessProviders({ refreshRuntime: true });
  const providerSettingsReady =
    !isLoadingProviderSettings && !isPlaceholderProviderSettings;
  const providerOptions = useMemo(
    () =>
      buildAgentProviderAvailabilityOptions({
        providers: configuredProviders,
        isReady: providerSettingsReady,
      }),
    [configuredProviders, providerSettingsReady],
  );
  const [composerActivityTick, setComposerActivityTick] = useState(0);
  const [isComposerHydrationPaused, setIsComposerHydrationPaused] = useState(false);
  const [isForkingConversation, setIsForkingConversation] = useState(false);
  const [isApprovingPlan, setIsApprovingPlan] = useState(false);
  const [isCreatingPlanProposals, setIsCreatingPlanProposals] = useState(false);
  const [isImplementingPlanDirectly, setIsImplementingPlanDirectly] = useState(false);
  const [isStartingPlanVerification, setIsStartingPlanVerification] = useState(false);
  const [codexFastModeByConversationId, setCodexFastModeByConversationId] =
    useState<Record<string, boolean>>({});
  const [
    shouldLoadWorkspaceBaseOptions,
    setShouldLoadWorkspaceBaseOptions,
  ] = useState(false);
  const [
    workspaceBasePullRequestOptions,
    setWorkspaceBasePullRequestOptions,
  ] = useState<BranchBaseOption[]>([]);
  const [
    isLoadingWorkspaceBasePullRequests,
    setIsLoadingWorkspaceBasePullRequests,
  ] = useState(false);
  const [
    workspaceBasePullRequestMessage,
    setWorkspaceBasePullRequestMessage,
  ] = useState<string | null>(null);
  const [
    pendingPlanModeSwitch,
    setPendingPlanModeSwitch,
  ] = useState<PendingPlanModeSwitch | null>(null);
  const pendingPlanModeSwitchConversationIdRef = useRef<string | null>(null);
  const pendingPlanModeSwitchAutoContinueMessageRef = useRef<string | null>(null);
  const pendingPlanModeSwitchRetryCountRef = useRef(0);
  const workspaceBasePullRequestRequestRef = useRef(0);
  const markComposerActivity = useCallback(() => {
    setIsComposerHydrationPaused(true);
    setComposerActivityTick((tick) => tick + 1);
  }, []);
  useEffect(() => {
    setIsComposerHydrationPaused(false);
    setShouldLoadWorkspaceBaseOptions(false);
    setWorkspaceBasePullRequestOptions([]);
    setWorkspaceBasePullRequestMessage(null);
    setIsLoadingWorkspaceBasePullRequests(false);
    workspaceBasePullRequestRequestRef.current += 1;
  }, [selectedConversationId]);
  const {
    isUpdatingFromBase: isUpdatingComposerWorkspaceBase,
    runUpdateFromBase: runComposerWorkspaceBaseUpdate,
  } = useAgentWorkspaceBaseUpdate({
    conversationTitle: activeConversation.title,
  });
  useEffect(() => {
    if (!isComposerHydrationPaused) {
      return;
    }

    const timer = window.setTimeout(() => {
      setIsComposerHydrationPaused(false);
    }, 900);

    return () => window.clearTimeout(timer);
  }, [composerActivityTick, isComposerHydrationPaused]);
  const workspaceProviderStatusMessage = getProviderAvailabilityMessage({
    provider: normalizedActiveRuntime.provider,
    providerOptions,
    isReady: providerSettingsReady,
  });
  const codexProviderSettings = configuredProviders.find(
    (entry) => entry.provider === "codex",
  );
  const codexProviderFastMode =
    codexProviderSettings?.serviceTier?.trim().toLowerCase() === "fast";
  const conversationServiceTier = activeConversation.serviceTier
    ?.trim()
    .toLowerCase();
  const conversationFastMode =
    conversationServiceTier === "fast"
      ? true
      : conversationServiceTier === "standard"
        ? false
        : codexProviderFastMode;
  const runtimeControlFastModeFallback =
    activeConversation.id === runtimeControlConversationId
      ? conversationFastMode
      : codexProviderFastMode;
  const activeCodexFastMode =
    codexFastModeByConversationId[runtimeControlConversationId] ??
    runtimeControlFastModeFallback;
  const handleActiveCodexFastModeChange = useCallback(
    (value: boolean) => {
      setCodexFastModeByConversationId((current) => ({
        ...current,
        [runtimeControlConversationId]: value,
      }));
    },
    [runtimeControlConversationId],
  );
  const workspaceProviderSupportedEfforts = useMemo(
    () =>
      supportedEffortsForProvider(
        providerOptions,
        normalizedActiveRuntime.provider
      ),
    [normalizedActiveRuntime.provider, providerOptions]
  );
  const workspaceProviderSupportedModelAliases = useMemo(
    () =>
      supportedModelAliasesForProvider(
        providerOptions,
        normalizedActiveRuntime.provider,
      ),
    [normalizedActiveRuntime.provider, providerOptions]
  );
  const selectableWorkspaceRuntime = useMemo(
    () =>
      normalizeRuntimeSelection(
        normalizedActiveRuntime,
        modelRegistry,
        workspaceProviderSupportedEfforts,
        workspaceProviderSupportedModelAliases
      ),
    [
      modelRegistry,
      normalizedActiveRuntime,
      workspaceProviderSupportedEfforts,
      workspaceProviderSupportedModelAliases,
    ]
  );
  const codexFastModeAvailability = codexFastModeAvailabilityForProvider({
    provider: codexProviderSettings,
    modelId: selectableWorkspaceRuntime.modelId,
    isReady: providerSettingsReady,
  });
  const activeCodexFastModeOption =
    normalizedActiveRuntime.provider === "codex" &&
    codexFastModeAvailability.supported
      ? activeCodexFastMode
      : null;
  const selectableCodexFastMode =
    normalizedActiveRuntime.provider === "codex" &&
    codexFastModeAvailability.supported
      ? activeCodexFastMode
      : false;
  const openProviderSettings = useCallback(() => {
    openModal("settings", { section: "providers" });
  }, [openModal]);
  const panelIdeationSessionId =
    focusedChatSessionId ??
    (activeConversation.contextType === "ideation" ? activeConversation.contextId : undefined);
  const taskRuntimeFocus = chatFocus.type === "task_runtime" ? chatFocus : null;
  const panelSelectedTaskId = taskRuntimeFocus?.taskId ?? null;
  const panelTaskRuntimeContextType = taskRuntimeFocus?.contextType;
  const focusedPanelKey = taskRuntimeFocus
    ? `${taskRuntimeFocus.contextType}:${taskRuntimeFocus.taskId}`
    : focusedWorkspaceReviewConversationId
    ? `workspace_review:${focusedWorkspaceReviewConversationId}`
    : focusedChatSessionId ?? "workspace";
  const isFocusedChildChat = chatFocus.type !== "workspace";
  const automationRunConversationId =
    activeConversation.automationId && activeConversation.automationRunId
      ? activeConversation.automationRunId
      : null;
  const automationRunDetailQuery = useAutomationDetail(
    activeConversation.automationId,
    {
      enabled: Boolean(automationRunConversationId) && !isFocusedChildChat,
    },
  );
  const automationRun = useMemo(
    () =>
      automationRunDetailQuery.data?.runs.find(
        (run) => run.id === automationRunConversationId,
      ) ?? null,
    [automationRunConversationId, automationRunDetailQuery.data?.runs],
  );
  const automationRunReadOnlyReason =
    automationRunConversationId &&
    !isFocusedChildChat &&
    (!automationRun || !TERMINAL_AUTOMATION_RUN_STATUSES.has(automationRun.status))
      ? "Automation run conversations are read-only until the run reaches a terminal state."
      : null;
  const usesWorkspaceRuntimeControls =
    !isFocusedChildChat || chatFocus.type === "workspace_review";
  const runtimeStatusConversationId =
    activeConversation.parentConversationId ?? selectedConversationId;
  const runtimeStatusStoreKey = runtimeStatusConversationId
    ? buildStoreKey("project", runtimeStatusConversationId)
    : null;
  const selectVisibleRuntimeStatus = useCallback(
    (status: AgentConversationRuntimeStatus | null | undefined) =>
      runtimeStatusForChatFocus(status, chatFocus),
    [chatFocus],
  );
  const runtimeStatusQuery = useAgentConversationRuntimeStatus(
    runtimeStatusConversationId,
    {
      enabled: activeConversation.contextType === "project",
      invalidateUnknownRuntimeIds: isFocusedChildChat,
      selectVisibleChatStatus: selectVisibleRuntimeStatus,
      storeKey: runtimeStatusStoreKey,
    },
  );
  const runtimeStatusItems = runtimeStatusQuery.data?.items;
  const belowTranscriptLayoutOwnedByVisibleRuntime = useMemo(() => {
    if (isFocusedChildChat) {
      return true;
    }
    const runtimeItems = runtimeStatusItems ?? [];
    if (runtimeItems.length === 0) {
      return true;
    }
    return runtimeItems.some((item) => isRuntimeItemOwnedByFocus(item, chatFocus));
  }, [chatFocus, isFocusedChildChat, runtimeStatusItems]);
  useEffect(() => {
    if (!selectedTaskArtifactId) {
      return;
    }
    const matchingTaskRuntime = runtimeStatusQuery.data?.items.find(
      (item) =>
        item.taskId === selectedTaskArtifactId &&
        isTaskRuntimeContextType(item.contextType),
    );
    if (
      !matchingTaskRuntime?.taskId ||
      !isTaskRuntimeContextType(matchingTaskRuntime.contextType)
    ) {
      return;
    }
    if (
      chatFocus.type === "task_runtime" &&
      chatFocus.taskId === matchingTaskRuntime.taskId &&
      chatFocus.contextType === matchingTaskRuntime.contextType
    ) {
      return;
    }
    onFocusTaskRuntime(matchingTaskRuntime.taskId, matchingTaskRuntime.contextType);
  }, [
    chatFocus,
    onFocusTaskRuntime,
    runtimeStatusQuery.data?.items,
    selectedTaskArtifactId,
  ]);
  const activeConversationStoreKey = useMemo(
    () => getAgentConversationStoreKey(activeConversation),
    [activeConversation],
  );
  const activeConversationAgentStatus = useChatStore(
    (state) => state.agentStatus[activeConversationStoreKey] ?? "idle",
  );
  const activeConversationIsSending = useChatStore(
    (state) => state.isSending[activeConversationStoreKey] ?? false,
  );
  const workspaceBaseSelectorAvailable =
    !isFocusedChildChat &&
    activeConversation.contextType === "project" &&
    Boolean(activeWorkspace?.conversationId) &&
    activeWorkspace?.status !== "missing" &&
    !getAgentWorkspaceTerminalPublicationStatus(activeWorkspace);
  const workspaceBaseEditable =
    workspaceBaseSelectorAvailable &&
    activeConversationAgentStatus !== "generating" &&
    !activeConversationIsSending &&
    !isForkingConversation &&
    !isUpdatingComposerWorkspaceBase;
  const fallbackWorkspaceBaseOptions = useMemo(
    () =>
      fallbackBranchBaseOptions(
        activeWorkspaceFreshness?.effectiveBaseRef ??
          activeWorkspaceFreshness?.baseRef ??
          activeWorkspace?.baseRef ??
          "main",
      ),
    [
      activeWorkspace?.baseRef,
      activeWorkspaceFreshness?.baseRef,
      activeWorkspaceFreshness?.effectiveBaseRef,
    ],
  );
  const workspaceBaseOptionsQuery = useQuery({
    queryKey: [
      "agents",
      "conversation-workspace-base-options",
      activeWorkspace?.conversationId,
      activeProjectId,
      activeWorkspace?.worktreePath,
      activeWorkspace?.branchName,
      activeWorkspace?.baseRef,
    ],
    queryFn: async () => {
      const result = await loadBranchBaseOptions({
        projectId: activeProjectId,
        workingDirectory: activeWorkspace!.worktreePath,
        includeAgentBranches: false,
      });
      return {
        options: result.options.filter(
          (option) => option.selection.ref !== activeWorkspace!.branchName,
        ),
        selectedKey: result.selectedKey,
      };
    },
    enabled:
      workspaceBaseSelectorAvailable &&
      shouldLoadWorkspaceBaseOptions &&
      Boolean(activeWorkspace?.worktreePath),
    staleTime: 10_000,
  });
  const workspaceBaseOptionsResult =
    workspaceBaseOptionsQuery.data ?? fallbackWorkspaceBaseOptions;
  const workspaceBaseOptions = workspaceBaseOptionsResult.options;
  const workspaceBasePickerValue = getWorkspaceBasePickerKey(
    activeWorkspace,
    activeWorkspaceFreshness,
  );
  const workspaceBaseSelectionOptions = useMemo(
    () => [...workspaceBaseOptions, ...workspaceBasePullRequestOptions],
    [workspaceBaseOptions, workspaceBasePullRequestOptions],
  );
  const handleWorkspaceBasePickerIntent = useCallback(() => {
    if (workspaceBaseSelectorAvailable) {
      setShouldLoadWorkspaceBaseOptions(true);
    }
  }, [workspaceBaseSelectorAvailable]);
  const handleWorkspaceBasePickerOpenChange = useCallback(
    (open: boolean) => {
      if (open && workspaceBaseSelectorAvailable) {
        setShouldLoadWorkspaceBaseOptions(true);
      }
    },
    [workspaceBaseSelectorAvailable],
  );
  const searchWorkspaceBasePullRequestOptions = useCallback(
    (query: string) => {
      if (!activeProjectId || !workspaceBaseSelectorAvailable) {
        setWorkspaceBasePullRequestOptions([]);
        setWorkspaceBasePullRequestMessage(null);
        setIsLoadingWorkspaceBasePullRequests(false);
        return;
      }

      const requestId = ++workspaceBasePullRequestRequestRef.current;
      setIsLoadingWorkspaceBasePullRequests(true);
      setWorkspaceBasePullRequestMessage(null);

      void loadPullRequestBaseOptions({ projectId: activeProjectId, query })
        .then((options) => {
          if (workspaceBasePullRequestRequestRef.current !== requestId) {
            return;
          }
          setWorkspaceBasePullRequestOptions((current) => {
            const selected = current.find(
              (option) => option.key === workspaceBasePickerValue,
            );
            if (
              selected &&
              !options.some((option) => option.key === selected.key)
            ) {
              return [selected, ...options];
            }
            return options;
          });
          setIsLoadingWorkspaceBasePullRequests(false);
        })
        .catch((error) => {
          if (workspaceBasePullRequestRequestRef.current !== requestId) {
            return;
          }
          setWorkspaceBasePullRequestOptions((current) =>
            current.filter((option) => option.key === workspaceBasePickerValue),
          );
          setWorkspaceBasePullRequestMessage(
            error instanceof Error
              ? error.message
              : "Unable to load pull requests",
          );
          setIsLoadingWorkspaceBasePullRequests(false);
        });
    },
    [activeProjectId, workspaceBasePickerValue, workspaceBaseSelectorAvailable],
  );
  const handleWorkspaceBaseChange = useCallback(
    (value: string) => {
      if (
        !workspaceBaseSelectorAvailable ||
        !activeWorkspace ||
        isUpdatingComposerWorkspaceBase ||
        value === workspaceBasePickerValue
      ) {
        return;
      }
      const selectedOption = workspaceBaseSelectionOptions.find(
        (option: BranchBaseOption) => option.key === value,
      );
      if (!selectedOption) {
        toast.error("Select a base branch before rebasing");
        return;
      }

      void confirm({
        title: "Rebase workspace?",
        description: `This will rebase ${activeWorkspace.branchName} onto ${selectedOption.selection.displayName}. If conflicts are found, RalphX will route this workspace through repair before the conversation continues.`,
        confirmText: "Rebase workspace",
      }).then((confirmed) => {
        if (!confirmed) {
          return;
        }
        runComposerWorkspaceBaseUpdate({
          baseSelection: selectedOption.selection,
          conversationId: activeWorkspace.conversationId,
          detail: `From ${selectedOption.selection.displayName}`,
          kind: "rebase",
          title: "Rebasing branch",
          workspace: activeWorkspace,
        });
      });
    },
    [
      activeWorkspace,
      confirm,
      isUpdatingComposerWorkspaceBase,
      runComposerWorkspaceBaseUpdate,
      workspaceBaseSelectionOptions,
      workspaceBasePickerValue,
      workspaceBaseSelectorAvailable,
    ],
  );
  const workspaceBaseControl = !isFocusedChildChat ? (
    <AgentConversationBaseLine
      className="justify-start"
      workspace={activeWorkspace}
      editable={workspaceBaseSelectorAvailable}
      disabled={!workspaceBaseEditable}
      isLoading={
        workspaceBaseOptionsQuery.isFetching || isUpdatingComposerWorkspaceBase
      }
      options={workspaceBaseOptions}
      pullRequestOptions={workspaceBasePullRequestOptions}
      isLoadingPullRequests={isLoadingWorkspaceBasePullRequests}
      pullRequestMessage={workspaceBasePullRequestMessage}
      prefixLabel="BASE:"
      value={workspaceBasePickerValue}
      onValueChange={handleWorkspaceBaseChange}
      onIntent={handleWorkspaceBasePickerIntent}
      onOpenChange={handleWorkspaceBasePickerOpenChange}
      onPullRequestSearch={searchWorkspaceBasePullRequestOptions}
      {...(activeWorkspaceFreshness
        ? { freshness: activeWorkspaceFreshness }
        : {})}
    />
  ) : null;
  const panelStoreKeyOverride = useMemo(() => {
    if (taskRuntimeFocus) {
      return buildStoreKey(taskRuntimeFocus.contextType, taskRuntimeFocus.taskId);
    }
    if (focusedWorkspaceReviewConversationId) {
      return buildStoreKey("project", focusedWorkspaceReviewConversationId);
    }
    if (focusedChatSessionId) {
      return buildStoreKey("ideation", focusedChatSessionId);
    }
    return getAgentConversationStoreKey(activeConversation);
  }, [
    activeConversation,
    focusedChatSessionId,
    focusedWorkspaceReviewConversationId,
    taskRuntimeFocus,
  ]);
  const queuedMessagesSelector = useMemo(
    () => selectQueuedMessages(panelStoreKeyOverride),
    [panelStoreKeyOverride]
  );
  const panelConversationIdOverride =
    focusedWorkspaceReviewConversationId ??
    (!isFocusedChildChat ? selectedConversationId : null);
  const panelAgentProcessContextIdOverride = taskRuntimeFocus
    ? taskRuntimeFocus.taskId
    : focusedWorkspaceReviewConversationId ??
      (!isFocusedChildChat && activeConversation.contextType === "project"
        ? selectedConversationId
        : null);
  const panelSendConversationId =
    focusedWorkspaceReviewConversationId ??
    (!isFocusedChildChat ? selectedConversationId : null);
  const queuedMessages = useChatStore(queuedMessagesSelector);
  const executionHaltState = useUiStore((s) =>
    getAgentQueueHaltState(s.executionStatus)
  );
  const queuedInitialPrompt = queuedMessages[0]?.content ?? null;
  const emptyState = useMemo(
    () =>
      executionHaltState && queuedInitialPrompt ? (
        <AgentsPausedQueuedEmptyState
          haltState={executionHaltState}
          prompt={queuedInitialPrompt}
        />
      ) : (
        <div />
      ),
    [executionHaltState, queuedInitialPrompt]
  );

  const composerChatFocus = useMemo<ChatFocusFieldConfig | undefined>(() => {
    if (chatFocusOptions.length <= 1) return undefined;
    const focusToneStyles: Record<
      "accent" | "warning",
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
    return {
      value: chatFocus.type,
      onValueChange: (id) => onSelectChatFocus(id as AgentsChatFocusType),
      options: chatFocusOptions.map((option) => {
        const tone = option.tone ? focusToneStyles[option.tone] : null;
        const icon =
          option.type === "workspace"
            ? MessageSquare
            : option.type === "task_runtime"
            ? Play
            : option.tone === "accent"
            ? Lightbulb
            : option.tone === "warning"
            ? ShieldCheck
            : undefined;
        return {
          id: option.type,
          label: option.label,
          ...(option.description !== undefined ? { description: option.description } : {}),
          ...(icon ? { icon } : {}),
          ...(tone
            ? {
                toneColor: tone.color,
                toneBackground: tone.background,
                toneBorder: tone.border,
              }
            : {}),
        };
      }),
      testId: "agents-composer-chat-focus",
    };
  }, [chatFocus.type, chatFocusOptions, onSelectChatFocus]);
  const handleViewRuntimeWorkspace = useCallback(() => {
    onSelectChatFocus("workspace");
  }, [onSelectChatFocus]);
  const handleViewRuntimeTask = useCallback(
    (taskId: string, contextType: AgentTaskRuntimeContextType) => {
      onFocusTaskRuntime(taskId, contextType);
      onOpenTaskArtifact(taskId);
    },
    [onFocusTaskRuntime, onOpenTaskArtifact],
  );
  const handleViewRuntimeWorkspaceReview = useCallback(
    (conversationId: string) => {
      onFocusWorkspaceReview(conversationId);
    },
    [onFocusWorkspaceReview],
  );
  const composerTaskLedgerContext = useMemo(() => {
    if (taskRuntimeFocus) {
      return {
        contextType: taskRuntimeFocus.contextType,
        contextId: taskRuntimeFocus.taskId,
      };
    }
    if (!isFocusedChildChat) {
      return {
        contextType: "conversation",
        contextId: selectedConversationId,
      };
    }
    return null;
  }, [
    isFocusedChildChat,
    selectedConversationId,
    taskRuntimeFocus,
  ]);
  const workspaceModelOptions = useMemo(
    () =>
      agentModelOptions(
        selectableWorkspaceRuntime.provider,
        modelRegistry,
        workspaceProviderSupportedModelAliases,
      ),
    [
      modelRegistry,
      selectableWorkspaceRuntime.provider,
      workspaceProviderSupportedModelAliases,
    ]
  );
  const workspaceEffortOptions = useMemo(
    () =>
      agentEffortOptions(
        selectableWorkspaceRuntime.provider,
        selectableWorkspaceRuntime.modelId,
        modelRegistry,
        workspaceProviderSupportedEfforts
      ),
    [
      modelRegistry,
      selectableWorkspaceRuntime.modelId,
      selectableWorkspaceRuntime.provider,
      workspaceProviderSupportedEfforts,
    ]
  );
  const modeOptions = useMemo(() => {
    if (!activeConversationModeLocked) {
      return AGENT_CONVERSATION_MODE_OPTIONS;
    }
    const lockReason =
      activeWorkspace?.modeSwitchLockReason ??
      "Active ideation or execution state owns this workspace.";
    return AGENT_CONVERSATION_MODE_OPTIONS.map((option) =>
      option.id === activeConversationMode || option.id === "ideation"
        ? option
        : {
            ...option,
            disabled: true,
            disabledReason: lockReason,
          },
    );
  }, [
    activeConversationMode,
    activeConversationModeLocked,
    activeWorkspace?.modeSwitchLockReason,
  ]);
  const isPlanWorkspaceComposer =
    !isFocusedChildChat &&
    activeConversationMode === "plan" &&
    activeWorkspace?.mode === "plan";
  const canShowPlanComposerViewPrompt =
    isPlanWorkspaceComposer && availableArtifactTabs.includes("plan");
  const { artifactState: resolvedArtifactState, artifactPaneOpen } =
    useResolvedAgentArtifactState(
      isPlanWorkspaceComposer ? selectedConversationId : null,
      hasAutoOpenArtifacts,
    );
  const isPlanArtifactVisible =
    isPlanWorkspaceComposer &&
    artifactPaneOpen &&
    resolvedArtifactState.activeTab === "plan";

  const canUsePlanComposerActions =
    !isFocusedChildChat &&
    activeConversationMode === "plan" &&
    activeWorkspace?.mode === "plan" &&
    isPlanArtifactVisible;
  const planApprovalSessionId = canUsePlanComposerActions
    ? attachedIdeationSessionId
    : null;
  const additionalQuestionSessionIds = useMemo(() => {
    if (isFocusedChildChat || activeConversation.contextType !== "project") {
      return undefined;
    }
    if (activeConversationMode === "plan") {
      return attachedIdeationSessionId ? [attachedIdeationSessionId] : undefined;
    }
    return [selectedConversationId];
  }, [
    activeConversation.contextType,
    activeConversationMode,
    attachedIdeationSessionId,
    isFocusedChildChat,
    selectedConversationId,
  ]);
  const reviewPrContextQuery = useQuery({
    queryKey: agentWorkspaceKeys.prReview(activeWorkspace?.conversationId),
    queryFn: () =>
      chatApi.getAgentWorkspacePrReviewContext(activeWorkspace!.conversationId),
    enabled: Boolean(
      !isFocusedChildChat &&
        activeConversation.contextType === "project" &&
        activeWorkspace?.conversationId &&
        activeWorkspace.mode === "review_pr",
    ),
    staleTime: 5_000,
    refetchInterval: (query) =>
      prReviewContextForConversation(
        query.state.data,
        activeWorkspace?.conversationId,
      )?.pendingAction
        ? false
        : 5_000,
  });
  const reviewPrContext = prReviewContextForConversation(
    reviewPrContextQuery.data,
    activeWorkspace?.conversationId,
  );
  const planApprovalQuery = useQuery({
    queryKey: ["agents", "plan-approval", planApprovalSessionId],
    queryFn: () => artifactApi.getSessionPlan(planApprovalSessionId!),
    enabled: !!planApprovalSessionId,
    staleTime: 5_000,
  });
  const planApprovalArtifact = planApprovalQuery.data ?? null;
  const planArtifactApprovalStatus = planApprovalArtifact
    ? planApprovalArtifact.planApproval?.status ?? "draft"
    : null;
  const isPlanApproved = planArtifactApprovalStatus === "approved";
  const canApproveComposerPlan =
    !!planApprovalSessionId &&
    !!planApprovalArtifact &&
    planArtifactApprovalStatus === "draft";
  const canCreatePlanProposals = !!planApprovalSessionId && isPlanApproved;
  const canImplementPlanDirectly = Boolean(
    planApprovalSessionId &&
      isPlanApproved &&
      activeWorkspace?.conversationId &&
      activeProjectId,
  );
  const planComplexityQuery = useQuery({
    queryKey: [
      "agents",
      "plan-complexity",
      planApprovalSessionId,
      planApprovalArtifact?.id,
      planApprovalArtifact?.metadata.version,
    ],
    queryFn: () => artifactApi.getPlanComplexityAssessment(planApprovalSessionId!),
    enabled: Boolean(
      planApprovalSessionId &&
        planApprovalArtifact?.id &&
        isPlanApproved,
    ),
    staleTime: 5_000,
    refetchInterval: (query) => (query.state.data ? false : 4_000),
  });
  const isPlanRecommendationPending = isPlanRecommendationCheckPending({
    assessment: planComplexityQuery.data,
    isFetching:
      (planComplexityQuery.isFetching || planComplexityQuery.isLoading) &&
      !planComplexityQuery.data,
    approvedAt: planApprovalArtifact?.planApproval?.approvedAt,
  });
  const planVerificationQuery = useVerificationStatus(
    planApprovalSessionId && planApprovalArtifact ? planApprovalSessionId : undefined,
  );
  const planVerificationState = planVerificationQuery.data?.status ?? null;
  const planVerificationInProgress =
    planVerificationQuery.data?.inProgress ?? false;
  const isPlanVerificationLoading =
    (planVerificationQuery.isLoading || planVerificationQuery.isFetching) &&
    !planVerificationQuery.data;
  const isPlanVerificationSatisfied =
    planVerificationState === "verified" ||
    planVerificationState === "imported_verified";
  const canVerifyComposerPlan = Boolean(
    planApprovalSessionId &&
      planApprovalArtifact &&
      !isPlanVerificationLoading &&
      !isPlanVerificationSatisfied,
  );

  const handleApprovePlanFromQuestion = useCallback(async () => {
    if (!planApprovalSessionId || !planApprovalArtifact || !canApproveComposerPlan) {
      return;
    }
    setIsApprovingPlan(true);
    try {
      const approved = await artifactApi.approvePlanArtifact({
        sessionId: planApprovalSessionId,
        artifactId: planApprovalArtifact.id,
      });
      queryClient.setQueryData(
        ["agents", "plan-approval", planApprovalSessionId],
        approved,
      );
      queryClient.setQueryData(
        ["agents", "session-plan", planApprovalSessionId, approved.id],
        approved,
      );
      await queryClient.invalidateQueries({
        queryKey: ["agents", "plan-complexity", planApprovalSessionId],
      });
      toast.success("Plan approved");
    } catch (err) {
      console.error("Failed to approve plan:", err);
      toast.error(err instanceof Error ? err.message : "Failed to approve plan");
    } finally {
      setIsApprovingPlan(false);
    }
  }, [
    canApproveComposerPlan,
    planApprovalArtifact,
    planApprovalSessionId,
    queryClient,
  ]);
  const handleCreatePlanProposals = useCallback(async () => {
    if (!planApprovalSessionId || !canCreatePlanProposals) {
      return;
    }
    setIsCreatingPlanProposals(true);
    try {
      const shouldPromoteWorkspace =
        activeWorkspace?.mode !== "ideation" &&
        activeWorkspace?.linkedIdeationSessionId === planApprovalSessionId &&
        Boolean(activeWorkspace?.conversationId);

      if (shouldPromoteWorkspace && activeWorkspace?.conversationId) {
        const result = await chatApi.switchAgentConversationMode({
          conversationId: activeWorkspace.conversationId,
          mode: "ideation",
        });
        if (result.workspace) {
          queryClient.setQueryData(
            agentWorkspaceKeys.workspace(activeWorkspace.conversationId),
            result.workspace,
          );
        }
        onConversationModeSwitched(
          activeWorkspace.conversationId,
          "ideation",
          result.workspace ?? null,
        );
        void invalidateWorkspaceQueries(
          queryClient,
          activeWorkspace.conversationId,
        );
      } else if (
        activeWorkspace?.conversationId &&
        activeWorkspace.linkedIdeationSessionId === planApprovalSessionId
      ) {
        onConversationModeSwitched(
          activeWorkspace.conversationId,
          "ideation",
          activeWorkspace,
        );
      }

      await chatApi.sendAgentMessage(
        "ideation",
        planApprovalSessionId,
        PLAN_TO_PROPOSALS_REQUEST,
      );
      toast.success("Proposal creation requested");
    } catch (err) {
      console.error("Failed to create proposals:", err);
      toast.error("Failed to request proposal creation");
    } finally {
      setIsCreatingPlanProposals(false);
    }
  }, [
    activeWorkspace,
    canCreatePlanProposals,
    onConversationModeSwitched,
    planApprovalSessionId,
    queryClient,
  ]);
  const handleImplementPlanDirectly = useCallback(async () => {
    if (
      !planApprovalSessionId ||
      !activeWorkspace?.conversationId ||
      !canImplementPlanDirectly
    ) {
      return;
    }
    setIsImplementingPlanDirectly(true);
    try {
      if (activeWorkspace.mode !== "edit") {
        const result = await chatApi.switchAgentConversationMode({
          conversationId: activeWorkspace.conversationId,
          mode: "edit",
        });
        if (result.workspace) {
          queryClient.setQueryData(
            agentWorkspaceKeys.workspace(activeWorkspace.conversationId),
            result.workspace,
          );
        }
        onConversationModeSwitched(
          activeWorkspace.conversationId,
          "edit",
          result.workspace ?? null,
        );
        void invalidateWorkspaceQueries(
          queryClient,
          activeWorkspace.conversationId,
        );
      } else {
        onConversationModeSwitched(
          activeWorkspace.conversationId,
          "edit",
          activeWorkspace,
        );
      }

      await chatApi.sendAgentMessage(
        "project",
        activeProjectId,
        PLAN_IMPLEMENT_DIRECTLY_REQUEST,
        undefined,
        undefined,
        {
          conversationId: activeWorkspace.conversationId,
          providerHarness: normalizedActiveRuntime.provider,
          modelId: normalizedActiveRuntime.modelId,
          logicalEffort: normalizedActiveRuntime.effort,
          codexFastMode: activeCodexFastModeOption,
          suppressUserMessage: true,
        },
      );
      toast.success("Implementation started");
    } catch (err) {
      console.error("Failed to implement plan directly:", err);
      toast.error(err instanceof Error ? err.message : "Failed to start implementation");
    } finally {
      setIsImplementingPlanDirectly(false);
    }
  }, [
    activeProjectId,
    activeWorkspace,
    activeCodexFastModeOption,
    canImplementPlanDirectly,
    normalizedActiveRuntime.effort,
    normalizedActiveRuntime.modelId,
    normalizedActiveRuntime.provider,
    onConversationModeSwitched,
    planApprovalSessionId,
    queryClient,
  ]);
  const handleVerifyPlanFromComposer = useCallback(async () => {
    if (
      !planApprovalSessionId ||
      !canVerifyComposerPlan ||
      planVerificationInProgress
    ) {
      return;
    }
    setIsStartingPlanVerification(true);
    try {
      let disabledSpecialists: string[] = [];
      try {
        const specialists = await verificationApi.getSpecialists();
        disabledSpecialists = specialists.specialists
          .filter((specialist) => !specialist.enabled_by_default)
          .map((specialist) => specialist.name);
      } catch (err) {
        console.warn("Failed to load verification specialists:", err);
      }

      await verificationApi.confirm(planApprovalSessionId, disabledSpecialists);
      await Promise.all([
        queryClient.invalidateQueries({
          queryKey: verificationStatusKey(planApprovalSessionId),
        }),
        queryClient.invalidateQueries({
          queryKey: ideationKeys.sessionWithData(planApprovalSessionId),
        }),
        queryClient.invalidateQueries({ queryKey: ideationKeys.sessions() }),
      ]);
      onSelectArtifact("verification");
      toast.success("Plan verification started");
    } catch (err) {
      console.error("Failed to start plan verification:", err);
      toast.error(
        err instanceof Error ? err.message : "Failed to start plan verification",
      );
    } finally {
      setIsStartingPlanVerification(false);
    }
  }, [
    canVerifyComposerPlan,
    onSelectArtifact,
    planApprovalSessionId,
    planVerificationInProgress,
    queryClient,
  ]);
  const planComposerHint = useMemo(() => {
    if (canShowPlanComposerViewPrompt && !isPlanArtifactVisible) {
      return "Open the plan to review it before taking action.";
    }
    if (canApproveComposerPlan) {
      return "Approve the draft plan when it matches the intended scope, or verify it first for adversarial review.";
    }
    return buildPlanActionHint({
      assessment: planComplexityQuery.data,
      isAssessing: isPlanRecommendationPending,
      canChoose: canCreatePlanProposals && canImplementPlanDirectly,
    });
  }, [
    canShowPlanComposerViewPrompt,
    canApproveComposerPlan,
    canCreatePlanProposals,
    canImplementPlanDirectly,
    isPlanArtifactVisible,
    isPlanRecommendationPending,
    planComplexityQuery.data,
  ]);
  const planComposerCtaActions = useMemo<PlanComposerCtaAction[]>(() => {
    if (!planApprovalSessionId || !planApprovalArtifact) {
      return [];
    }

    const verifyAction: PlanComposerCtaAction | null = canVerifyComposerPlan
      ? {
          id: "verify",
          label: "Verify Plan",
          icon: ShieldCheck,
          isPrimary: false,
          isPending:
            isStartingPlanVerification || planVerificationInProgress,
          disabled: isPlanRecommendationPending,
          onClick: () => {
            void handleVerifyPlanFromComposer();
          },
        }
      : null;

    if (canApproveComposerPlan) {
      return [
        {
          id: "approve",
          label: "Approve Plan",
          icon: CheckCircle2,
          isPrimary: true,
          isPending: isApprovingPlan,
          disabled: false,
          onClick: () => {
            void handleApprovePlanFromQuestion();
          },
        },
        verifyAction,
      ].filter((action): action is PlanComposerCtaAction => action !== null);
    }

    if (!isPlanApproved) {
      return [];
    }

    if (isCreatingPlanProposals || isImplementingPlanDirectly) {
      return [];
    }

    if (activeWorkspaceFreshness?.hasUncommittedChanges === true) {
      return [];
    }

    const implementationAction: PlanComposerCtaAction | null =
      canImplementPlanDirectly
        ? {
            id: "implement-directly",
            label: "Implement Directly",
            icon: Play,
            isPrimary:
              !isPlanRecommendationPending &&
              planComplexityQuery.data?.recommendedAction !==
              "create_proposals",
            isPending: isImplementingPlanDirectly,
            disabled: isPlanRecommendationPending,
            onClick: () => {
              void handleImplementPlanDirectly();
            },
          }
        : null;
    const proposalsAction: PlanComposerCtaAction | null = canCreatePlanProposals
      ? {
          id: "create-proposals",
          label: "Create Proposals",
          icon: GitPullRequestArrow,
          isPrimary:
            !isPlanRecommendationPending &&
            planComplexityQuery.data?.recommendedAction ===
            "create_proposals",
          isPending: isCreatingPlanProposals,
          disabled: isPlanRecommendationPending,
          onClick: () => {
            void handleCreatePlanProposals();
          },
        }
      : null;
    const mainActions =
      isPlanRecommendationPending ||
      planComplexityQuery.data?.recommendedAction === "create_proposals"
        ? [proposalsAction, implementationAction]
        : [implementationAction, proposalsAction];
    return [...mainActions, verifyAction].filter(
      (action): action is PlanComposerCtaAction => action !== null,
    );
  }, [
    canApproveComposerPlan,
    canCreatePlanProposals,
    canImplementPlanDirectly,
    canVerifyComposerPlan,
    activeWorkspaceFreshness?.hasUncommittedChanges,
    handleApprovePlanFromQuestion,
    handleCreatePlanProposals,
    handleImplementPlanDirectly,
    handleVerifyPlanFromComposer,
    isApprovingPlan,
    isCreatingPlanProposals,
    isImplementingPlanDirectly,
    isPlanApproved,
    isPlanRecommendationPending,
    isStartingPlanVerification,
    planApprovalArtifact,
    planApprovalSessionId,
    planComplexityQuery.data?.recommendedAction,
    planVerificationInProgress,
  ]);
  const planComposerViewPlanAction = useMemo<
    PlanComposerViewPlanAction | undefined
  >(() => {
    if (!canShowPlanComposerViewPrompt || isPlanArtifactVisible) {
      return undefined;
    }
    return {
      available: true,
      conversationId: selectedConversationId,
      hasAutoOpenArtifacts,
      isPlanVisible: isPlanArtifactVisible,
      onClick: onOpenPlanArtifact,
    };
  }, [
    canShowPlanComposerViewPrompt,
    hasAutoOpenArtifacts,
    isPlanArtifactVisible,
    onOpenPlanArtifact,
    selectedConversationId,
  ]);
  const planApprovalAction = useMemo(() => {
    if (!canApproveComposerPlan) {
      return undefined;
    }
    return {
      label: "Approve Plan",
      onClick: () => {
        void handleApprovePlanFromQuestion();
      },
      disabled: isApprovingPlan,
      isPending: isApprovingPlan,
    };
  }, [
    canApproveComposerPlan,
    handleApprovePlanFromQuestion,
    isApprovingPlan,
  ]);

  const continuePlanModeConversation = useCallback(
    async (
      conversationId: string,
      message: string | null | undefined,
    ): Promise<boolean> => {
      const trimmedMessage = message?.trim();
      if (!trimmedMessage) {
        return true;
      }

      try {
        const sendResult = await chatApi.sendAgentMessage(
          "project",
          activeProjectId,
          trimmedMessage,
          undefined,
          undefined,
          {
            conversationId,
            providerHarness: normalizedActiveRuntime.provider,
            modelId: normalizedActiveRuntime.modelId,
            logicalEffort: normalizedActiveRuntime.effort,
            codexFastMode: activeCodexFastModeOption,
          },
        );
        onAgentUserMessageSent({
          content: trimmedMessage,
          result: sendResult,
        });
        return true;
      } catch (err) {
        console.error("Failed to continue in Plan mode:", err);
        toast.error(
          err instanceof Error
            ? err.message
            : "Switched to Plan mode, but failed to continue automatically",
        );
        return false;
      }
    },
    [
      activeCodexFastModeOption,
      activeProjectId,
      normalizedActiveRuntime,
      onAgentUserMessageSent,
    ],
  );

  const switchConversationToPlanMode = useCallback(
    async (
      conversationId: string,
      options?: {
        deferIfRunning?: boolean;
        showDeferredToast?: boolean;
        autoContinueMessage?: string | null;
      },
    ): Promise<boolean> => {
      try {
        const result = await chatApi.switchAgentConversationMode({
          conversationId,
          mode: "plan",
        });
        if (result.workspace) {
          queryClient.setQueryData(
            agentWorkspaceKeys.workspace(conversationId),
            result.workspace,
          );
        }
        onConversationModeSwitched(
          conversationId,
          "plan",
          result.workspace ?? null,
        );
        const autoContinueMessage =
          options?.autoContinueMessage ??
          (pendingPlanModeSwitchConversationIdRef.current === conversationId
            ? pendingPlanModeSwitchAutoContinueMessageRef.current
            : null);
        if (pendingPlanModeSwitchConversationIdRef.current === conversationId) {
          pendingPlanModeSwitchConversationIdRef.current = null;
          pendingPlanModeSwitchAutoContinueMessageRef.current = null;
          pendingPlanModeSwitchRetryCountRef.current = 0;
          setPendingPlanModeSwitch(null);
        }
        void invalidateWorkspaceQueries(queryClient, conversationId);
        const continued = await continuePlanModeConversation(
          conversationId,
          autoContinueMessage,
        );
        if (continued) {
          toast.success(
            autoContinueMessage
              ? "Continuing in Plan mode"
              : "Switched to Plan mode",
          );
        }
        return true;
      } catch (err) {
        if (options?.deferIfRunning && isRunningModeSwitchError(err)) {
          const isSamePendingConversation =
            pendingPlanModeSwitchConversationIdRef.current === conversationId;
          const nextAttempt = isSamePendingConversation
            ? pendingPlanModeSwitchRetryCountRef.current + 1
            : 1;

          if (nextAttempt > PLAN_MODE_SWITCH_MAX_RETRY_ATTEMPTS) {
            pendingPlanModeSwitchConversationIdRef.current = null;
            pendingPlanModeSwitchAutoContinueMessageRef.current = null;
            pendingPlanModeSwitchRetryCountRef.current = 0;
            setPendingPlanModeSwitch(null);
            toast.error(
              "Could not switch to Plan mode after the agent turn finished. Try switching modes manually.",
            );
            return false;
          }

          pendingPlanModeSwitchConversationIdRef.current = conversationId;
          pendingPlanModeSwitchAutoContinueMessageRef.current =
            options.autoContinueMessage ??
            pendingPlanModeSwitchAutoContinueMessageRef.current;
          pendingPlanModeSwitchRetryCountRef.current = nextAttempt;
          setPendingPlanModeSwitch({
            conversationId,
            attempt: nextAttempt,
            autoContinueMessage:
              pendingPlanModeSwitchAutoContinueMessageRef.current,
          });
          if (options.showDeferredToast !== false) {
            toast.info("Will switch to Plan mode when this agent turn finishes.");
          }
          return false;
        }

        console.error("Failed to switch to Plan mode:", err);
        toast.error(
          err instanceof Error ? err.message : "Failed to switch to Plan mode",
        );
        return false;
      }
    },
    [continuePlanModeConversation, onConversationModeSwitched, queryClient],
  );

  useEffect(() => {
    if (!pendingPlanModeSwitch) {
      return;
    }

    const conversationId = pendingPlanModeSwitch.conversationId;
    let eventRetryTimer: number | undefined;
    let fallbackRetryTimer: number | undefined;
    const retryAfterCompletedRun = (payload: AgentRunCompletedPayload) => {
      if (
        payload.conversation_id !== conversationId ||
        payload.teammate_name
      ) {
        return;
      }
      if (eventRetryTimer !== undefined) {
        return;
      }

      eventRetryTimer = window.setTimeout(() => {
        eventRetryTimer = undefined;
        void switchConversationToPlanMode(conversationId, {
          deferIfRunning: true,
          showDeferredToast: false,
        });
      }, PLAN_MODE_SWITCH_EVENT_RETRY_DELAY_MS);
    };
    fallbackRetryTimer = window.setTimeout(() => {
      fallbackRetryTimer = undefined;
      void switchConversationToPlanMode(conversationId, {
        deferIfRunning: true,
        showDeferredToast: false,
      });
    }, PLAN_MODE_SWITCH_FALLBACK_RETRY_DELAY_MS);

    const unsubscribeRunCompleted = bus.subscribe<AgentRunCompletedPayload>(
      "agent:run_completed",
      retryAfterCompletedRun,
    );
    const unsubscribeTurnCompleted = bus.subscribe<AgentRunCompletedPayload>(
      "agent:turn_completed",
      retryAfterCompletedRun,
    );

    return () => {
      unsubscribeRunCompleted();
      unsubscribeTurnCompleted();
      if (eventRetryTimer !== undefined) {
        window.clearTimeout(eventRetryTimer);
      }
      if (fallbackRetryTimer !== undefined) {
        window.clearTimeout(fallbackRetryTimer);
      }
    };
  }, [bus, pendingPlanModeSwitch, switchConversationToPlanMode]);

  const handleQuestionAnswered = useCallback(
    async (
      question: AskUserQuestionPayload,
      response: AskUserQuestionResponse,
      result?: SubmitQuestionAnswerResult,
    ) => {
      const proposalConversationId = getPlanModeProposalConversationId(question);
      if (!proposalConversationId || !acceptsPlanModeProposal(response)) {
        return;
      }

      if (result?.planModeProposalHandled) {
        return;
      }

      if (
        isFocusedChildChat ||
        activeConversation.contextType !== "project" ||
        proposalConversationId !== selectedConversationId
      ) {
        return;
      }

      const autoContinueMessage =
        buildPlanModeProposalContinuationMessage(question);

      if (activeConversationMode === "plan") {
        onConversationModeSwitched(
          selectedConversationId,
          "plan",
          activeWorkspace,
        );
        await continuePlanModeConversation(
          selectedConversationId,
          autoContinueMessage,
        );
        return;
      }

      if (activeConversationModeLocked) {
        toast.error(
          activeWorkspace?.modeSwitchLockReason ??
            "This conversation cannot switch modes while the workspace is busy.",
        );
        return;
      }

      await switchConversationToPlanMode(selectedConversationId, {
        deferIfRunning: true,
        autoContinueMessage,
      });
    },
    [
      activeConversation.contextType,
      activeConversationMode,
      activeConversationModeLocked,
      activeWorkspace,
      continuePlanModeConversation,
      isFocusedChildChat,
      onConversationModeSwitched,
      selectedConversationId,
      switchConversationToPlanMode,
    ],
  );

  return (
    <div
      className="flex-1 h-full flex flex-col"
      style={{ minWidth: AGENTS_CHAT_MIN_WIDTH }}
      data-testid="agents-active-conversation-panel"
    >
      <div className="min-h-0 flex-1">
        <AgentWorkspaceFileLinkProvider
          conversationId={selectedConversationId}
          workspace={
            chatFocus.type === "workspace_review"
              ? activeWorkspace
              : isFocusedChildChat
                ? null
                : activeWorkspace
          }
        >
          <IntegratedChatPanel
            key={`${selectedConversationId}:${chatFocus.type}:${focusedPanelKey}`}
            projectId={activeProjectId}
            {...(panelIdeationSessionId
              ? { ideationSessionId: panelIdeationSessionId }
              : {})}
            {...(panelConversationIdOverride
              ? { conversationIdOverride: panelConversationIdOverride }
              : {})}
            selectedTaskIdOverride={panelSelectedTaskId}
            {...(panelTaskRuntimeContextType
              ? { taskRuntimeContextTypeOverride: panelTaskRuntimeContextType }
              : {})}
            storeContextKeyOverride={panelStoreKeyOverride}
            {...(panelAgentProcessContextIdOverride
              ? {
                  agentProcessContextIdOverride:
                    panelAgentProcessContextIdOverride,
                }
              : {})}
            {...(panelSendConversationId
              ? {
                  sendOptions: {
                    conversationId: panelSendConversationId,
                    providerHarness: normalizedActiveRuntime.provider,
                    modelId: normalizedActiveRuntime.modelId,
                    logicalEffort: normalizedActiveRuntime.effort,
                    codexFastMode: activeCodexFastModeOption,
                  },
                }
              : {})}
            onUserMessageSent={onAgentUserMessageSent}
            onQuestionAnswered={handleQuestionAnswered}
            onChildSessionNavigate={onFocusIdeationSession}
            belowTranscriptLayoutOwnedByVisibleRuntime={
              belowTranscriptLayoutOwnedByVisibleRuntime
            }
            hideHeaderSessionControls
            hideSessionToolbar
            surfaceBackground="transparent"
            contentWidthClassName={AGENTS_CHAT_CONTENT_WIDTH_CLASS}
            {...{
              inputContainerClassName:
                "shrink-0 bg-transparent px-4 pb-4 pt-3",
              renderComposer: (composerProps: IntegratedChatComposerRenderProps) => {
              const runForkCommand = async (
                followup: string,
                options?: AgentComposerSendOptions,
              ) => {
                const confirmed = await confirm({
                  title: "Fork session?",
                  description:
                    "Create a new agent conversation copied from this one. The original conversation will stay unchanged.",
                  confirmText: "Fork session",
                });
                if (!confirmed) {
                  return;
                }
                setIsForkingConversation(true);
                try {
                  const forkResult = await onForkConversation(selectedConversationId);
                  const trimmedFollowup = followup.trim();
                  if (trimmedFollowup) {
                    const sendResult = await chatApi.sendAgentMessage(
                      "project",
                      activeProjectId,
                      trimmedFollowup,
                      undefined,
                      undefined,
                      {
                        conversationId: forkResult.conversation.id,
                        providerHarness: normalizedActiveRuntime.provider,
                        modelId: normalizedActiveRuntime.modelId,
                        logicalEffort: normalizedActiveRuntime.effort,
                        codexFastMode: activeCodexFastModeOption,
                        ...(options?.projectReferences?.length
                          ? { composerProjectReferences: options.projectReferences }
                          : {}),
                        ...(options?.integrationReferences?.length
                          ? {
                              composerIntegrationReferences:
                                options.integrationReferences,
                            }
                          : {}),
                        ...(options?.artifactReferences?.length
                          ? {
                              composerArtifactReferences:
                                options.artifactReferences,
                            }
                          : {}),
                      },
                    );
                    onAgentUserMessageSent({
                      content: trimmedFollowup,
                      result: sendResult,
                      ...(options?.integrationReferences?.length
                        ? { composerIntegrationReferences: options.integrationReferences }
                        : {}),
                    });
                  }
                } finally {
                  setIsForkingConversation(false);
                }
              };
              const handleComposerSend = async (
                message: string,
                options?: AgentComposerSendOptions,
              ) => {
                const forkFollowup = !isFocusedChildChat
                  ? parseForkCommand(message)
                  : null;
                if (forkFollowup == null) {
                  await composerProps.onSend(message, options);
                  return;
                }

                await runForkCommand(forkFollowup, options);
              };
              const shouldShowPlanComposerCta =
                !!planComposerHint && composerProps.questionMode === undefined;
              return (
                <>
                  {!isFocusedChildChat &&
                    activeWorkspace?.mode === "review_pr" &&
                    activeWorkspace.conversationId && (
                      <AgentWorkspacePrReviewCard
                        conversationId={activeWorkspace.conversationId}
                        context={reviewPrContext}
                        isLoading={reviewPrContextQuery.isLoading}
                        isFetching={reviewPrContextQuery.isFetching}
                        error={reviewPrContextQuery.error}
                      />
                    )}
                  <AgentsComposerWorkspaceChangesCard
                    conversationId={selectedConversationId}
                    projectId={activeProjectId}
                    workspace={activeWorkspace}
                    isFocusedChildChat={isFocusedChildChat}
                    currentFocus={chatFocus}
                    taskLedgerContext={composerTaskLedgerContext}
                    isAgentGenerating={composerProps.agentStatus === "generating"}
                    pauseHydration={isComposerHydrationPaused}
                    onViewWorkspace={handleViewRuntimeWorkspace}
                    onViewIdeation={onFocusIdeationSession}
                    onViewVerification={onFocusVerificationSession}
                    onViewWorkspaceReview={handleViewRuntimeWorkspaceReview}
                    onViewTaskRuntime={handleViewRuntimeTask}
                    onOpenFile={onOpenPublishFile}
                    onPreloadPublishPane={onPreloadArtifacts}
                  />
                  {shouldShowPlanComposerCta && (
                    <PlanComposerCtaRow
                      hint={planComposerHint}
                      actions={planComposerCtaActions}
                      viewPlanAction={planComposerViewPlanAction}
                    />
                  )}
                  {automationRunReadOnlyReason && (
                    <div
                      className="flex items-start gap-2 rounded-md px-3 py-2 text-xs"
                      style={{
                        backgroundColor: "var(--bg-surface)",
                        borderColor: "var(--border-default)",
                        borderStyle: "solid",
                        borderWidth: "1px",
                        color: "var(--text-muted)",
                      }}
                      data-testid="agents-automation-run-readonly-banner"
                    >
                      <ShieldCheck
                        className="mt-0.5 h-4 w-4 shrink-0"
                        style={{ color: "var(--accent-primary)" }}
                        aria-hidden="true"
                      />
                      <span>{automationRunReadOnlyReason}</span>
                    </div>
                  )}
                  <AgentComposerSurface
                    dataTestId="agents-conversation-composer"
                    actionTestId="agents-conversation-submit"
                    collapsible
                    onSend={handleComposerSend}
                    onStop={composerProps.onStop}
                    agentStatus={composerProps.agentStatus}
                    isSubmitting={composerProps.isSending || isForkingConversation}
                    isReadOnly={
                      composerProps.isReadOnly ||
                      isForkingConversation ||
                      Boolean(automationRunReadOnlyReason)
                    }
                    autoFocus={composerProps.autoFocus}
                    conversationId={selectedConversationId}
                    {...(!isFocusedChildChat
                      ? {
                          onForkSession: () => runForkCommand(""),
                          forkSessionDisabled: isForkingConversation,
                        }
                      : {})}
                    placeholder={
                      isFocusedChildChat
                        ? "Send a message..."
                        : "Ask the agent to plan, build, debug, or review something"
                    }
                    onFocusChange={(focused) => {
                      if (focused) {
                        markComposerActivity();
                      }
                    }}
                    onLayoutChange={composerProps.onLayoutChange}
                    sendDisabledReason={
                      automationRunReadOnlyReason ??
                      (usesWorkspaceRuntimeControls
                        ? workspaceProviderStatusMessage
                        : null)
                    }
                    hasQueuedMessages={composerProps.hasQueuedMessages}
                    onEditLastQueued={composerProps.onEditLastQueued}
                    attachments={composerProps.attachments}
                    enableAttachments={composerProps.enableAttachments}
                    onFilesSelected={composerProps.onFilesSelected}
                    onRemoveAttachment={composerProps.onRemoveAttachment}
                    attachmentsUploading={composerProps.attachmentsUploading}
                    {...(composerProps.value !== undefined
                      ? {
                          value: composerProps.value,
                          onChange: (value: string) => {
                            markComposerActivity();
                            composerProps.onChange?.(value);
                          },
                        }
                      : {})}
                    {...(composerProps.questionMode !== undefined
                      ? { questionMode: composerProps.questionMode }
                      : {})}
                    submitLabel="Send"
                    {...(activeConversationMode
                      ? {
                          mode: {
                            value: activeConversationMode,
                            onOpen: onActiveConversationModeMenuOpen,
                            onValueChange: (value: string) =>
                              onActiveConversationModeChange(
                                value as AgentConversationWorkspaceMode,
                              ),
                            options: modeOptions,
                            // Workspace conversation owns mode; child chats
                            // inherit and display it read-only.
                            disabled:
                              isFocusedChildChat ||
                              composerProps.isSending ||
                              composerProps.agentStatus === "generating" ||
                              switchingConversationModeId ===
                                selectedConversationId,
                          },
                        }
                      : {})}
                    {...(composerChatFocus ? { chatFocus: composerChatFocus } : {})}
                    slashCommands={
                      !isFocusedChildChat
                        ? [
                            {
                              id: "fork",
                              label: "/fork",
                              description: "Fork this agent conversation",
                              disabled: isForkingConversation,
                              onSelect: () => runForkCommand(""),
                            },
                          ]
                        : []
                    }
                    project={{
                      value: activeProjectId,
                      onValueChange: () => undefined,
                      options: activeProjectOptions,
                      placeholder: "Current project",
                      disabled: true,
                    }}
                    {...(() => {
                      if (usesWorkspaceRuntimeControls) {
                        return {
                          provider: {
                            value: normalizedActiveRuntime.provider,
                            onValueChange: (provider) =>
                              onActiveProviderChange(
                                provider,
                                supportedEffortsForProvider(
                                  providerOptions,
                                  provider,
                                ),
                              ),
                            options:
                              providerOptions.length > 0
                                ? providerOptions
                                : AGENT_PROVIDER_OPTIONS,
                            disabled: !providerSettingsReady,
                            footerAction: (
                              <AgentProviderSettingsButton
                                onClick={openProviderSettings}
                                testId="agents-conversation-provider-settings"
                              />
                            ),
                            compactFooterAction: (
                              <AgentProviderSettingsButton
                                onClick={openProviderSettings}
                                testId="agents-conversation-provider-settings-compact"
                                compact
                              />
                            ),
                          },
                          model: {
                            value: selectableWorkspaceRuntime.modelId,
                            onValueChange: (modelId) =>
                              onActiveModelChange(
                                modelId,
                                workspaceProviderSupportedEfforts,
                                workspaceProviderSupportedModelAliases,
                              ),
                            options: workspaceModelOptions,
                            disabled: Boolean(workspaceProviderStatusMessage),
                            fastMode: {
                              visible:
                                normalizedActiveRuntime.provider === "codex",
                              value: selectableCodexFastMode,
                              onValueChange: handleActiveCodexFastModeChange,
                              disabled:
                                !providerSettingsReady ||
                                !codexFastModeAvailability.supported,
                              description:
                                codexFastModeAvailability.reason ??
                                CODEX_FAST_MODE_DESCRIPTION,
                              testId: "agents-conversation-codex-fast-mode",
                            },
                            onOpenModelSettings: () =>
                              openModal("settings", { section: "models" }),
                          },
                          effort: {
                            value: selectableWorkspaceRuntime.effort,
                            onValueChange: (effort) =>
                              onActiveEffortChange(
                                effort,
                                workspaceProviderSupportedEfforts,
                                workspaceProviderSupportedModelAliases,
                              ),
                            options: workspaceEffortOptions,
                            disabled: Boolean(workspaceProviderStatusMessage),
                            testId: "agents-conversation-effort",
                          },
                        };
                      }
                      // Child chat: use the focused session's actual runtime
                      // straight from the chat panel. We never fall back to the
                      // workspace runtime here because that produced misleading
                      // mismatched displays.
                      const childProvider =
                        (composerProps.providerHarness as
                          | AgentProvider
                          | undefined) ?? undefined;
                      const childModelId = composerProps.effectiveModel?.id;
                      // Fallback provider value satisfies the typed union when
                      // harness is missing; the pill self-hides when labels are empty.
                      const fallbackProvider: AgentProvider = "codex";
                      return {
                        provider: {
                          value: childProvider ?? fallbackProvider,
                          onValueChange: () => undefined,
                          options: childProvider ? AGENT_PROVIDER_OPTIONS : [],
                          disabled: true,
                        },
                        model: {
                          value: childModelId ?? "",
                          onValueChange: () => undefined,
                          options: childProvider
                            ? agentModelOptions(childProvider, modelRegistry)
                            : [],
                          disabled: true,
                        },
                        effort: {
                          value: "",
                          onValueChange: () => undefined,
                          options: [],
                          disabled: true,
                        },
                      };
                    })()}
                  />
                  {usesWorkspaceRuntimeControls && workspaceProviderStatusMessage && (
                    <div
                      className="mx-2 mt-2 flex flex-wrap items-center justify-between gap-2 rounded-md border px-3 py-2 text-[0.8125rem]"
                      style={{
                        color: "var(--text-secondary)",
                        background: "var(--bg-surface)",
                        borderColor: "var(--border-subtle)",
                      }}
                      data-testid="agents-conversation-provider-status"
                    >
                      <span>{workspaceProviderStatusMessage}</span>
                      <button
                        type="button"
                        className="rounded-md px-2 py-1 text-[0.75rem] font-medium"
                        style={{
                          color: "var(--accent-primary)",
                          background: "var(--accent-muted)",
                        }}
                        onClick={openProviderSettings}
                        data-testid="agents-conversation-provider-status-settings"
                      >
                        Open Settings
                      </button>
                    </div>
                  )}
                  <div className="mt-2 flex w-full flex-wrap items-center justify-between gap-2 px-2">
                    <AgentComposerProjectLine
                      value={activeProjectId}
                      onValueChange={() => undefined}
                      options={activeProjectOptions}
                      placeholder="Current project"
                      disabled
                    />
                    <AgentConversationWorkspaceLine
                      workspace={activeWorkspace}
                      {...(activeWorkspaceFreshness
                        ? { freshness: activeWorkspaceFreshness }
                        : {})}
                    />
                  </div>
                </>
              );
              },
            }}
            {...(additionalQuestionSessionIds
              ? { additionalQuestionSessionIds }
              : {})}
            {...(planApprovalAction !== undefined ? { planApprovalAction } : {})}
            headerContent={
              <AgentsChatHeaderController
                conversation={activeConversation}
                workspace={isFocusedChildChat ? null : activeWorkspace}
                chatFocus={chatFocus}
                availableArtifactTabs={availableArtifactTabs}
                modelDisplay={{
                  id: normalizedActiveRuntime.modelId,
                  label: normalizedActiveRuntime.modelId,
                }}
                hasAutoOpenArtifacts={hasAutoOpenArtifacts}
                terminalArchivedReason={
                  isFocusedChildChat ? null : terminalArchivedReason
                }
                terminalUnavailableReason={terminalUnavailableReason}
                onRenameConversation={onRenameConversation}
                onPublishWorkspace={onPublishWorkspace}
                onOpenPublishPane={onOpenPublishPane}
                onPreloadArtifacts={onPreloadArtifacts}
                publishShortcutLabel={publishShortcutLabel}
                publishShortcutWorkspace={
                  promotePublishShortcut ? activeWorkspace : null
                }
                promotePublishShortcut={promotePublishShortcut}
                isPublishingWorkspace={publishingConversationId === selectedConversationId}
                onToggleArtifacts={onToggleArtifacts}
                onSelectArtifact={onSelectArtifact}
                {...(isFocusedChildChat
                  ? { onBackToWorkspaceChat: handleViewRuntimeWorkspace }
                  : {})}
                workspaceControl={workspaceBaseControl}
                showTitle={false}
              />
            }
            emptyState={emptyState}
          />
        </AgentWorkspaceFileLinkProvider>
      </div>
      <AgentsTerminalDockHost
        dock="chat"
        conversationId={selectedConversationId}
        workspace={activeWorkspace}
        terminalArchivedReason={terminalArchivedReason}
        terminalUnavailableReason={terminalUnavailableReason}
        hasAutoOpenArtifacts={hasAutoOpenArtifacts}
        setDockElement={setTerminalChatDockElement}
      />
      <ConfirmationDialog {...confirmationDialogProps} />
    </div>
  );
});

function AgentsPausedQueuedEmptyState({
  haltState,
  prompt,
}: {
  haltState: NonNullable<AgentQueueHaltState>;
  prompt: string;
}) {
  const title =
    haltState === "stopped" ? "Execution is stopped" : "Execution is paused";
  const detail =
    haltState === "stopped"
      ? "This prompt will start when execution starts."
      : "This prompt will start when execution resumes.";
  const promptExcerpt = formatQueuedMessageExcerpt(prompt);

  return (
    <div
      data-testid="agents-paused-queued-empty-state"
      className="flex h-full w-full items-center justify-center p-6"
    >
      <div className="w-full max-w-[360px] text-center">
        <div
          className="mx-auto mb-4 flex h-12 w-12 items-center justify-center rounded-md border"
          style={{
            backgroundColor: "var(--status-warning-muted)",
            borderColor: "var(--status-warning-border)",
            borderStyle: "solid",
            borderWidth: "1px",
            color: "var(--status-warning)",
          }}
        >
          <Clock className="h-5 w-5" />
        </div>
        <h3
          className="text-base font-semibold tracking-tight"
          style={{ color: "var(--text-primary)" }}
        >
          {title}
        </h3>
        <p
          className="mt-2 text-sm leading-relaxed"
          style={{ color: "var(--text-secondary)" }}
        >
          {detail}
        </p>
        <div
          className="mt-5 rounded-md border px-3 py-2.5 text-left"
          style={{
            backgroundColor: "var(--bg-surface)",
            borderColor: "var(--border-subtle)",
            borderStyle: "solid",
            borderWidth: "1px",
          }}
        >
          <p
            className="text-[11px] font-medium uppercase tracking-[0.12em]"
            style={{ color: "var(--text-muted)" }}
          >
            Queued prompt
          </p>
          <p
            data-testid="agents-paused-queued-prompt"
            className="mt-1 line-clamp-4 text-sm leading-relaxed"
            style={{ color: "var(--text-secondary)" }}
          >
            {promptExcerpt}
          </p>
        </div>
      </div>
    </div>
  );
}
