import { useCallback, useMemo, type ReactNode } from "react";
import {
  CheckCircle2,
  ExternalLink,
  Lightbulb,
  Loader2,
  MessageSquare,
  Pause,
  Play,
  Square,
  Workflow,
  type LucideIcon,
} from "lucide-react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";

import {
  automationsApi,
  type Automation,
  type AutomationRun,
  type AutomationRunMode,
} from "@/api/automations";
import * as chatApi from "@/api/chat";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { useAfterPaintMounted } from "@/components/agents/agentDeferredFrame";
import {
  describeAutomationStage,
  describeRunFailure,
  latestRun,
} from "@/components/automations/automationStage";
import {
  AUTOMATION_PHASES_LABEL,
  AUTOMATION_STATUS_LABELS,
  parseAutomationGoalItems,
} from "@/components/automations/automationGoalItems";
import { useArtifact } from "@/hooks/useArtifacts";
import {
  invalidateAutomationQueries,
  useAutomationDetail,
  useAutomationEvents,
} from "@/hooks/useAutomations";
import { useAskUserQuestion } from "@/hooks/useAskUserQuestion";
import { useAgentModels } from "@/hooks/useAgentModels";
import { useConfirmation } from "@/hooks/useConfirmation";
import { useHarnessProviders } from "@/hooks/useHarnessProviders";
import { withAlpha } from "@/lib/theme-colors";
import type {
  AgentEffort,
  AgentProvider,
  AgentRuntimeSelection,
} from "@/stores/agentSessionStore";
import type { AskUserQuestionPayload } from "@/types/ask-user-question";

import {
  AGENT_PROVIDER_OPTIONS,
  agentEffortOptions,
  type AgentEffortOption,
  type AgentModelOption,
  agentModelOptions,
  defaultEffortForModel,
  defaultModelForProvider,
  normalizeRuntimeSelection,
} from "./agentOptions";
import {
  buildAgentProviderAvailabilityOptions,
  supportedEffortsForProvider,
  supportedModelAliasesForProvider,
} from "./agentProviderAvailability";

interface AgentsAutomationPanelProps {
  automationId: string;
  conversationTitle?: string | null;
  onOpenAutomation?: (automationId: string) => void;
}

const AUTOMATION_SETUP_PROPOSAL_KIND = "automation_setup_proposal";
const AUTOMATION_SETUP_PROPOSAL_APPLY_VALUE = "apply_automation_proposal";
const UPDATE_AUTOMATION_FROM_LATEST_PROPOSAL_PROMPT =
  "The user clicked Update automation in the Automation artifact. Update the bound draft automation now with the goal, phases, setup summary, first-run prompt, run mode, provider/model, and base from your latest Automation proposal. Call get_automation if needed, then call update_automation with the accepted proposal. Do not finalize, run, or activate the automation.";
const OPEN_RUN_STATUSES = new Set<AutomationRun["status"]>([
  "pending",
  "provisioning",
  "running",
  "published",
]);
const AUTOMATION_RUN_MODE_OPTIONS: Array<{
  id: AutomationRunMode;
  label: string;
  description: string;
  icon: LucideIcon;
}> = [
  {
    id: "edit",
    label: "Build",
    description: "Builds scoped PRs and publishes them.",
    icon: Workflow,
  },
  {
    id: "plan",
    label: "Plan",
    description: "Creates or refines implementation plans.",
    icon: MessageSquare,
  },
  {
    id: "ideation",
    label: "Ideation",
    description: "Runs exploration and proposal workflows.",
    icon: Lightbulb,
  },
];

function formatBase(automation: Automation): string {
  return (automation.baseDisplayName ?? automation.baseRef) || automation.baseRefKind;
}

function formatModel(automation: Automation): string {
  const effort = automation.logicalEffort ? `/${automation.logicalEffort}` : "";
  return `${automation.providerHarness}/${automation.modelId}${effort}`;
}

function formatAutomationRunModeLabel(runMode: AutomationRunMode): string {
  return (
    AUTOMATION_RUN_MODE_OPTIONS.find((option) => option.id === runMode)?.label ??
    runMode
  );
}

function completionSignalForRunMode(runMode: AutomationRunMode) {
  return runMode === "edit" ? "pr_merged" : "agent_completed";
}

function automationProposalApplyOptionIndex(
  question: AskUserQuestionPayload | null | undefined,
): number {
  if (!question) {
    return -1;
  }

  const optionIndex = question.options.findIndex(
    (option) => option.value === AUTOMATION_SETUP_PROPOSAL_APPLY_VALUE,
  );
  if (optionIndex < 0) {
    return -1;
  }

  if (question.metadata?.kind === AUTOMATION_SETUP_PROPOSAL_KIND) {
    return optionIndex;
  }

  const header = question.header?.toLowerCase() ?? "";
  return header.includes("automation") ? optionIndex : -1;
}

