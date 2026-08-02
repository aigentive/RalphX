import {
  memo,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
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
  CapabilityIntent,
  ComposerIntegrationReference,
  ForkAgentConversationResult,
} from "@/api/chat";
import { chatApi } from "@/api/chat";
import { manualRoleDefaultsApi } from "@/api/manual-role-defaults";
import { artifactApi } from "@/api/artifact";
import {
  automationsApi,
  type Automation,
  type AutomationRun,
} from "@/api/automations";
import {
  getAutomationRunView,
  isAutomationRunComposerReadOnly,
} from "@/components/automations/automationStage";
import { AutomationRunStatusHeader } from "@/components/automations/AutomationRunStatusHeader";
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
import { useAgentGate } from "@/hooks/useAgentGate";
import { useConfirmation } from "@/hooks/useConfirmation";
import { useHarnessProviders } from "@/hooks/useHarnessProviders";
import { useFeatureFlags } from "@/hooks/useFeatureFlags";
import { useConversationRoleDefault } from "@/hooks/useManualRoleDefaults";
import {
  agentModelSupportsCodexUltra,
} from "@/lib/agent-models";
import {
  invalidateAutomationQueries,
  useAutomationDetail,
} from "@/hooks/useAutomations";
import type { SubmitQuestionAnswerResult } from "@/hooks/useAskUserQuestion";
import { ideationKeys } from "@/hooks/useIdeation";
import { useIdeationSettings } from "@/hooks/useIdeationSettings";
import { useVerificationStatus, verificationStatusKey } from "@/hooks/useVerificationStatus";
import { useEventBus } from "@/providers/EventProvider";
import { selectQueuedMessages, useChatStore } from "@/stores/chatStore";
import { useUiStore } from "@/stores/uiStore";
import type {
  AgentArtifactTab,
  AgentProvider,
  AgentRuntimeSelection,
} from "@/stores/agentSessionStore";
import type {
  ManualRoleRuntimeSelection,
  ManualServiceTier,
} from "@/api/manual-role-defaults.types";
import { useAgentSessionStore } from "@/stores/agentSessionStore";
import { invalidateConversationDataQueries } from "@/hooks/useChat";
import type { AgentWorkspacePublishAttempt } from "./useAgentWorkspacePublisher";

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
import { buildCapabilityOptions } from "./composer/runtime/capabilityOptions";
import { AgentConversationBaseLine } from "./AgentConversationBaseLine";
import { AgentConversationWorkspaceLine } from "./AgentConversationWorkspaceLine";
import { AgentWorkspacePrReviewCard } from "./AgentWorkspacePrReviewCard";
import { shouldPollForPrReviewContext } from "./agentWorkspacePrReviewPresentation";
import { useAgentConversationRuntimeStatus } from "./useAgentConversationRuntimeStatus";
import { AgentsComposerWorkspaceChangesCard } from "./AgentsComposerWorkspaceChangesCard";
import { AgentsChatHeaderController } from "./AgentsChatHeaderController";
import { AgentWorkspaceFileLinkProvider } from "./AgentWorkspaceFileLinkProvider";
import { useResolvedAgentArtifactState } from "./agentArtifactState";
import {
  buildAgentConversationModeOptions,
  isConversationModeLocked,
} from "./agentConversationMode";
import type { DiffFilterMode } from "./AgentsPublishDiffFilter";
import {
  AGENT_PROVIDER_OPTIONS,
  agentEffortOptions,
  agentModelOptions,
  defaultModelForProvider,
  normalizeRuntimeSelection,
} from "./agentOptions";
import { agentConversationKeys } from "./useProjectAgentConversations";
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
  getFocusedAutomationRunConversationId,
  getFocusedChatSessionId,
  getFocusedWorkspaceReviewConversationId,
  getAutomationRunFocusOptions,
  type AgentsChatFocus,
  type AgentsChatFocusSwitchOption,
  type AgentsChatFocusType,
  type AutomationRunFocusOptions,
} from "./agentChatFocus";
import {
  isTaskRuntimeContextType,
  type AgentTaskRuntimeContextType,
} from "./agentTaskRuntimeContext";
import {
  buildPlanActionHint,
  isPlanRecommendationCheckPending,
} from "./agentPlanModeActions";
import {
  activateAgentPlanProposals,
  PlanContinuationCommittedError,
} from "./agentPlanProposalActivation";
import {
  implementAgentPlanDirectly,
  type DirectImplementationActivationSnapshot,
} from "./implementAgentPlanDirectly";
import { materializeWorkspaceRuntimeSelection } from "./agentPlanRuntime";
import { useApprovedPlanContinuation } from "./useApprovedPlanContinuation";
import { PRIMARY_AGENT_START_MODE_IDS } from "./agentStartModeOptions";
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
const AUTOMATION_SETUP_PROPOSAL_KIND = "automation_setup_proposal";
const AUTOMATION_SETUP_PROPOSAL_APPLY_VALUE = "apply_automation_proposal";
const PLAN_MODE_SWITCH_EVENT_RETRY_DELAY_MS = 150;
const PLAN_MODE_SWITCH_FALLBACK_RETRY_DELAY_MS = 750;
const PLAN_MODE_SWITCH_MAX_RETRY_ATTEMPTS = 40;
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
  proposalKey: string;
  attempt: number;
  autoContinueMessage: string | null;
}