function automationProviderFromValue(value: string): AgentProvider {
  return value === "codex" ? "codex" : "claude";
}

function automationEffortFromValue(value: string | null): AgentEffort | undefined {
  return value === "low" ||
    value === "medium" ||
    value === "high" ||
    value === "xhigh" ||
    value === "max"
    ? value
    : undefined;
}

function automationRuntimeFromConfig(automation: Automation): AgentRuntimeSelection {
  return {
    provider: automationProviderFromValue(automation.providerHarness),
    modelId: automation.modelId,
    effort: automationEffortFromValue(automation.logicalEffort) ?? "medium",
  };
}

function automationDisplayName(
  automation: Automation,
  conversationTitle?: string | null,
): string {
  const name = automation.name.trim();
  if (name && name.toLowerCase() !== "untitled automation") {
    return name;
  }
  const title = conversationTitle?.trim();
  return title || "Automation setup";
}

function formatRunSummary(run: AutomationRun | null, maxRuns: number): string {
  if (!run) {
    return `0 of ${maxRuns}`;
  }
  return `${run.runIndex} of ${maxRuns}`;
}

function formatPrState(run: AutomationRun | null): string {
  if (!run) {
    return "No PR yet";
  }
  const status = OPEN_RUN_STATUSES.has(run.status) ? "Running" : run.status;
  if (!run.prNumber) {
    return status;
  }
  return `PR #${run.prNumber} · ${status}`;
}

function PanelShell() {
  return (
    <div className="space-y-4 p-5" data-testid="agents-automation-panel-loading">
      <Skeleton className="h-5 w-40" />
      <div
        className="grid gap-3 rounded-md p-4"
        style={{
          backgroundColor: "var(--bg-surface)",
          borderColor: "var(--border-default)",
          borderStyle: "solid",
          borderWidth: "1px",
        }}
      >
        <Skeleton className="h-4 w-28" />
        <Skeleton className="h-4 w-44" />
        <Skeleton className="h-4 w-36" />
      </div>
      <div className="flex gap-2">
        <Skeleton className="h-9 w-24" />
        <Skeleton className="h-9 w-24" />
      </div>
    </div>
  );
}

export function AgentsAutomationPanel({
  automationId,
  conversationTitle,
  onOpenAutomation,
}: AgentsAutomationPanelProps) {
  const afterPaint = useAfterPaintMounted(Boolean(automationId));
  const detail = useAutomationDetail(automationId, { enabled: afterPaint });
  const queryClient = useQueryClient();
  const { confirm, confirmationDialogProps, ConfirmationDialog } = useConfirmation();
  const { registry: modelRegistry } = useAgentModels();
  const {
    providers: configuredProviders,
    isLoading: isLoadingProviderSettings,
    isPlaceholderData: isPlaceholderProviderSettings,
  } = useHarnessProviders({ refreshRuntime: true });
  useAutomationEvents(automationId);
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
  const automationForRuntime = detail.data?.automation ?? null;
  const automationSetupQuestionSessionId =
    automationForRuntime?.setupConversationId ?? undefined;
  const {
    activeQuestion: activeAutomationSetupQuestion,
    submitAnswer: submitAutomationSetupAnswer,
    isLoading: isSubmittingAutomationSetupAnswer,
  } = useAskUserQuestion(automationSetupQuestionSessionId);
  const automationProposalOptionIndex = automationProposalApplyOptionIndex(
    activeAutomationSetupQuestion,
  );
  const selectableAutomationRuntime = useMemo<AgentRuntimeSelection | null>(() => {
    if (!automationForRuntime) {
      return null;
    }
    const provider = automationProviderFromValue(
      automationForRuntime.providerHarness,
    );
    return normalizeRuntimeSelection(
      automationRuntimeFromConfig(automationForRuntime),
      modelRegistry,
      supportedEffortsForProvider(providerOptions, provider),
      supportedModelAliasesForProvider(providerOptions, provider),
    );
  }, [automationForRuntime, modelRegistry, providerOptions]);
  const automationModelOptions = useMemo(
    () =>
      selectableAutomationRuntime
        ? agentModelOptions(
            selectableAutomationRuntime.provider,
            modelRegistry,
            supportedModelAliasesForProvider(
              providerOptions,
              selectableAutomationRuntime.provider,
            ),
          )
        : [],
    [modelRegistry, providerOptions, selectableAutomationRuntime],
  );
  const automationEffortOptions = useMemo(
    () =>
      selectableAutomationRuntime
        ? agentEffortOptions(
            selectableAutomationRuntime.provider,
            selectableAutomationRuntime.modelId,
            modelRegistry,
            supportedEffortsForProvider(
              providerOptions,
              selectableAutomationRuntime.provider,
            ),
          )
        : [],
    [modelRegistry, providerOptions, selectableAutomationRuntime],
  );

  const invalidate = useCallback(() => {
    invalidateAutomationQueries(queryClient, automationId);
  }, [automationId, queryClient]);
  const updateSetupMutation = useMutation({
    mutationFn: ({
      conversationId,
      input,
    }: {
      conversationId: string;
      input: Parameters<typeof automationsApi.setupAgent.updateAutomation>[1];
    }) => automationsApi.setupAgent.updateAutomation(conversationId, input),
    onSuccess: invalidate,
    onError: () => toast.error("Failed to update automation setup"),
  });
  const requestSpecUpdateMutation = useMutation({
    mutationFn: (automation: Automation) =>
      chatApi.sendAgentMessage(
        "project",
        automation.projectId,
        UPDATE_AUTOMATION_FROM_LATEST_PROPOSAL_PROMPT,
        undefined,
        undefined,
        {
          conversationId: automation.setupConversationId,
          providerHarness: automation.providerHarness,
          modelId: automation.modelId,
          logicalEffort: automation.logicalEffort,
        },
      ),
    onSuccess: () => {
      invalidate();
      toast.success("Automation update requested");
    },
    onError: () => toast.error("Failed to request automation update"),
  });

  const pauseMutation = useMutation({
    mutationFn: () =>
      automationsApi.pause({
        id: automationId,
        reasonCode: "user",
        reasonDetail: "Paused from Agents automation panel",
      }),
    onSuccess: () => {
      invalidate();
      toast.success("Automation paused");
    },
    onError: () => toast.error("Failed to pause automation"),
  });
  const resumeMutation = useMutation({
    mutationFn: () => automationsApi.resume(automationId),
    onSuccess: () => {
      invalidate();
      toast.success("Automation resumed");
    },
    onError: () => toast.error("Failed to resume automation"),
  });
  const stopMutation = useMutation({
    mutationFn: () => automationsApi.stop(automationId),
    onSuccess: () => {
      invalidate();
      toast.success("Automation stopped");
    },
    onError: () => toast.error("Failed to stop automation"),
  });
  const handleAutomationRunModeChange = useCallback(
    (runMode: AutomationRunMode) => {
      const setupConversationId = automationForRuntime?.setupConversationId;
      if (
        !setupConversationId ||
        !automationForRuntime ||
        automationForRuntime.status !== "draft" ||
        automationForRuntime.runMode === runMode ||
        updateSetupMutation.isPending
      ) {
        return;
      }
      updateSetupMutation.mutate(
        {
          conversationId: setupConversationId,
          input: {
            runMode,
            completionSignal: completionSignalForRunMode(runMode),
          },
        },
        {
          onSuccess: () =>
            toast.success(
              `Automation will run as ${formatAutomationRunModeLabel(runMode)}`,
            ),
        },
      );
    },
    [automationForRuntime, updateSetupMutation],
  );
  const updateAutomationRuntime = useCallback(
    (runtime: AgentRuntimeSelection) => {
      const setupConversationId = automationForRuntime?.setupConversationId;
      if (
        !setupConversationId ||
        !automationForRuntime ||
        automationForRuntime.status !== "draft" ||
        updateSetupMutation.isPending
      ) {
        return;
      }
      updateSetupMutation.mutate(
        {
          conversationId: setupConversationId,
          input: {
            providerHarness: runtime.provider,
            modelId: runtime.modelId,
            logicalEffort: runtime.effort,
          },
        },
        {
          onSuccess: () => toast.success("Automation run agent updated"),
        },
      );
    },
    [automationForRuntime, updateSetupMutation],
  );
  const handleApplyAutomationProposal = useCallback(async () => {
    if (
      !activeAutomationSetupQuestion ||
      automationProposalOptionIndex < 0 ||
      isSubmittingAutomationSetupAnswer
    ) {
      return;
    }
    const option = activeAutomationSetupQuestion.options[automationProposalOptionIndex];
    if (!option) {
      return;
    }
    const result = await submitAutomationSetupAnswer({
      requestId: activeAutomationSetupQuestion.requestId,
      ...(activeAutomationSetupQuestion.taskId
        ? { taskId: activeAutomationSetupQuestion.taskId }
        : {}),
      selectedOptions: [
        option.value ?? option.label ?? AUTOMATION_SETUP_PROPOSAL_APPLY_VALUE,
      ],
    });
    if (result.success) {
      invalidate();
      toast.success("Automation update accepted");
    }
  }, [
    activeAutomationSetupQuestion,
    automationProposalOptionIndex,
    invalidate,
    isSubmittingAutomationSetupAnswer,
    submitAutomationSetupAnswer,
  ]);
  const handleRequestAutomationSpecUpdate = useCallback(() => {
    if (
      !automationForRuntime?.setupConversationId ||
      automationForRuntime.status !== "draft" ||
      requestSpecUpdateMutation.isPending
    ) {
      return;
    }
    requestSpecUpdateMutation.mutate(automationForRuntime);
  }, [automationForRuntime, requestSpecUpdateMutation]);
  const handleAutomationProviderChange = useCallback(
    (provider: AgentProvider) => {
      if (!selectableAutomationRuntime) {
        return;
      }
      const modelId = defaultModelForProvider(provider, modelRegistry);
      const nextRuntime = normalizeRuntimeSelection(
        {
          provider,
          modelId,
          effort: defaultEffortForModel(provider, modelId, modelRegistry),
        },
        modelRegistry,
        supportedEffortsForProvider(providerOptions, provider),
        supportedModelAliasesForProvider(providerOptions, provider),
      );
      updateAutomationRuntime(nextRuntime);
    },
    [
      modelRegistry,
      providerOptions,
      selectableAutomationRuntime,
      updateAutomationRuntime,
    ],
  );
  const handleAutomationModelChange = useCallback(
    (modelId: string) => {
      if (!selectableAutomationRuntime) {
        return;
      }
      const provider = selectableAutomationRuntime.provider;
      const nextRuntime = normalizeRuntimeSelection(
        {
          ...selectableAutomationRuntime,
          modelId,
        },
        modelRegistry,
        supportedEffortsForProvider(providerOptions, provider),
        supportedModelAliasesForProvider(providerOptions, provider),
      );
      updateAutomationRuntime(nextRuntime);
    },
    [
      modelRegistry,
      providerOptions,
      selectableAutomationRuntime,
      updateAutomationRuntime,
    ],
  );
  const handleAutomationEffortChange = useCallback(
    (effort: AgentEffort) => {
      if (!selectableAutomationRuntime) {
        return;
      }
      updateAutomationRuntime({
        ...selectableAutomationRuntime,
        effort,
      });
    },
    [selectableAutomationRuntime, updateAutomationRuntime],
  );

  const handleStop = async () => {
    const confirmed = await confirm({
      title: "Stop automation?",
      description: "Stopping is terminal for this automation.",
      confirmText: "Stop",
      pendingText: "Stopping...",
      variant: "destructive",
    });
    if (confirmed) {
      stopMutation.mutate();
    }
  };

  if (!afterPaint || detail.isLoading) {
    return <PanelShell />;
  }

  if (detail.isError || !detail.data) {
    return (
      <div className="p-5 text-sm" style={{ color: "var(--status-error)" }}>
        Could not load automation.
      </div>
    );
  }

  const { automation, runs } = detail.data;
  const displayName = automationDisplayName(automation, conversationTitle);
  const run = latestRun(runs);
  const goalItems = parseAutomationGoalItems(automation.goalItemsJson);
  const stage = describeAutomationStage(automation, run);
  const failureReason = describeRunFailure(run);
  const showPausedReason =
    !failureReason && automation.status === "paused" && Boolean(automation.pausedReasonCode);
  const actionPending =
    pauseMutation.isPending || resumeMutation.isPending || stopMutation.isPending;
  const canPause = automation.status === "active";
  const canResume = automation.status === "paused";
  const canStop = automation.status !== "completed" && automation.status !== "stopped";
  const setupConversationId = automation.setupConversationId;
  const setupControlsDisabled =
    automation.status !== "draft" || !setupConversationId;
  const setupDisabledReason = !setupConversationId
    ? "Setup conversation link is missing, so settings cannot be updated here."
    : automation.status !== "draft"
      ? "Approved automation settings are read-only."
      : null;
  const showAutomationProposalCta =
    automation.status === "draft" &&
    Boolean(activeAutomationSetupQuestion) &&
    automationProposalOptionIndex >= 0;
  const showMissingSpecCta =
    automation.status === "draft" &&
    !showAutomationProposalCta &&
    Boolean(setupConversationId) &&
    (!automation.goalPrompt.trim() || goalItems.length === 0);

  return (
    <div className="space-y-4 p-5" data-testid="agents-automation-panel">
      <div className="flex items-start gap-3">
        <div
          className="grid h-9 w-9 shrink-0 place-items-center rounded-md"
          style={{ backgroundColor: withAlpha("var(--accent-primary)", 14) }}
          aria-hidden="true"
        >
          <Workflow className="h-5 w-5" style={{ color: "var(--accent-primary)" }} />
        </div>
        <div className="min-w-0">
          <h2 className="truncate text-sm font-semibold" style={{ color: "var(--text-primary)" }}>
            {displayName}
          </h2>
          <p className="mt-1 text-xs" style={{ color: "var(--text-muted)" }}>
            Automation-owned conversation
          </p>
        </div>
      </div>

      <div
        className="grid gap-3 rounded-md p-4 text-sm"
        style={{
          backgroundColor: "var(--bg-surface)",
          borderColor: "var(--border-default)",
          borderStyle: "solid",
          borderWidth: "1px",
        }}
      >
        <SummaryRow label="Status" value={AUTOMATION_STATUS_LABELS[automation.status]} />
        <SummaryRow label="Stage" value={stage} testId="agents-automation-stage" />
        <SummaryRow label="Run type" value={automation.runMode} />
        <SummaryRow label="Model" value={formatModel(automation)} />
        <SummaryRow label="Base" value={formatBase(automation)} />
        <SummaryRow label="Run" value={formatRunSummary(run, automation.maxRuns)} />
        <SummaryRow label="Current PR" value={formatPrState(run)} />
      </div>

      {showAutomationProposalCta && activeAutomationSetupQuestion ? (
        <AutomationProposalCallout
          header={activeAutomationSetupQuestion.header ?? "Automation update"}
          description={activeAutomationSetupQuestion.question}
          isPending={isSubmittingAutomationSetupAnswer}
          onApply={handleApplyAutomationProposal}
        />
      ) : showMissingSpecCta ? (
        <AutomationProposalCallout
          header="Automation spec not saved"
          description="Save the latest proposed goal and phases into this automation artifact."
          isPending={requestSpecUpdateMutation.isPending}
          pendingLabel="Requesting..."
          onApply={handleRequestAutomationSpecUpdate}
        />
      ) : null}

      <DetailSection title="Setup" testId="agents-automation-setup">
        <div className="space-y-4">
          <p className="text-xs leading-5" style={{ color: "var(--text-muted)" }}>
            Automation setup - draft and approve this automation spec by chatting
            with the setup agent.
          </p>
          {selectableAutomationRuntime ? (
            <AutomationRuntimeSelector
              runtime={selectableAutomationRuntime}
              providerOptions={
                providerOptions.length > 0 ? providerOptions : AGENT_PROVIDER_OPTIONS
              }
              modelOptions={automationModelOptions}
              effortOptions={automationEffortOptions}
              disabled={setupControlsDisabled}
              isUpdating={updateSetupMutation.isPending}
              onProviderChange={handleAutomationProviderChange}
              onModelChange={handleAutomationModelChange}
              onEffortChange={handleAutomationEffortChange}
            />
          ) : null}
          <AutomationRunModeSelector
            runMode={automation.runMode}
            disabled={setupControlsDisabled}
            isUpdating={updateSetupMutation.isPending}
            onChange={handleAutomationRunModeChange}
          />
          {setupDisabledReason ? (
            <p className="text-xs leading-5" style={{ color: "var(--text-muted)" }}>
              {setupDisabledReason}
            </p>
          ) : null}
        </div>
      </DetailSection>

      <DetailSection title="Goal" testId="agents-automation-goal">
        <p className="text-xs leading-5" style={{ color: "var(--text-primary)" }}>
          {automation.goalPrompt.trim() || "No goal configured yet."}
        </p>
      </DetailSection>

      <DetailSection title={AUTOMATION_PHASES_LABEL} testId="agents-automation-phases">
        {goalItems.length > 0 ? (
          <div className="space-y-2">
            {goalItems.map((item, index) => (
              <div
                key={`${item.id}:${index}`}
                className="grid grid-cols-[minmax(0,1fr)_auto] gap-3 text-xs"
              >
                <span
                  className="min-w-0 truncate font-medium"
                  style={{ color: "var(--text-primary)" }}
                >
                  {item.title}
                </span>
                <span style={{ color: "var(--text-muted)" }}>{item.status}</span>
              </div>
            ))}
          </div>
        ) : (
          <p className="text-xs" style={{ color: "var(--text-muted)" }}>
            No phases configured yet.
          </p>
        )}
      </DetailSection>

      <AutomationSpecSection specArtifactId={automation.specArtifactId} />

      {automation.setupAnalysisSummary ? (
        <DetailSection title="Setup summary" testId="agents-automation-setup-summary">
          <p className="text-xs leading-5" style={{ color: "var(--text-primary)" }}>
            {automation.setupAnalysisSummary}
          </p>
        </DetailSection>
      ) : null}

      <DetailSection title="First run" testId="agents-automation-first-run">
        <p className="text-xs leading-5" style={{ color: "var(--text-primary)" }}>
          {automation.firstRunPrompt?.trim() || "No first run prompt configured yet."}
        </p>
      </DetailSection>

      {failureReason ? (
        <div
          className="rounded-md px-3 py-2 text-xs font-medium"
          style={{
            backgroundColor: "var(--bg-surface)",
            borderColor: "var(--border-default)",
            borderStyle: "solid",
            borderWidth: "1px",
            color: "var(--status-error)",
          }}
          data-testid="agents-automation-failure"
        >
          {failureReason}
        </div>
      ) : showPausedReason ? (
        <div
          className="rounded-md px-3 py-2 text-xs"
          style={{
            backgroundColor: "var(--bg-hover)",
            color: "var(--text-secondary)",
          }}
          data-testid="agents-automation-paused"
        >
          Paused: {automation.pausedReasonCode}
          {automation.pausedReasonDetail ? ` - ${automation.pausedReasonDetail}` : ""}
        </div>
      ) : null}

      <div className="flex flex-wrap gap-2">
        {canPause ? (
          <Button
            type="button"
            variant="secondary"
            size="sm"
            className="gap-2"
            disabled={actionPending}
            onClick={() => pauseMutation.mutate()}
            data-testid="agents-automation-pause"
          >
            <Pause className="h-4 w-4" />
            Pause
          </Button>
        ) : null}
        {canResume ? (
          <Button
            type="button"
            variant="secondary"
            size="sm"
            className="gap-2"
            disabled={actionPending}
            onClick={() => resumeMutation.mutate()}
            data-testid="agents-automation-resume"
          >
            <Play className="h-4 w-4" />
            Resume
          </Button>
        ) : null}
        {canStop ? (
          <Button
            type="button"
            variant="outline"
            size="sm"
            className="gap-2"
            disabled={actionPending}
            onClick={handleStop}
            data-testid="agents-automation-stop"
          >
            <Square className="h-4 w-4" />
            Stop
          </Button>
        ) : null}
        <Button
          type="button"
          size="sm"
          className="gap-2"
          disabled={!onOpenAutomation}
          onClick={() => onOpenAutomation?.(automation.id)}
          data-testid="agents-automation-open"
        >
          <ExternalLink className="h-4 w-4" />
          Open automation
        </Button>
      </div>
      <ConfirmationDialog {...confirmationDialogProps} />
    </div>
  );
}