interface PlanModeProposalAttempt {
  committed: boolean;
  inFlight: Promise<boolean> | null;
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

function acceptsAutomationSetupProposal(
  question: AskUserQuestionPayload,
  response: AskUserQuestionResponse,
): boolean {
  if (
    response.skipped === true ||
    !response.selectedOptions.includes(AUTOMATION_SETUP_PROPOSAL_APPLY_VALUE)
  ) {
    return false;
  }
  if (question.metadata?.kind === AUTOMATION_SETUP_PROPOSAL_KIND) {
    return true;
  }
  return (question.header?.toLowerCase() ?? "").includes("automation");
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
  if (chatFocus.type === "automation_run") {
    return (item.conversationId ?? item.contextId) === chatFocus.conversationId;
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
  tone?: "default" | "success";
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
  testIdPrefix = "agents-plan-composer-cta",
  actionGroupLabel = "Plan actions",
  compactHintOverride,
  suppressDetails = false,
}: {
  hint: string;
  actions: PlanComposerCtaAction[];
  viewPlanAction?: PlanComposerViewPlanAction | undefined;
  testIdPrefix?: string;
  actionGroupLabel?: string;
  compactHintOverride?: string | undefined;
  suppressDetails?: boolean;
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
  const compactHint =
    compactHintOverride ?? getPlanComposerCompactHint(hint, resolvedActions);
  const hintDetails = suppressDetails
    ? null
    : getPlanComposerHintDetails(hint, compactHint);
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
          data-testid={`${testIdPrefix}-details`}
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
        style={
          action.tone === "success"
            ? {
                backgroundColor: "var(--status-success-muted)",
                borderColor: "var(--status-success-border)",
                color: "var(--status-success)",
              }
            : undefined
        }
        onClick={action.onClick}
        disabled={action.disabled || action.isPending}
        data-testid={`${testIdPrefix}-${action.id}`}
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
      data-testid={`${testIdPrefix}-row`}
    >
      {isSingleAction ? (
        <div className="flex items-center gap-2">
          <div
            className="flex min-w-0 flex-1 items-center gap-2"
            data-testid={`${testIdPrefix}-copy`}
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
              data-testid={`${testIdPrefix}-hint`}
            >
              {compactHint}
            </p>
            {detailsButton}
          </div>
          <div
            className="flex shrink-0 items-center"
            role="group"
            aria-label={actionGroupLabel}
            data-testid={`${testIdPrefix}-actions`}
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
              data-testid={`${testIdPrefix}-copy`}
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
                data-testid={`${testIdPrefix}-hint`}
              >
                {compactHint}
              </p>
              {detailsButton}
            </div>
          </div>
          <div
            className="mt-2 flex flex-wrap items-center gap-2"
            role="group"
            aria-label={actionGroupLabel}
            data-testid={`${testIdPrefix}-actions`}
          >
            {resolvedActions.map(renderActionButton)}
          </div>
        </>
      )}
    </div>
  );
}

function hasPersistedAutomationPhaseSpec(goalItemsJson: string | null): boolean {
  const trimmed = goalItemsJson?.trim() ?? "";
  if (!trimmed) {
    return false;
  }
  try {
    const parsed = JSON.parse(trimmed) as unknown;
    return Array.isArray(parsed) && parsed.length > 0;
  } catch {
    return false;
  }
}

function isAutomationApprovalReady(automation: Automation): boolean {
  if (automation.status !== "draft") {
    return false;
  }
  if (!automation.goalPrompt.trim()) {
    return false;
  }
  if (!automation.firstRunPrompt?.trim()) {
    return false;
  }
  if (!automation.providerHarness.trim() || !automation.modelId.trim()) {
    return false;
  }
  if (!hasPersistedAutomationPhaseSpec(automation.goalItemsJson)) {
    return false;
  }
  if (automation.completionSignal === "pr_merged" && automation.runMode !== "edit") {
    return false;
  }
  if (automation.baseRefKind === "project_default") {
    return true;
  }
  if (automation.baseRefKind === "local_branch") {
    return Boolean(automation.baseRef.trim());
  }
  return false;
}

function hasOpenAutomationRun(
  automation: Automation,
  runs: readonly AutomationRun[],
): boolean {
  return runs.some((run) => getAutomationRunView(automation, run).isOpen);
}

interface AgentsActiveConversationPanelProps {
  activeConversation: AgentConversation;
  activeConversationMode: AgentConversationWorkspaceMode | null;
  activeConversationModeLocked: boolean;
  activeProjectId: string | null;
  activeProjectOptions: AgentComposerOption[];
  activeWorkspace: AgentConversationWorkspace | null;
  activeWorkspaceFreshness: AgentConversationWorkspaceFreshness | undefined;
  attachedIdeationSessionId: string | null;
  availableArtifactTabs: readonly IdeationArtifactTab[];
  chatFocus: AgentsChatFocus;
  chatFocusOptions: readonly AgentsChatFocusSwitchOption[];
  hasAttachedPlanArtifact: boolean;
  hasAutoOpenArtifacts: boolean;
  focusedWorkspaceReviewServiceTier: ManualServiceTier | null;
  normalizedActiveRuntime: AgentRuntimeSelection;
  onActiveConversationModeChange: (mode: AgentConversationWorkspaceMode) => void;
  onActiveConversationModeMenuOpen: () => void;
  onActiveCapabilityChange: (
    mode: CapabilityIntent["coordinationMode"],
  ) => void | Promise<unknown>;
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
    providerSupportedEfforts?: readonly string[] | null,
    providerSupportedModelAliases?: readonly string[] | null
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
  onFocusIdeationSessionForConversation: (
    conversationId: string,
    sessionId: string
  ) => void;
  onFocusWorkspaceReview: (
    conversationId: string,
    runtimeHint?: AgentRuntimeSelection,
  ) => void;
  onFocusVerificationSession: (
    parentSessionId: string,
    childSessionId: string
  ) => void;
  onFocusTaskRuntime: (
    taskId: string,
    contextType: AgentTaskRuntimeContextType
  ) => void;
  onFocusAutomationRun: (
    automationId: string,
    runId: string,
    conversationId: string,
    options?: AutomationRunFocusOptions
  ) => void;
  onOpenTaskArtifact: (taskId: string) => void;
  onOpenAutomation?: (automationId: string) => void;
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
  onStartPersonaBuilder: () => void;
  publishShortcutLabel: string;
  promotePublishShortcut?: boolean;
  publishAttemptsByConversationId: Record<string, AgentWorkspacePublishAttempt>;
  selectedConversationId: string;
  selectedTaskArtifactId: string | null;
  setTerminalChatDockElement: (element: HTMLDivElement | null) => void;
  switchingConversationModeId: string | null;
  updatingCapabilityConversationId: string | null;
  terminalArchivedReason: string | null;
  terminalUnavailableReason: string | null;
}

export const AgentsActiveConversationPanel = memo(function AgentsActiveConversationPanel({
  activeConversation,
  activeConversationMode,
  activeProjectId,
  activeProjectOptions,
  activeWorkspace,
  activeWorkspaceFreshness,
  attachedIdeationSessionId,
  availableArtifactTabs,
  chatFocus,
  chatFocusOptions,
  hasAttachedPlanArtifact,
  hasAutoOpenArtifacts,
  focusedWorkspaceReviewServiceTier,
  normalizedActiveRuntime,
  onActiveConversationModeChange,
  onActiveConversationModeMenuOpen,
  onActiveCapabilityChange,
  onActiveEffortChange,
  onActiveModelChange,
  onActiveProviderChange,
  onAgentUserMessageSent,
  onConversationModeSwitched,
  onFocusIdeationSession,
  onFocusIdeationSessionForConversation,
  onFocusWorkspaceReview,
  onFocusVerificationSession,
  onFocusTaskRuntime,
  onFocusAutomationRun,
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
  onStartPersonaBuilder,
  publishShortcutLabel,
  promotePublishShortcut = false,
  publishAttemptsByConversationId,
  selectedConversationId,
  selectedTaskArtifactId,
  setTerminalChatDockElement,
  switchingConversationModeId,
  updatingCapabilityConversationId,
  terminalArchivedReason,
  terminalUnavailableReason,
}: AgentsActiveConversationPanelProps) {
  const resolvedConversationModeLocked = isConversationModeLocked(
    activeConversation,
    activeWorkspace,
  );
  const queryClient = useQueryClient();
  // Switching a conversation's mode reaches `ensure_git_worktree` on the host, so remotely it
  // travels as the WP5a spawn-free intent. The gate is resolved from the intent's presence on
  // the facade: a host that predates it, or a device without `ui:agent`, gets a disabled picker
  // carrying the reason instead of a control whose every click ends in a refusal.
  const modeSwitchGate = useAgentGate("conversationModeSwitch");
  const planApproveGate = useAgentGate("planApprove");
  const conversationForkGate = useAgentGate("conversationFork");
  const ideationSettingsQuery = useIdeationSettings();
  const tasksEnabled =
    !ideationSettingsQuery.isLoading &&
    !ideationSettingsQuery.isError &&
    ideationSettingsQuery.settings.tasksEnabled;
  const bus = useEventBus();
  const focusedChatSessionId = getFocusedChatSessionId(chatFocus);
  const focusedWorkspaceReviewConversationId =
    getFocusedWorkspaceReviewConversationId(chatFocus);
  const runtimeControlConversationId =
    focusedWorkspaceReviewConversationId ?? selectedConversationId;
  const { registry: modelRegistry } = useAgentModels();
  const { data: featureFlags } = useFeatureFlags();
  const roleDefaultQuery = useConversationRoleDefault(selectedConversationId);
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
  const [isApprovingAutomation, setIsApprovingAutomation] = useState(false);
  const [isRunningAutomation, setIsRunningAutomation] = useState(false);
  const [isResettingRoleDefault, setIsResettingRoleDefault] = useState(false);
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
  const planModeProposalAttemptsRef = useRef(
    new Map<string, PlanModeProposalAttempt>(),
  );
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
  const codexUltraAvailable = agentModelSupportsCodexUltra(
    normalizedActiveRuntime.provider,
    normalizedActiveRuntime.modelId,
    modelRegistry,
    codexProviderSettings?.ultraSupportedModels,
  );
  const activeCapabilityAvailable =
    activeConversation.coordinationMode === "solo" ||
    (activeConversation.coordinationMode === "rx_native_team" &&
      featureFlags.agentConversationTeam) ||
    (activeConversation.coordinationMode === "rx_native_workflow" &&
      featureFlags.agentConversationWorkflows) ||
    (activeConversation.coordinationMode === "codex_native_ultra" &&
      codexUltraAvailable);
  const capabilityBlockedReason = activeCapabilityAvailable
    ? null
    : activeConversation.coordinationMode === "codex_native_ultra"
      ? "Codex Ultra is unavailable for the selected model or account. Switch to Defaults or choose a supported Codex runtime."
      : "This conversation's capability is disabled. Enable it in Settings > Capabilities or switch to Defaults.";
  const capabilityOptions = (() => {
    const options = buildCapabilityOptions({
      teamEnabled: featureFlags.agentConversationTeam,
      workflowsEnabled: featureFlags.agentConversationWorkflows,
      codexUltraAvailable,
    });
    if (!options.some((option) => option.id === activeConversation.coordinationMode)) {
      const labels: Record<string, string> = {
        rx_native_team: "Team (disabled)",
        rx_native_workflow: "Workflow (disabled)",
        codex_native_ultra: "Ultra (unavailable)",
      };
      options.push({
        id: activeConversation.coordinationMode,
        label: labels[activeConversation.coordinationMode] ?? "Unavailable",
        description: capabilityBlockedReason ?? "This capability is unavailable.",
        disabled: true,
      });
    }
    return options;
  })();
  const handleActiveCapabilitySelection = useCallback(
    async (next: CapabilityIntent["coordinationMode"]) => {
      if (next === activeConversation.coordinationMode) {
        return;
      }
      if (next === "codex_native_ultra") {
        const confirmed = await confirm({
          title: "Enable Codex Ultra?",
          description:
            "Ultra activates provider-native subagents plus maximum reasoning and can dramatically increase total usage. Select it only after considering the cost.",
          confirmText: "Enable Ultra",
        });
        if (!confirmed) {
          return;
        }
      }
      await onActiveCapabilityChange(next);
    },
    [activeConversation.coordinationMode, confirm, onActiveCapabilityChange],
  );
  const conversationServiceTier = activeConversation.serviceTier
    ?.trim()
    .toLowerCase();
  const persistedConversationServiceTier = useAgentSessionStore(
    (state) => state.serviceTierByConversationId[runtimeControlConversationId],
  );
  const activeServiceTier: ManualServiceTier =
    focusedWorkspaceReviewServiceTier ??
    persistedConversationServiceTier ??
    (conversationServiceTier === "fast" || conversationServiceTier === "standard"
      ? conversationServiceTier
      : "provider_default");
  const handleActiveServiceTierChange = useCallback(
    (value: string) => {
      useAgentSessionStore
        .getState()
        .setServiceTierForConversation(
          runtimeControlConversationId,
          value as ManualServiceTier,
        );
    },
    [runtimeControlConversationId],
  );
  const handleResetRoleDefault = useCallback(async () => {
    if (chatFocus.type !== "workspace") {
      return;
    }
    setIsResettingRoleDefault(true);
    try {
      const resolved = await manualRoleDefaultsApi.resetConversation({
        conversationId: selectedConversationId,
      });
      const nextProvider =
        resolved.value.provider === "claude" ||
        resolved.value.provider === "codex"
          ? resolved.value.provider
          : null;
      if (!nextProvider) {
        throw new Error(
          `Unsupported provider in ${resolved.role} default: ${resolved.value.provider}`,
        );
      }
      const nextRuntime = normalizeRuntimeSelection(
        {
          provider: nextProvider,
          modelId:
            resolved.value.model ??
            defaultModelForProvider(
              nextProvider,
              modelRegistry,
              supportedModelAliasesForProvider(providerOptions, nextProvider),
            ),
          ...(resolved.value.effort
            ? { effort: resolved.value.effort as AgentRuntimeSelection["effort"] }
            : {}),
        },
        modelRegistry,
        supportedEffortsForProvider(providerOptions, nextProvider),
        supportedModelAliasesForProvider(providerOptions, nextProvider),
      );
      const refreshedRoleDefault = roleDefaultQuery.refetch().then((result) => {
        if (result.isError || !result.data) {
          throw result.error instanceof Error
            ? result.error
            : new Error("Failed to load the current role default");
        }
        return result.data;
      });
      await Promise.all([
        activeProjectId
          ? queryClient.invalidateQueries({
              queryKey: agentConversationKeys.project(activeProjectId),
            })
          : Promise.resolve(),
        invalidateConversationDataQueries(queryClient, selectedConversationId),
        refreshedRoleDefault,
      ]);
      useAgentSessionStore
        .getState()
        .setRoleDefaultRuntimeForConversation(
          selectedConversationId,
          activeProjectId,
          nextRuntime,
        );
      useAgentSessionStore
        .getState()
        .setServiceTierForConversation(
          selectedConversationId,
          resolved.value.serviceTier,
        );
    } catch (error) {
      toast.error(
        error instanceof Error
          ? error.message
          : "Failed to reset the current role default",
      );
    } finally {
      setIsResettingRoleDefault(false);
    }
  }, [
    activeProjectId,
    chatFocus.type,
    modelRegistry,
    providerOptions,
    queryClient,
    roleDefaultQuery,
    selectedConversationId,
  ]);
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
      ? activeServiceTier === "provider_default"
        ? null
        : activeServiceTier === "fast"
      : null;
  const openProviderSettings = useCallback(() => {
    openModal("settings", { section: "providers" });
  }, [openModal]);
  const panelIdeationSessionId =
    focusedChatSessionId ??
    (activeConversation.contextType === "ideation" ? activeConversation.contextId : undefined);
  const focusedAutomationRunConversationId =
    getFocusedAutomationRunConversationId(chatFocus);
  const taskRuntimeFocus = chatFocus.type === "task_runtime" ? chatFocus : null;
  const panelSelectedTaskId = taskRuntimeFocus?.taskId ?? null;
  const panelTaskRuntimeContextType = taskRuntimeFocus?.contextType;
  const focusedPanelKey = taskRuntimeFocus
    ? `${taskRuntimeFocus.contextType}:${taskRuntimeFocus.taskId}`
    : focusedWorkspaceReviewConversationId
    ? `workspace_review:${focusedWorkspaceReviewConversationId}`
    : focusedAutomationRunConversationId
    ? `automation_run:${focusedAutomationRunConversationId}`
    : focusedChatSessionId ?? "workspace";
  const isFocusedChildChat = chatFocus.type !== "workspace";
  const activeAutomationRunId =
    chatFocus.type === "automation_run"
      ? chatFocus.runId
      : activeConversation.automationRunId ?? null;
  const automationDetailQuery = useAutomationDetail(
    activeConversation.automationId,
    {
      enabled:
        Boolean(activeConversation.automationId) &&
        (!isFocusedChildChat || chatFocus.type === "automation_run"),
    },
  );
  const automationRun = useMemo(
    () =>
      automationDetailQuery.data?.runs.find(
        (run) => run.id === activeAutomationRunId,
      ) ?? null,
    [activeAutomationRunId, automationDetailQuery.data?.runs],
  );
  const automationRunReadOnlyReason =
    activeAutomationRunId &&
    (!automationRun ||
      isAutomationRunComposerReadOnly(automationRun))
      ? "Automation run conversations are read-only while the automation is working on this run."
      : null;
  // Automation SETUP conversation: automationId present, no run yet. Editable —
  // the user configures the automation by chatting with the setup agent. Mutually
  // exclusive with automationRunConversationId (which requires automationRunId).
  const automationSetupConversationId =
    !isFocusedChildChat &&
    activeConversation.agentMode === "automation" &&
    activeConversation.automationId &&
    !activeConversation.automationRunId
      ? activeConversation.automationId
      : null;
  const automationSetupDetail =
    automationSetupConversationId && automationDetailQuery.data
      ? automationDetailQuery.data
      : null;
  const usesWorkspaceRuntimeControls =
    !isFocusedChildChat || chatFocus.type === "workspace_review";
  const workspaceSendRuntime = usesWorkspaceRuntimeControls
    ? selectableWorkspaceRuntime
    : normalizedActiveRuntime;
  const panelCodexFastModeOption = focusedWorkspaceReviewConversationId
    ? workspaceSendRuntime.provider === "codex"
      ? false
      : null
    : activeCodexFastModeOption;
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
    if (focusedAutomationRunConversationId) {
      return buildStoreKey("project", focusedAutomationRunConversationId);
    }
    if (focusedChatSessionId) {
      return buildStoreKey("ideation", focusedChatSessionId);
    }
    return getAgentConversationStoreKey(activeConversation);
  }, [
    activeConversation,
    focusedAutomationRunConversationId,
    focusedChatSessionId,
    focusedWorkspaceReviewConversationId,
    taskRuntimeFocus,
  ]);
  const queuedMessagesSelector = useMemo(
    () => selectQueuedMessages(panelStoreKeyOverride),
    [panelStoreKeyOverride]
  );
  const panelConversationIdOverride =
    taskRuntimeFocus
      ? null
      : focusedWorkspaceReviewConversationId ??
        focusedAutomationRunConversationId ??
        (!isFocusedChildChat ? selectedConversationId : null);
  const panelAgentProcessContextIdOverride = taskRuntimeFocus
    ? taskRuntimeFocus.taskId
    : focusedWorkspaceReviewConversationId ??
      focusedAutomationRunConversationId ??
      (!isFocusedChildChat && activeConversation.contextType === "project"
        ? selectedConversationId
        : null);
  const panelSendConversationId =
    taskRuntimeFocus
      ? null
      : focusedWorkspaceReviewConversationId ??
        focusedAutomationRunConversationId ??
        (!isFocusedChildChat ? selectedConversationId : null);
  const queuedMessages = useChatStore(queuedMessagesSelector);
  const executionHaltState = useUiStore((s) =>
    getAgentQueueHaltState({
      ...s.executionStatus,
      isKnown: s.executionStatusKnown,
    })
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
  const handleOpenAutomationRun = useCallback(
    (automationId: string, run: AutomationRun) => {
      if (!run.conversationId) {
        return;
      }
      if (run.status === "awaiting_plan_approval") {
        onSelectArtifact("plan");
      }
      onFocusAutomationRun(
        automationId,
        run.id,
        run.conversationId,
        getAutomationRunFocusOptions(run),
      );
    },
    [onFocusAutomationRun, onSelectArtifact],
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
  const automationConfig = automationSetupDetail?.automation ?? null;
  const automationConfigId =
    automationConfig?.id ?? activeConversation.automationId ?? null;
  const modeOptions = useMemo(() => {
    const eligibleOptions = buildAgentConversationModeOptions({
      currentMode: activeConversationMode ?? "chat",
      taskPipelineAvailable:
        tasksEnabled &&
        (activeWorkspace?.taskPipelineAvailable ??
          Boolean(activeWorkspace?.taskPipelineSessionId)),
      autopilotEnabled: featureFlags.agentConversationAutopilot ?? false,
    }).filter(
      (option) =>
        tasksEnabled ||
        option.id !== "tasks" ||
        option.id === activeConversationMode,
    );
    if (!resolvedConversationModeLocked) {
      return eligibleOptions;
    }
    const lockReason =
      activeWorkspace?.modeSwitchLockReason ??
      "Active ideation or execution state owns this workspace.";
    return eligibleOptions.map((option) => ({
      ...option,
      disabled: true,
      disabledReason: lockReason,
    }));
  }, [
    activeConversationMode,
    resolvedConversationModeLocked,
    activeWorkspace?.modeSwitchLockReason,
    activeWorkspace?.taskPipelineAvailable,
    activeWorkspace?.taskPipelineSessionId,
    featureFlags.agentConversationAutopilot,
    tasksEnabled,
  ]);
  const secondaryModeOptionIds = useMemo(
    () =>
      modeOptions
        .filter(
          (option) =>
            option.id !== "tasks" &&
            option.id !== "ideation" &&
            !PRIMARY_AGENT_START_MODE_IDS.includes(option.id),
        )
        .map((option) => option.id),
    [modeOptions],
  );
  const isPlanWorkspaceComposer =
    !isFocusedChildChat &&
    activeConversationMode === "plan" &&
    activeWorkspace?.mode === "plan";
  const canShowPlanComposerViewPrompt =
    isPlanWorkspaceComposer &&
    hasAttachedPlanArtifact &&
    availableArtifactTabs.includes("plan");
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
      shouldPollForPrReviewContext(
        prReviewContextForConversation(
          query.state.data,
          activeWorkspace?.conversationId,
        ),
      )
        ? 5_000
        : false,
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
  const isPlanBundleComplete =
    planApprovalArtifact?.planContractVersion !== 2 ||
    Boolean(planApprovalArtifact.blueprint);
  const canApproveComposerPlan =
    !!planApprovalSessionId &&
    !!planApprovalArtifact &&
    isPlanBundleComplete &&
    planArtifactApprovalStatus === "draft";
  const canCreatePlanProposals =
    !!planApprovalSessionId &&
    isPlanApproved &&
    isPlanBundleComplete &&
    !activeAutomationRunId &&
    tasksEnabled;
  const canImplementPlanDirectly = Boolean(
    planApprovalSessionId &&
      isPlanApproved &&
      isPlanBundleComplete &&
      activeWorkspace?.conversationId &&
      activeProjectId &&
      !activeAutomationRunId,
  );
  const planComplexityQuery = useQuery({
    queryKey: [
      "agents",
      "plan-complexity",
      planApprovalSessionId,
      planApprovalArtifact?.id,
      planApprovalArtifact?.metadata.version,
      planApprovalArtifact?.blueprint?.id,
      planApprovalArtifact?.blueprint?.metadata.version,
    ],
    queryFn: () => artifactApi.getPlanComplexityAssessment(planApprovalSessionId!),
    enabled: Boolean(
      tasksEnabled &&
        planApprovalSessionId &&
        planApprovalArtifact?.id &&
        isPlanApproved,
    ),
    staleTime: 5_000,
    refetchInterval: (query) => (query.state.data ? false : 4_000),
  });
  const isPlanRecommendationPending = tasksEnabled && isPlanRecommendationCheckPending({
    assessment: planComplexityQuery.data,
    isFetching:
      (planComplexityQuery.isFetching || planComplexityQuery.isLoading) &&
      !planComplexityQuery.data,
    approvedAt: planApprovalArtifact?.planApproval?.approvedAt,
  });
  const planVerificationQuery = useVerificationStatus(
    planApprovalSessionId && planApprovalArtifact ? planApprovalSessionId : undefined,
    activeWorkspace?.conversationId,
  );
  const planVerificationState = planVerificationQuery.data?.status ?? null;
  const planVerificationInProgress =
    planVerificationQuery.data?.inProgress ?? false;
  const isPlanVerificationLoading =
    (planVerificationQuery.isLoading || planVerificationQuery.isFetching) &&
    !planVerificationQuery.data;
  const isPlanVerificationSatisfied = planVerificationState === "verified";
  const canVerifyComposerPlan = Boolean(
    planApprovalSessionId &&
      planApprovalArtifact &&
      !isPlanVerificationLoading,
  );
  const {
    confirmImplementDirectly,
    confirmCreateProposals,
    confirmationDialogProps: planContinuationDialogProps,
    ConfirmationDialog: PlanContinuationDialog,
  } = useApprovedPlanContinuation({
    conversationId: activeWorkspace?.conversationId ?? null,
    projectId: activeProjectId,
  });

  const handleApprovePlanFromQuestion = useCallback(async () => {
    if (planApproveGate.gated || !planApprovalSessionId || !planApprovalArtifact || !canApproveComposerPlan) {
      return;
    }
    setIsApprovingPlan(true);
    try {
      const approved = await artifactApi.approvePlanArtifact({
        sessionId: planApprovalSessionId,
        artifactId: planApprovalArtifact.id,
        ...(planApprovalArtifact.blueprint && {
          blueprintArtifactId: planApprovalArtifact.blueprint.id,
          blueprintArtifactVersion: planApprovalArtifact.blueprint.metadata.version,
        }),
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
    planApproveGate.gated,
    canApproveComposerPlan,
    planApprovalArtifact,
    planApprovalSessionId,
    queryClient,
  ]);
  const handleCreatePlanProposals = useCallback(() => {
    if (!planApprovalSessionId || !canCreatePlanProposals) {
      return;
    }
    let workspaceActivationCompleted = activeWorkspace?.mode === "tasks";
    let committedRuntimeOverride: ManualRoleRuntimeSelection | null = null;
    void confirmCreateProposals(async (runtimeOverride) => {
      const runtimeForAttempt = committedRuntimeOverride ?? runtimeOverride;
      setIsCreatingPlanProposals(true);
      try {
        await activateAgentPlanProposals({
        sessionId: planApprovalSessionId,
        workspace: activeWorkspace,
        queryClient,
        canPromoteWorkspace: true,
        onConversationModeSwitched,
          onFocusIdeationSessionForConversation,
          runtimeOverride: runtimeForAttempt,
          workspaceActivationCompleted,
          onWorkspaceActivated: () => {
            workspaceActivationCompleted = true;
            committedRuntimeOverride = { ...runtimeForAttempt };
          },
        });
        toast.success("Proposal creation requested");
      } catch (err) {
        console.error("Failed to create proposals:", err);
        toast.error("Failed to request proposal creation");
        throw err;
      } finally {
        setIsCreatingPlanProposals(false);
      }
    });
  }, [
    activeWorkspace,
    canCreatePlanProposals,
    onConversationModeSwitched,
    onFocusIdeationSessionForConversation,
    planApprovalSessionId,
    queryClient,
    confirmCreateProposals,
  ]);
  const handleImplementPlanDirectly = useCallback(() => {
    if (
      !planApprovalSessionId ||
      !activeProjectId ||
      !activeWorkspace?.conversationId ||
      !canImplementPlanDirectly
    ) {
      return;
    }
    let pinnedActivation: DirectImplementationActivationSnapshot | undefined;
    let committedRuntimeOverride: ManualRoleRuntimeSelection | null = null;
    void confirmImplementDirectly(async (runtimeOverride) => {
      const runtimeForAttempt = committedRuntimeOverride ?? runtimeOverride;
      setIsImplementingPlanDirectly(true);
      try {
        await implementAgentPlanDirectly({
          projectId: activeProjectId,
          workspace: pinnedActivation?.workspace ?? activeWorkspace,
          queryClient,
          onConversationModeSwitched,
          ...(pinnedActivation ? { pinnedActivation } : {}),
          onActivated: (snapshot) => {
            if (!pinnedActivation) {
              pinnedActivation = snapshot;
              committedRuntimeOverride = { ...runtimeForAttempt };
            }
          },
          sendOptions: { runtimeOverride: runtimeForAttempt },
        });
        useAgentSessionStore.getState().setRuntimeForConversation(
          activeWorkspace.conversationId,
          activeProjectId,
          materializeWorkspaceRuntimeSelection(runtimeForAttempt, modelRegistry),
        );
        useAgentSessionStore
          .getState()
          .setServiceTierForConversation(
            activeWorkspace.conversationId,
            runtimeForAttempt.serviceTier,
          );
        toast.success("Implementation started");
      } catch (err) {
        console.error("Failed to implement plan directly:", err);
        if (!(err instanceof PlanContinuationCommittedError)) {
          toast.error(
            err instanceof Error ? err.message : "Failed to start implementation",
          );
        }
        throw err;
      } finally {
        setIsImplementingPlanDirectly(false);
      }
    });
  }, [
    activeProjectId,
    activeWorkspace,
    canImplementPlanDirectly,
    onConversationModeSwitched,
    planApprovalSessionId,
    queryClient,
    modelRegistry,
    confirmImplementDirectly,
  ]);
  const handleVerifyPlanFromComposer = useCallback(async () => {
    if (
      !planApprovalSessionId ||
      !canVerifyComposerPlan ||
      planVerificationInProgress
    ) {
      return;
    }
    if (isPlanVerificationSatisfied) {
      const confirmed = await confirm({
        title: "Verify this plan again?",
        description:
          "The current plan is already verified. This queues another visible review turn and keeps the existing proof unless the plan changes.",
        confirmText: "Verify again",
      });
      if (!confirmed) {
        return;
      }
    }
    setIsStartingPlanVerification(true);
    try {
      await verificationApi.confirm(planApprovalSessionId);
      await Promise.all([
        queryClient.invalidateQueries({
          queryKey: verificationStatusKey(planApprovalSessionId),
        }),
        queryClient.invalidateQueries({
          queryKey: ideationKeys.sessionWithData(planApprovalSessionId),
        }),
        queryClient.invalidateQueries({ queryKey: ideationKeys.sessions() }),
      ]);
      toast.success("Verify Plan queued in this conversation");
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
    confirm,
    isPlanVerificationSatisfied,
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
    if (!tasksEnabled && isPlanApproved) {
      return "Recommended: Implement Directly.";
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
    tasksEnabled,
    isPlanApproved,
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
          label: isPlanVerificationSatisfied ? "Verified" : "Verify Plan",
          icon: ShieldCheck,
          isPrimary: false,
          isPending:
            isStartingPlanVerification || planVerificationInProgress,
          disabled: isPlanRecommendationPending,
          tone: isPlanVerificationSatisfied ? "success" : "default",
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
          disabled: planApproveGate.gated,
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
              (!tasksEnabled ||
                planComplexityQuery.data?.recommendedAction !==
                "create_proposals"),
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
      tasksEnabled &&
      (isPlanRecommendationPending ||
        planComplexityQuery.data?.recommendedAction === "create_proposals")
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
    isPlanVerificationSatisfied,
    isStartingPlanVerification,
    planApprovalArtifact,
    planApprovalSessionId,
    planComplexityQuery.data?.recommendedAction,
    tasksEnabled,
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
  const handleApproveAutomation = useCallback(async () => {
    if (!automationConfigId || isApprovingAutomation) {
      return;
    }
    setIsApprovingAutomation(true);
    try {
      await automationsApi.finalize(automationConfigId);
      invalidateAutomationQueries(queryClient, automationConfigId);
      toast.success("Automation spec approved");
    } catch (err) {
      console.error("Failed to approve automation:", err);
      toast.error(err instanceof Error ? err.message : "Failed to approve automation");
    } finally {
      setIsApprovingAutomation(false);
    }
  }, [automationConfigId, isApprovingAutomation, queryClient]);
  const handleRunAutomation = useCallback(async () => {
    if (!automationConfigId || isRunningAutomation) {
      return;
    }
    setIsRunningAutomation(true);
    try {
      const schedule = await automationsApi.triggerRunNow(automationConfigId);
      invalidateAutomationQueries(queryClient, automationConfigId);
      if (schedule.scheduled) {
        toast.success("Automation run queued");
      } else {
        toast.info(schedule.reason ?? "Automation run was not scheduled");
      }
    } catch (err) {
      console.error("Failed to run automation:", err);
      toast.error(err instanceof Error ? err.message : "Failed to run automation");
    } finally {
      setIsRunningAutomation(false);
    }
  }, [automationConfigId, isRunningAutomation, queryClient]);
  const automationComposerCtaActions = useMemo<PlanComposerCtaAction[]>(() => {
    const automation = automationSetupDetail?.automation;
    if (!automation) {
      return [];
    }
    if (automation.status === "draft") {
      if (!isAutomationApprovalReady(automation)) {
        return [];
      }
      return [
        {
          id: "approve",
          label: "Approve",
          icon: CheckCircle2,
          isPrimary: true,
          isPending: isApprovingAutomation,
          disabled: false,
          onClick: () => {
            void handleApproveAutomation();
          },
        },
      ];
    }
    if (
      automation.status === "active" &&
      !hasOpenAutomationRun(automation, automationSetupDetail.runs)
    ) {
      return [
        {
          id: "run",
          label: "Run",
          icon: Play,
          isPrimary: true,
          isPending: isRunningAutomation,
          disabled: false,
          onClick: () => {
            void handleRunAutomation();
          },
        },
      ];
    }
    return [];
  }, [
    automationSetupDetail,
    handleApproveAutomation,
    handleRunAutomation,
    isApprovingAutomation,
    isRunningAutomation,
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
      disabled: isApprovingPlan || planApproveGate.gated,
      isPending: isApprovingPlan,
    };
  }, [
    canApproveComposerPlan,
    handleApprovePlanFromQuestion,
    isApprovingPlan,
    planApproveGate.gated,
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
          activeProjectId!,
          trimmedMessage,
          undefined,
          {
            conversationId,
            providerHarness: workspaceSendRuntime.provider,
            modelId: workspaceSendRuntime.modelId,
            logicalEffort: workspaceSendRuntime.effort,
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
      onAgentUserMessageSent,
      workspaceSendRuntime,
    ],
  );

  const switchConversationToPlanMode = useCallback(
    async (
      conversationId: string,
      options?: {
        deferIfRunning?: boolean;
        showDeferredToast?: boolean;
        autoContinueMessage?: string | null;
        proposalKey: string;
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
            proposalKey: options.proposalKey,
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

  const attemptPlanModeProposal = useCallback(
    (
      proposalKey: string,
      conversationId: string,
      autoContinueMessage: string,
      options?: { initiallyPlan?: true },
    ): Promise<boolean> => {
      let attempt = planModeProposalAttemptsRef.current.get(proposalKey);
      if (!attempt) {
        attempt = { committed: false, inFlight: null };
        planModeProposalAttemptsRef.current.set(proposalKey, attempt);
      }
      if (attempt.committed) {
        return Promise.resolve(true);
      }
      if (attempt.inFlight) {
        return attempt.inFlight;
      }

      const activation = (async () => {
        const cachedWorkspace =
          queryClient.getQueryData<AgentConversationWorkspace>(
            agentWorkspaceKeys.workspace(conversationId),
          );
        const isAlreadyPlan =
          cachedWorkspace?.mode === "plan" || options?.initiallyPlan === true;
        if (isAlreadyPlan) {
          attempt.committed = true;
          if (
            pendingPlanModeSwitchConversationIdRef.current === conversationId
          ) {
            pendingPlanModeSwitchConversationIdRef.current = null;
            pendingPlanModeSwitchAutoContinueMessageRef.current = null;
            pendingPlanModeSwitchRetryCountRef.current = 0;
            setPendingPlanModeSwitch(null);
          }
          onConversationModeSwitched(
            conversationId,
            "plan",
            cachedWorkspace ?? activeWorkspace,
          );
          await continuePlanModeConversation(conversationId, autoContinueMessage);
          return true;
        }

        const switched = await switchConversationToPlanMode(conversationId, {
          deferIfRunning: true,
          showDeferredToast: false,
          autoContinueMessage,
          proposalKey,
        });
        if (switched) {
          attempt.committed = true;
        }
        return switched;
      })();
      attempt.inFlight = activation;
      void activation.then(
        () => {
          if (attempt.inFlight === activation) {
            attempt.inFlight = null;
          }
        },
        () => {
          if (attempt.inFlight === activation) {
            attempt.inFlight = null;
          }
        },
      );
      return activation;
    },
    [
      activeWorkspace,
      continuePlanModeConversation,
      onConversationModeSwitched,
      queryClient,
      switchConversationToPlanMode,
    ],
  );

  useEffect(() => {
    if (!pendingPlanModeSwitch) {
      return;
    }

    const conversationId = pendingPlanModeSwitch.conversationId;
    let eventRetryTimer: number | undefined;
    let fallbackRetryTimer: number | undefined;
    const retryAfterCompletedRun = (payload: AgentRunCompletedPayload) => {
      if (payload.conversation_id !== conversationId) {
        return;
      }
      if (eventRetryTimer !== undefined) {
        return;
      }

      eventRetryTimer = window.setTimeout(() => {
        eventRetryTimer = undefined;
        void attemptPlanModeProposal(
          pendingPlanModeSwitch.proposalKey,
          conversationId,
          pendingPlanModeSwitch.autoContinueMessage ?? "",
        );
      }, PLAN_MODE_SWITCH_EVENT_RETRY_DELAY_MS);
    };
    fallbackRetryTimer = window.setTimeout(() => {
      fallbackRetryTimer = undefined;
      void attemptPlanModeProposal(
        pendingPlanModeSwitch.proposalKey,
        conversationId,
        pendingPlanModeSwitch.autoContinueMessage ?? "",
      );
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
  }, [attemptPlanModeProposal, bus, pendingPlanModeSwitch]);

  const handleQuestionAnswered = useCallback(
    async (
      question: AskUserQuestionPayload,
      response: AskUserQuestionResponse,
      result?: SubmitQuestionAnswerResult,
    ) => {
      if (
        automationConfigId &&
        activeConversation.agentMode === "automation" &&
        !activeConversation.automationRunId &&
        acceptsAutomationSetupProposal(question, response)
      ) {
        invalidateAutomationQueries(queryClient, automationConfigId);
        return;
      }

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
      const cachedWorkspace = queryClient.getQueryData<AgentConversationWorkspace>(
        agentWorkspaceKeys.workspace(selectedConversationId),
      );
      const isAlreadyPlan =
        activeConversationMode === "plan" ||
        activeWorkspace?.mode === "plan" ||
        cachedWorkspace?.mode === "plan";

      if (isAlreadyPlan) {
        await attemptPlanModeProposal(
          `${proposalConversationId}:${question.requestId}`,
          selectedConversationId,
          autoContinueMessage,
          { initiallyPlan: true },
        );
        return;
      }

      if (resolvedConversationModeLocked) {
        toast.error(
          activeWorkspace?.modeSwitchLockReason ??
            "This conversation cannot switch modes while the workspace is busy.",
        );
        return;
      }

      await attemptPlanModeProposal(
        `${proposalConversationId}:${question.requestId}`,
        selectedConversationId,
        autoContinueMessage,
      );
    },
    [
      activeConversation.agentMode,
      activeConversation.contextType,
      activeConversation.automationRunId,
      activeConversationMode,
      resolvedConversationModeLocked,
      activeWorkspace,
      automationConfigId,
      attemptPlanModeProposal,
      isFocusedChildChat,
      queryClient,
      selectedConversationId,
    ],
  );

  const workspaceConversationId =
    activeWorkspace?.conversationId ?? selectedConversationId;

  return (
    <div
      className="flex-1 h-full flex flex-col"
      style={{ minWidth: AGENTS_CHAT_MIN_WIDTH }}
      data-testid="agents-active-conversation-panel"
    >
      <div className="min-h-0 flex-1">
        <AgentWorkspaceFileLinkProvider
          conversationId={workspaceConversationId}
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
            {...(activeConversation.contextType === "standalone"
              ? {
                  contextTypeOverride: "standalone" as const,
                  contextIdOverride: activeConversation.contextId,
                }
              : {})}
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
                    providerHarness: workspaceSendRuntime.provider,
                    modelId: workspaceSendRuntime.modelId,
                    logicalEffort: workspaceSendRuntime.effort,
                    codexFastMode: panelCodexFastModeOption,
                  },
                }
              : {})}
            onUserMessageSent={onAgentUserMessageSent}
            onQuestionAnswered={handleQuestionAnswered}
            onChildSessionNavigate={onFocusIdeationSession}
            onBuildPersona={onStartPersonaBuilder}
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
                if (conversationForkGate.gated) return;
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
                      forkResult.conversation.contextType,
                      forkResult.conversation.contextId,
                      trimmedFollowup,
                      undefined,
                      {
                        conversationId: forkResult.conversation.id,
                        providerHarness: workspaceSendRuntime.provider,
                        modelId: workspaceSendRuntime.modelId,
                        logicalEffort: workspaceSendRuntime.effort,
                        codexFastMode: activeCodexFastModeOption,
                        ...(options?.capabilityIntent
                          ? { capabilityIntent: options.capabilityIntent }
                          : {}),
                        ...(options?.teamIntent
                          ? { teamIntent: options.teamIntent }
                          : {}),
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
                        ...(options?.excerptReferences?.length
                          ? {
                              composerExcerptReferences:
                                options.excerptReferences,
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
              const shouldShowAutomationComposerCta =
                automationComposerCtaActions.length > 0 &&
                composerProps.questionMode === undefined;
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
                    automationDetail={automationSetupDetail}
                    currentAutomationRunId={activeAutomationRunId}
                    isAgentGenerating={composerProps.agentStatus === "generating"}
                    pauseHydration={isComposerHydrationPaused}
                    onViewWorkspace={handleViewRuntimeWorkspace}
                    onViewIdeation={onFocusIdeationSession}
                    onViewVerification={onFocusVerificationSession}
                    onViewWorkspaceReview={handleViewRuntimeWorkspaceReview}
                    onViewTaskRuntime={handleViewRuntimeTask}
                    onViewAutomationRun={handleOpenAutomationRun}
                    onOpenFile={onOpenPublishFile}
                    onPreloadPublishPane={onPreloadArtifacts}
                  />
                  {shouldShowPlanComposerCta && (
                    <PlanComposerCtaRow
                      hint={planComposerHint}
                      actions={planComposerCtaActions}
                      viewPlanAction={planComposerViewPlanAction}
                      suppressDetails={!tasksEnabled && isPlanApproved}
                    />
                  )}
                  {shouldShowAutomationComposerCta && (
                    <PlanComposerCtaRow
                      hint={
                        automationSetupDetail?.automation.status === "draft"
                          ? "Approve the automation spec. The setup has a goal, phase spec, run mode, model, base, and first-run prompt."
                          : "Run the approved automation now."
                      }
                      actions={automationComposerCtaActions}
                      testIdPrefix="agents-automation-composer-cta"
                      actionGroupLabel="Automation actions"
                      compactHintOverride={
                        automationSetupDetail?.automation.status === "draft"
                          ? "Ready for approval"
                          : "Run available"
                      }
                    />
                  )}
                  {automationRunReadOnlyReason && (
                    <AutomationRunStatusHeader
                      automation={automationSetupDetail?.automation ?? null}
                      run={automationRun ?? null}
                      density="banner"
                      message={automationRunReadOnlyReason}
                      testId="agents-automation-run-readonly-banner"
                    />
                  )}
                  {capabilityBlockedReason && (
                    <div
                      className="mx-2 mb-2 flex flex-wrap items-center justify-between gap-2 rounded-md border px-3 py-2 text-[0.75rem]"
                      style={{
                        backgroundColor: "var(--status-warning-muted)",
                        borderColor: "var(--status-warning-border)",
                        borderStyle: "solid",
                        borderWidth: "1px",
                        color: "var(--text-secondary)",
                      }}
                      data-testid="agents-conversation-capability-blocked"
                    >
                      <span>{capabilityBlockedReason}</span>
                      <button
                        type="button"
                        className="font-medium"
                        style={{ color: "var(--accent-primary)" }}
                        onClick={() =>
                          openModal("settings", { section: "capabilities" })
                        }
                      >
                        Open Capabilities Settings
                      </button>
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
                          forkSessionDisabled: isForkingConversation || conversationForkGate.gated,
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
                    sendDisabledReason={
                      capabilityBlockedReason ??
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
                    {...(!isFocusedChildChat &&
                    activeConversation.contextType === "project" &&
                    capabilityOptions.length > 1
                      ? {
                          capability: {
                            value: activeConversation.coordinationMode,
                            onValueChange: handleActiveCapabilitySelection,
                            options: capabilityOptions,
                            disabled:
                              composerProps.isReadOnly ||
                              isForkingConversation ||
                              Boolean(automationRunReadOnlyReason) ||
                              composerProps.agentStatus !== "idle",
                            pending:
                              updatingCapabilityConversationId ===
                              selectedConversationId,
                            testId: "agents-conversation-capability",
                          },
                        }
                      : {})}
                    {...(activeConversation.contextType === "project" &&
                    composerProps.persona !== undefined
                      ? { persona: composerProps.persona }
                      : activeConversation.contextType === "project" &&
                          composerProps.personaControl !== undefined
                        ? { personaControl: composerProps.personaControl }
                      : {})}
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
                            secondaryOptionIds: secondaryModeOptionIds,
                            // Workspace conversation owns mode; child chats
                            // inherit and display it read-only.
                            disabled:
                              isFocusedChildChat ||
                              Boolean(activeAutomationRunId) ||
                              composerProps.isSending ||
                              composerProps.agentStatus === "generating" ||
                              modeSwitchGate.gated ||
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
                              disabled: isForkingConversation || conversationForkGate.gated,
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
                    {...(chatFocus.type === "workspace"
                      ? {
                          runtimeDefault: {
                            source: roleDefaultQuery.data?.source ?? null,
                            isResetting: isResettingRoleDefault,
                            disabled: isResettingRoleDefault,
                            onReset: handleResetRoleDefault,
                          },
                        }
                      : {})}
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
                                supportedModelAliasesForProvider(
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
                              value: activeServiceTier === "fast",
                              onValueChange: (value) =>
                                handleActiveServiceTierChange(
                                  value ? "fast" : "standard",
                                ),
                              disabled:
                                !providerSettingsReady ||
                                !codexFastModeAvailability.supported,
                              description:
                                codexFastModeAvailability.reason ??
                                CODEX_FAST_MODE_DESCRIPTION,
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
                          ...(normalizedActiveRuntime.provider === "codex"
                            ? {
                                speed: {
                                  value: activeServiceTier,
                                  onValueChange: handleActiveServiceTierChange,
                                  options: [
                                    {
                                      id: "provider_default",
                                      label: "Provider default",
                                      description:
                                        "Use the service tier configured for Codex.",
                                    },
                                    {
                                      id: "standard",
                                      label: "Standard",
                                      description: "Use standard processing.",
                                    },
                                    {
                                      id: "fast",
                                      label: "Fast",
                                      description:
                                        codexFastModeAvailability.reason ??
                                        CODEX_FAST_MODE_DESCRIPTION,
                                      ...(!providerSettingsReady ||
                                      !codexFastModeAvailability.supported
                                        ? {
                                            disabled: true,
                                            disabledReason:
                                              codexFastModeAvailability.reason ??
                                              "Fast processing is unavailable.",
                                          }
                                        : {}),
                                    },
                                  ],
                                  testId: "agents-conversation-speed",
                                },
                              }
                            : {}),
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
                      standaloneCaption="Runs in a private workspace"
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
                isPublishingWorkspace={Boolean(
                  publishAttemptsByConversationId[selectedConversationId],
                )}
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
        conversationId={workspaceConversationId}
        workspace={activeWorkspace}
        terminalArchivedReason={terminalArchivedReason}
        terminalUnavailableReason={terminalUnavailableReason}
        hasAutoOpenArtifacts={hasAutoOpenArtifacts}
        setDockElement={setTerminalChatDockElement}
      />
      <ConfirmationDialog {...confirmationDialogProps} />
      <PlanContinuationDialog {...planContinuationDialogProps} />
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