function AutomationProposalCallout({
  header,
  description,
  isPending,
  pendingLabel = "Applying...",
  onApply,
}: {
  header: string;
  description: string;
  isPending: boolean;
  pendingLabel?: string;
  onApply: () => void;
}) {
  return (
    <section
      className="rounded-md p-4"
      style={{
        backgroundColor: "var(--accent-muted)",
        borderColor: "var(--accent-border)",
        borderStyle: "solid",
        borderWidth: "1px",
      }}
      data-testid="agents-automation-proposal-cta"
    >
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0 flex-1">
          <h3
            className="text-xs font-semibold uppercase tracking-[0.08em]"
            style={{ color: "var(--accent-primary)" }}
          >
            {header}
          </h3>
          <p className="mt-2 text-xs leading-5" style={{ color: "var(--text-primary)" }}>
            {description}
          </p>
        </div>
        <Button
          type="button"
          size="sm"
          className="gap-2"
          disabled={isPending}
          onClick={onApply}
          data-testid="agents-automation-proposal-update"
        >
          {isPending ? (
            <Loader2 className="h-4 w-4 animate-spin" />
          ) : (
            <CheckCircle2 className="h-4 w-4" />
          )}
          {isPending ? pendingLabel : "Update automation"}
        </Button>
      </div>
    </section>
  );
}

function SummaryRow({
  label,
  value,
  testId,
}: {
  label: string;
  value: string;
  testId?: string;
}) {
  return (
    <div
      className="grid grid-cols-[96px_minmax(0,1fr)] gap-3"
      {...(testId ? { "data-testid": testId } : {})}
    >
      <span className="text-xs font-medium" style={{ color: "var(--text-muted)" }}>
        {label}
      </span>
      <span className="min-w-0 truncate text-xs font-semibold" style={{ color: "var(--text-primary)" }}>
        {value}
      </span>
    </div>
  );
}

function specPreview(content: string): string {
  const trimmed = content.trim();
  const preview = trimmed.split(/\r?\n/).slice(0, 8).join("\n");
  return preview.length > 600 ? `${preview.slice(0, 600)}...` : preview;
}

function AutomationSpecSection({
  specArtifactId,
}: {
  specArtifactId: string | null;
}) {
  // `useArtifact` no-ops on an empty id (`enabled: !!id`).
  const artifact = useArtifact(specArtifactId ?? "");
  const data = specArtifactId ? artifact.data : null;

  return (
    <DetailSection title="Spec" testId="agents-automation-spec">
      {data ? (
        <div className="space-y-1.5">
          <p
            className="truncate text-xs font-semibold"
            style={{ color: "var(--text-primary)" }}
          >
            {data.name}
          </p>
          <p
            className="whitespace-pre-wrap break-words text-xs leading-5"
            style={{ color: "var(--text-muted)" }}
          >
            {specPreview(data.content) || "Spec has no content yet."}
          </p>
        </div>
      ) : (
        <p className="text-xs" style={{ color: "var(--text-muted)" }}>
          No spec linked yet.
        </p>
      )}
    </DetailSection>
  );
}

function DetailSection({
  title,
  children,
  testId,
}: {
  title: string;
  children: ReactNode;
  testId?: string;
}) {
  return (
    <section
      className="rounded-md p-4"
      style={{
        backgroundColor: "var(--bg-surface)",
        borderColor: "var(--border-default)",
        borderStyle: "solid",
        borderWidth: "1px",
      }}
      {...(testId ? { "data-testid": testId } : {})}
    >
      <h3
        className="mb-3 text-xs font-semibold uppercase tracking-[0.08em]"
        style={{ color: "var(--text-muted)" }}
      >
        {title}
      </h3>
      {children}
    </section>
  );
}

function AutomationRunModeSelector({
  runMode,
  disabled,
  isUpdating,
  onChange,
}: {
  runMode: AutomationRunMode;
  disabled: boolean;
  isUpdating: boolean;
  onChange: (runMode: AutomationRunMode) => void;
}) {
  return (
    <div
      className="flex flex-col gap-2"
      data-testid="agents-automation-run-mode-selector"
    >
      <div className="flex flex-wrap items-end justify-between gap-2">
        <div>
          <span
            className="block text-[0.6875rem] font-semibold uppercase tracking-[0.08em]"
            style={{ color: "var(--text-muted)" }}
          >
            Run type
          </span>
          <span className="text-xs" style={{ color: "var(--text-muted)" }}>
            Choose which agent mode future runs use.
          </span>
        </div>
        {isUpdating ? (
          <span
            className="inline-flex items-center gap-1.5 text-xs"
            style={{ color: "var(--text-muted)" }}
          >
            <Loader2 className="h-3.5 w-3.5 animate-spin" aria-hidden="true" />
            Saving
          </span>
        ) : null}
      </div>
      <div
        className="inline-flex w-fit flex-wrap rounded-md border p-1"
        style={{
          backgroundColor: "var(--bg-base)",
          borderColor: "var(--border-subtle)",
          borderStyle: "solid",
          borderWidth: "1px",
        }}
        role="group"
        aria-label="Automation run type"
      >
        {AUTOMATION_RUN_MODE_OPTIONS.map((option) => {
          const selected = option.id === runMode;
          const Icon = option.icon;
          return (
            <button
              key={option.id}
              type="button"
              disabled={disabled || isUpdating || selected}
              onClick={() => onChange(option.id)}
              className="inline-flex h-8 items-center gap-2 rounded px-2.5 text-xs font-medium outline-none transition-colors disabled:cursor-not-allowed disabled:opacity-60 focus-visible:ring-2 focus-visible:ring-[var(--accent-primary)]"
              style={{
                backgroundColor: selected
                  ? "var(--bg-surface-hover)"
                  : "transparent",
                color: selected ? "var(--text-primary)" : "var(--text-muted)",
              }}
              aria-pressed={selected}
              data-testid={`agents-automation-run-mode-${option.id}`}
            >
              <Icon
                className="h-3.5 w-3.5 shrink-0"
                style={{
                  color: selected
                    ? "var(--accent-primary)"
                    : "var(--text-muted)",
                }}
                aria-hidden="true"
              />
              <span>{option.label}</span>
            </button>
          );
        })}
      </div>
      <p className="text-xs leading-5" style={{ color: "var(--text-muted)" }}>
        {
          AUTOMATION_RUN_MODE_OPTIONS.find((option) => option.id === runMode)
            ?.description
        }
      </p>
    </div>
  );
}

function AutomationRuntimeSelector({
  runtime,
  providerOptions,
  modelOptions,
  effortOptions,
  disabled,
  isUpdating,
  onProviderChange,
  onModelChange,
  onEffortChange,
}: {
  runtime: AgentRuntimeSelection;
  providerOptions: readonly {
    id: AgentProvider;
    label: string;
    disabled?: boolean;
    disabledReason?: string;
  }[];
  modelOptions: readonly AgentModelOption[];
  effortOptions: readonly AgentEffortOption[];
  disabled: boolean;
  isUpdating: boolean;
  onProviderChange: (provider: AgentProvider) => void;
  onModelChange: (modelId: string) => void;
  onEffortChange: (effort: AgentEffort) => void;
}) {
  const controlDisabled = disabled || isUpdating;
  const selectedProvider = providerOptions.find(
    (option) => option.id === runtime.provider,
  );
  const selectedProviderDisabledReason =
    selectedProvider?.disabledReason && !disabled
      ? selectedProvider.disabledReason
      : null;

  return (
    <div
      className="flex flex-col gap-2"
      data-testid="agents-automation-runtime-selector"
    >
      <div className="flex flex-wrap items-end justify-between gap-2">
        <div>
          <span
            className="block text-[0.6875rem] font-semibold uppercase tracking-[0.08em]"
            style={{ color: "var(--text-muted)" }}
          >
            Run agent
          </span>
          <span className="text-xs" style={{ color: "var(--text-muted)" }}>
            Provider, model, and effort future automation runs use.
          </span>
        </div>
        {isUpdating ? (
          <span
            className="inline-flex items-center gap-1.5 text-xs"
            style={{ color: "var(--text-muted)" }}
          >
            <Loader2 className="h-3.5 w-3.5 animate-spin" aria-hidden="true" />
            Saving
          </span>
        ) : null}
      </div>
      <div className="grid gap-2 sm:grid-cols-[minmax(7rem,0.8fr)_minmax(9rem,1fr)_minmax(7rem,0.75fr)]">
        <AutomationSelect
          label="Provider"
          value={runtime.provider}
          disabled={controlDisabled}
          testId="agents-automation-provider"
          onChange={(value) => onProviderChange(value as AgentProvider)}
          options={providerOptions.map((option) => ({
            value: option.id,
            label: option.label,
            ...(option.disabled !== undefined
              ? { disabled: option.disabled }
              : {}),
          }))}
        />
        <AutomationSelect
          label="Model"
          value={runtime.modelId}
          disabled={controlDisabled}
          testId="agents-automation-model"
          onChange={onModelChange}
          options={modelOptions.map((option) => ({
            value: option.id,
            label: option.label,
          }))}
        />
        <AutomationSelect
          label="Effort"
          value={runtime.effort}
          disabled={controlDisabled}
          testId="agents-automation-effort"
          onChange={(value) => onEffortChange(value as AgentEffort)}
          options={effortOptions.map((option) => ({
            value: option.id,
            label: option.label,
          }))}
        />
      </div>
      {selectedProviderDisabledReason ? (
        <p className="text-xs leading-5" style={{ color: "var(--text-muted)" }}>
          {selectedProviderDisabledReason}
        </p>
      ) : null}
    </div>
  );
}

function AutomationSelect({
  label,
  value,
  options,
  disabled,
  testId,
  onChange,
}: {
  label: string;
  value: string;
  options: readonly { value: string; label: string; disabled?: boolean }[];
  disabled: boolean;
  testId: string;
  onChange: (value: string) => void;
}) {
  return (
    <label className="flex min-w-0 flex-col gap-1">
      <span
        className="text-[0.6875rem] font-medium uppercase tracking-[0.06em]"
        style={{ color: "var(--text-muted)" }}
      >
        {label}
      </span>
      <select
        value={value}
        disabled={disabled}
        onChange={(event) => onChange(event.currentTarget.value)}
        className="h-8 min-w-0 rounded-md border px-2 text-xs outline-none transition-colors focus-visible:ring-2 focus-visible:ring-[var(--accent-primary)] disabled:cursor-not-allowed disabled:opacity-60"
        style={{
          backgroundColor: "var(--bg-base)",
          borderColor: "var(--border-subtle)",
          borderStyle: "solid",
          borderWidth: "1px",
          color: "var(--text-primary)",
        }}
        data-testid={testId}
      >
        {options.map((option) => (
          <option
            key={option.value}
            value={option.value}
            disabled={option.disabled}
          >
            {option.label}
          </option>
        ))}
      </select>
    </label>
  );
}
