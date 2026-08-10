import { useCallback, useMemo, useState, type ReactNode } from "react";
import { useAgentGate } from "@/hooks/useAgentGate";
import {
  CheckCircle2,
  ExternalLink,
  Lightbulb,
  Loader2,
  MessageSquare,
  Pause,
  Play,
  RotateCcw,
  Square,
  Trash2,
  Workflow,
  type LucideIcon,
} from "lucide-react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";

import {
  automationsApi,
  type Automation,
  type AutomationPlanApprovalMode,
  type AutomationPrMergeMode,
  type AutomationRun,
  type AutomationRunMode,
  type UpdateAutomationSettingsInput,
} from "@/api/automations";
import * as chatApi from "@/api/chat";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { Switch } from "@/components/ui/switch";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { useAfterPaintMounted } from "@/components/agents/agentDeferredFrame";
import {
  AUTOMATION_CANCEL_CONFIRMATION_DESCRIPTION,
  CANCELLED_RUN_RESTART_DESCRIPTION,
  describeAutomationDeleteConsequences,
  describeRunFailure,
  getAutomationJudgeRecovery,
  getAutomationRunView,
  isIdleAfterCancelledRun,
  latestRun,
  runTimelineHighlight,
  type AutomationRunStatusTone,
} from "@/components/automations/automationStage";
import {
  AUTOMATION_PHASES_LABEL,
  AUTOMATION_STATUS_LABELS,
  findInProgressAutomationGoalItemFromItems,
  parseAutomationGoalItems,
  type AutomationGoalItem,
} from "@/components/automations/automationGoalItems";
import { AutomationPhaseProgress } from "@/components/automations/AutomationPhases";
import { AutomationRunPhaseChip } from "@/components/automations/AutomationRunPhaseChip";
import { AutomationRunPrLink } from "@/components/automations/AutomationRunPrLink";
import { AutomationSpecView } from "@/components/automations/AutomationSpecView";
import {
  evictDeletedAutomation,
  invalidateAutomationQueries,
  useAutomationDetail,
} from "@/hooks/useAutomations";
import { useAskUserQuestion } from "@/hooks/useAskUserQuestion";
import { useAgentModels } from "@/hooks/useAgentModels";
import { useConfirmation } from "@/hooks/useConfirmation";
import { useHarnessProviders } from "@/hooks/useHarnessProviders";
import { extractErrorMessage } from "@/lib/errors";
import { cn } from "@/lib/utils";
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
import {
  getAutomationRunFocusOptions,
  type AutomationRunFocusOptions,
} from "./agentChatFocus";

interface AgentsAutomationPanelProps {
  automationId: string;
  conversationTitle?: string | null;
  onOpenAutomation?: (automationId: string) => void;
  onFocusAutomationRun?: (
    automationId: string,
    runId: string,
    conversationId: string,
    options?: AutomationRunFocusOptions,
  ) => void;
}

const AUTOMATION_SETUP_PROPOSAL_KIND = "automation_setup_proposal";
const AUTOMATION_SETUP_PROPOSAL_APPLY_VALUE = "apply_automation_proposal";
const STACKED_CHAIN_MODE = "pr_head_stacked";
const AUTOMATION_STACKED_AUTO_MERGE_ERROR_CODE =
  "automation_stacked_auto_merge_unsupported";
const AUTOMATION_AUTO_MERGE_ENABLE_WARNING_CODE = "auto_merge_enable_failed";
const PLAN_GATE_PAUSED_REASON_CODES = new Set([
  "plan_revision_exhausted",
  "plan_judge_failed",
]);
type AutomationSettingsPatch = Omit<UpdateAutomationSettingsInput, "id">;
const UPDATE_AUTOMATION_FROM_LATEST_PROPOSAL_PROMPT =
  "The user clicked Update automation in the Automation artifact. Update the bound draft automation now with the goal, phases, setup summary, first-run prompt, run mode, provider/model, and base from your latest Automation proposal. Call get_automation if needed, then call update_automation with the accepted proposal. Do not finalize, run, or activate the automation.";
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

/** Tone color token for a run-status pill — success / warning / error / neutral. */
function runStatusToneClass(tone: AutomationRunStatusTone): string {
  switch (tone) {
    case "success":
      return "text-[var(--status-success)]";
    case "warning":
      return "text-[var(--status-warning)]";
    case "error":
      return "text-[var(--status-error)]";
    case "accent":
      return "text-[var(--accent-primary)]";
    case "neutral":
      return "text-[var(--text-secondary)]";
  }
}

/** Secondary line for a run row: PR link text or the prompt author. */
function runRowDetail(run: AutomationRun): string {
  if (run.prNumber) {
    return `PR #${run.prNumber}`;
  }
  return run.errorCode ? `Failed: ${run.errorCode}` : "No PR";
}

function runRowWarning(run: AutomationRun): string | null {
  if (
    run.status === "published" &&
    run.errorCode === AUTOMATION_AUTO_MERGE_ENABLE_WARNING_CODE &&
    run.errorDetail
  ) {
    return run.errorDetail;
  }
  return null;
}

function automationSettingsErrorToast(error: unknown): string {
  const message = extractErrorMessage(error, "");
  if (message.includes(AUTOMATION_STACKED_AUTO_MERGE_ERROR_CODE)) {
    return "Stacked PR chains require manual merge.";
  }
  return "Failed to update automation settings";
}

function planGatePausedCopy(pausedReasonCode: string | null): string {
  if (pausedReasonCode === "plan_judge_failed") {
    return "Plan judge failed — review and approve the plan to resume this automation.";
  }
  if (pausedReasonCode === "plan_revision_exhausted") {
    return "Plan revision limit reached — review and approve the plan to resume this automation.";
  }
  return "Review and approve the plan to resume this automation.";
}

/**
 * Inline editor for the automation's run budget (`maxRuns`). The backend only allows settings
 * edits while the automation is Draft or Paused, so this renders only in those states. It lets
 * a user extend an exhausted budget (e.g. after a `judge_stopped_unmet` pause) and then resume
 * to schedule another run, instead of being stuck with no way to continue.
 */
function MaxRunsEditor({
  currentMaxRuns,
  runsUsed,
  isSaving,
  onSave,
  gateReason,
}: {
  currentMaxRuns: number;
  runsUsed: number;
  isSaving: boolean;
  onSave: (maxRuns: number) => void;
  gateReason: string | null;
}) {
  const min = Math.max(1, runsUsed);
  const [value, setValue] = useState(String(currentMaxRuns));
  const parsed = Number.parseInt(value, 10);
  const isValid = Number.isFinite(parsed) && parsed >= min;
  const isChanged = parsed !== currentMaxRuns;
  return (
    <div
      className="mb-3 flex flex-wrap items-center gap-2"
      data-testid="agents-automation-max-runs"
    >
      <span className="text-xs font-medium" style={{ color: "var(--text-muted)" }}>
        Max runs
      </span>
      <input
        type="number"
        min={min}
        value={value}
        disabled={gateReason !== null}
        title={gateReason ?? undefined}
        onChange={(event) => setValue(event.target.value)}
        aria-label="Max runs"
        className="w-16 rounded px-2 py-1 text-xs outline-none focus:ring-0 focus:outline-none focus-visible:outline-none"
        style={{
          backgroundColor: "var(--bg-surface)",
          borderColor: "var(--border-default)",
          borderStyle: "solid",
          borderWidth: "1px",
          color: "var(--text-primary)",
          boxShadow: "none",
        }}
      />
      <Button
        type="button"
        variant="secondary"
        size="sm"
        disabled={!isValid || !isChanged || isSaving || gateReason !== null}
        title={gateReason ?? undefined}
        onClick={() => onSave(parsed)}
        data-testid="agents-automation-max-runs-save"
      >
        {isSaving ? "Saving..." : "Save"}
      </Button>
      <span className="text-xs" style={{ color: "var(--text-muted)" }}>
        {runsUsed} used
      </span>
    </div>
  );
}

function AutomationPlanGateSettings({
  automation,
  isSaving,
  pendingPatch,
  onUpdate,
  gateReason,
}: {
  automation: Automation;
  isSaving: boolean;
  pendingPatch: AutomationSettingsPatch | null;
  onUpdate: (patch: AutomationSettingsPatch) => void;
  gateReason: string | null;
}) {
  const planApprovalMode =
    pendingPatch?.planApprovalMode ?? automation.planApprovalMode;
  const prMergeMode = pendingPatch?.prMergeMode ?? automation.prMergeMode;
  const planDeepVerification =
    pendingPatch?.planDeepVerification ?? automation.planDeepVerification;
  const stackedChain = automation.chainMode === STACKED_CHAIN_MODE;
  const planApprovalSaving =
    isSaving && pendingPatch?.planApprovalMode !== undefined;
  const prMergeSaving = isSaving && pendingPatch?.prMergeMode !== undefined;
  const planDeepVerificationSaving =
    isSaving && pendingPatch?.planDeepVerification !== undefined;
  return (
    <div className="space-y-3" title={gateReason ?? undefined}>
      <div className="grid gap-2 sm:grid-cols-2">
        <AutomationSelect
          label="Plan approval"
          value={planApprovalMode}
          disabled={planApprovalSaving || gateReason !== null}
          testId="agents-automation-plan-approval-mode"
          onChange={(value) =>
            onUpdate({ planApprovalMode: value as AutomationPlanApprovalMode })
          }
          options={[
            { value: "manual", label: "Manual" },
            { value: "automatic", label: "Automatic (judge)" },
          ]}
        />
        <AutomationSelect
          label="PR merge"
          value={prMergeMode}
          disabled={prMergeSaving || stackedChain || gateReason !== null}
          testId="agents-automation-pr-merge-mode"
          onChange={(value) =>
            onUpdate({ prMergeMode: value as AutomationPrMergeMode })
          }
          options={[
            { value: "manual", label: "Manual" },
            { value: "automatic", label: "Automatic" },
          ]}
        />
      </div>
      {stackedChain ? (
        <p className="text-xs leading-5" style={{ color: "var(--text-muted)" }}>
          Stacked PR chains require manual merge.
        </p>
      ) : null}
      <label
        className="flex items-center justify-between gap-3 rounded-md px-3 py-2"
        style={{
          backgroundColor: "var(--bg-base)",
          borderColor: "var(--border-subtle)",
          borderStyle: "solid",
          borderWidth: "1px",
        }}
      >
        <span className="min-w-0">
          <span
            className="block text-xs font-medium"
            style={{ color: "var(--text-primary)" }}
          >
            Deep plan verification
          </span>
          <span className="block text-xs" style={{ color: "var(--text-muted)" }}>
            Adversarially verify each run plan before it can be approved.
          </span>
        </span>
        <Switch
          checked={planDeepVerification}
          disabled={planDeepVerificationSaving || gateReason !== null}
          title={gateReason ?? undefined}
          onCheckedChange={(checked) =>
            onUpdate({ planDeepVerification: checked })
          }
          className="data-[state=checked]:bg-[var(--accent-primary)] data-[state=unchecked]:bg-[var(--bg-elevated)]"
          data-testid="agents-automation-plan-deep-verification"
          aria-label="Deep plan verification"
        />
      </label>
      {isSaving ? (
        <span className="inline-flex items-center gap-1.5 text-xs" style={{ color: "var(--text-muted)" }}>
          <Loader2 className="h-3.5 w-3.5 animate-spin" aria-hidden="true" />
          Saving
        </span>
      ) : null}
      {gateReason ? (
        <p className="text-xs" style={{ color: "var(--text-muted)" }}>
          {gateReason}
        </p>
      ) : null}
    </div>
  );
}

/**
 * Compact newest-first list of an automation's runs with their statuses, mirroring the
 * runtime/task list rows in the Agents surface. Replaces the single "Run: N of M" summary
 * line with an actual per-run ledger so failed/succeeded runs are visible at a glance. Each
 * still-open run exposes a Cancel action so a run can be stopped without stopping the whole
 * automation.
 */
function AutomationRunsList({
  automation,
  automationId,
  runs,
  activeGoalItem,
  onCancelRun,
  cancelingRunId,
  cancelGateReason,
  onFocusAutomationRun,
}: {
  automation: Automation;
  automationId: string;
  runs: AutomationRun[];
  activeGoalItem: AutomationGoalItem | null;
  onCancelRun?: (runId: string) => void;
  cancelingRunId?: string | null;
  cancelGateReason: string | null;
  onFocusAutomationRun?: (
    automationId: string,
    runId: string,
    conversationId: string,
    options?: AutomationRunFocusOptions,
  ) => void;
}) {
  if (runs.length === 0) {
    return (
      <p className="text-xs" style={{ color: "var(--text-muted)" }}>
        No runs yet.
      </p>
    );
  }
  const ordered = [...runs].sort((a, b) => b.runIndex - a.runIndex);
  return (
    <ul className="flex flex-col gap-1" data-testid="agents-automation-runs-list">
      {ordered.map((run) => {
        const runView = getAutomationRunView(automation, run);
        const highlight = runTimelineHighlight(run);
        const phaseItem = runView.isOpen ? activeGoalItem : null;
        const isCancellable = runView.isCancellable;
        const isCanceling = cancelingRunId === run.id;
        const conversationId = run.conversationId;
        const canOpenRun = Boolean(conversationId && onFocusAutomationRun);
        const warning = runRowWarning(run);
        const statusPill = (
          <span
            className={cn(
              "inline-flex w-fit items-center rounded-full px-2 py-0.5 text-[0.6875rem] font-semibold",
              canOpenRun && "group-hover:underline",
              runStatusToneClass(runView.statusTone),
            )}
            style={{
              backgroundColor: "var(--bg-surface)",
              borderColor: "var(--border-default)",
              borderStyle: "solid",
              borderWidth: "1px",
            }}
          >
            {runView.statusLabel}
          </span>
        );
        return (
          <li
            key={run.id}
            className="grid grid-cols-[auto_minmax(0,1fr)_auto] items-center gap-2 rounded-md px-2 py-1.5"
            style={{
              backgroundColor: highlight.backgroundColor,
              borderColor: highlight.borderColor,
              borderStyle: "solid",
              borderWidth: "1px",
            }}
            data-testid={`agents-automation-run-${run.runIndex}`}
          >
            <span
              className="font-mono text-xs font-semibold tabular-nums"
              style={{ color: "var(--text-muted)" }}
            >
              #{run.runIndex}
            </span>
            <span className="min-w-0">
              <span
                className="block truncate text-xs"
                style={{ color: "var(--text-secondary)" }}
              >
                {runRowDetail(run)}
              </span>
              {phaseItem ? (
                <AutomationRunPhaseChip
                  item={phaseItem}
                  className="mt-1"
                  testId={`agents-automation-run-${run.runIndex}-phase`}
                />
              ) : null}
              {warning ? (
                <span
                  className="mt-0.5 block truncate text-[0.6875rem]"
                  style={{ color: "var(--status-warning)" }}
                  data-testid={`agents-automation-run-${run.runIndex}-warning`}
                >
                  {warning}
                </span>
              ) : null}
            </span>
            <span className="flex shrink-0 items-center gap-2">
              {run.prUrl ? (
                <AutomationRunPrLink
                  run={run}
                  testId={`agents-automation-run-${run.runIndex}-pr-link`}
                />
              ) : null}
              {canOpenRun && conversationId ? (
                <TooltipProvider delayDuration={150}>
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <button
                        type="button"
                        aria-label="Open run conversation"
                        onClick={() =>
                          onFocusAutomationRun?.(
                            automationId,
                            run.id,
                            conversationId,
                            getAutomationRunFocusOptions(run),
                          )
                        }
                        className="group cursor-pointer rounded-full outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent-primary)]"
                        data-testid={`agents-automation-run-${run.runIndex}-status`}
                      >
                        {statusPill}
                      </button>
                    </TooltipTrigger>
                    <TooltipContent side="top">
                      Open run conversation
                    </TooltipContent>
                  </Tooltip>
                </TooltipProvider>
              ) : (
                <span data-testid={`agents-automation-run-${run.runIndex}-status`}>
                  {statusPill}
                </span>
              )}
              {isCancellable && onCancelRun ? (
                <button
                  type="button"
                  className="rounded px-1.5 py-0.5 text-[0.6875rem] font-semibold disabled:opacity-50"
                  style={{ color: "var(--status-error)" }}
                  disabled={isCanceling || cancelGateReason !== null}
                  title={cancelGateReason ?? undefined}
                  onClick={() => onCancelRun(run.id)}
                  data-testid={`agents-automation-run-${run.runIndex}-cancel`}
                >
                  {isCanceling ? "Canceling..." : "Cancel"}
                </button>
              ) : null}
            </span>
          </li>
        );
      })}
    </ul>
  );
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
  onFocusAutomationRun,
}: AgentsAutomationPanelProps) {
  const resumeGate = useAgentGate("automationResume");
  const runNowGate = useAgentGate("automationRunNow");
  const restartGate = useAgentGate("automationRestart");
  const pauseGate = useAgentGate("automationPause");
  const stopGate = useAgentGate("automationStop");
  const cancelRunGate = useAgentGate("automationCancelRun");
  const retryPlanJudgeGate = useAgentGate("automationRetryPlanJudge");
  const retryJudgeGate = useAgentGate("automationRetryJudge");
  const settingsGate = useAgentGate("automationSettingsEdit");
  const setupEditGate = useAgentGate("automationSetupEdit");
  const deleteGate = useAgentGate("automationDelete");
  const afterPaint = useAfterPaintMounted(Boolean(automationId));
  const detail = useAutomationDetail(automationId, { enabled: afterPaint });
  const queryClient = useQueryClient();
  const [pendingAutomationSettingsPatch, setPendingAutomationSettingsPatch] =
    useState<AutomationSettingsPatch | null>(null);
  const { confirm, confirmationDialogProps, ConfirmationDialog } = useConfirmation();
  const { registry: modelRegistry } = useAgentModels();
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
      input: Parameters<typeof automationsApi.setupAgent.updateAutomation>[2];
    }) =>
      automationsApi.setupAgent.updateAutomation(
        conversationId,
        automation,
        input,
      ),
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
      toast.success("Automation cancelled");
    },
    onError: () => toast.error("Failed to cancel automation"),
  });
  const restartMutation = useMutation({
    mutationFn: () => automationsApi.restart(automationId),
    onSuccess: (outcome) => {
      invalidate();
      if (outcome.scheduled) {
        toast.success("Automation restarted with a new run");
      } else {
        toast.info(outcome.reason ?? "Automation was not restarted");
      }
    },
    onError: () => toast.error("Failed to restart automation"),
  });
  const runNowMutation = useMutation({
    mutationFn: () => automationsApi.triggerRunNow(automationId),
    onSuccess: (outcome) => {
      invalidate();
      if (outcome.scheduled) {
        toast.success("Automation run scheduled");
      } else {
        toast.info(outcome.reason ?? "Automation run was not scheduled");
      }
    },
    onError: () => toast.error("Failed to run automation"),
  });
  const retryJudgeMutation = useMutation({
    mutationFn: () => automationsApi.retryJudge(automationId),
    onSuccess: (outcome) => {
      invalidate();
      if (outcome.scheduled) {
        toast.success("Terminal judge retry scheduled");
      } else {
        toast.info(outcome.reason ?? "Terminal judge was not retried");
      }
    },
    onError: () => toast.error("Failed to retry terminal judge"),
  });
  const retryPlanJudgeMutation = useMutation({
    mutationFn: () => automationsApi.retryPlanJudge(automationId),
    onSuccess: (outcome) => {
      invalidate();
      if (outcome.scheduled) {
        toast.success("Plan judge retry scheduled");
      } else {
        toast.info(outcome.reason ?? "Plan judge was not retried");
      }
    },
    onError: () => toast.error("Failed to retry plan judge"),
  });
  const cancelRunMutation = useMutation({
    mutationFn: (runId: string) =>
      automationsApi.cancelRun({ id: automationId, runId }),
    onSuccess: () => {
      invalidate();
      toast.success("Run canceled");
    },
    onError: () => toast.error("Failed to cancel run"),
  });
  const maxRunsMutation = useMutation({
    mutationFn: (maxRuns: number) =>
      automationsApi.updateSettings({ id: automationId, maxRuns }),
    onSuccess: () => {
      invalidate();
      toast.success("Max runs updated");
    },
    onError: () => toast.error("Failed to update max runs"),
  });
  const automationSettingsMutation = useMutation({
    mutationFn: (patch: AutomationSettingsPatch) =>
      automationsApi.updateSettings({ id: automationId, ...patch }),
    onMutate: (patch) => {
      if (settingsGate.gated) return;
      setPendingAutomationSettingsPatch((current) => ({ ...current, ...patch }));
    },
    onSuccess: () => {
      invalidate();
      toast.success("Automation settings updated");
    },
    onError: (error) => {
      toast.error(automationSettingsErrorToast(error));
    },
    onSettled: () => {
      setPendingAutomationSettingsPatch(null);
    },
  });
  const deleteMutation = useMutation({
    mutationFn: () => automationsApi.delete(automationId),
    onSuccess: () => {
      evictDeletedAutomation(queryClient, automationId);
      toast.success("Automation deleted");
    },
    onError: () => toast.error("Failed to delete automation"),
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
      const providerSupportedModelAliases = supportedModelAliasesForProvider(
        providerOptions,
        provider,
      );
      const modelId = defaultModelForProvider(
        provider,
        modelRegistry,
        providerSupportedModelAliases,
      );
      const nextRuntime = normalizeRuntimeSelection(
        {
          provider,
          modelId,
          effort: defaultEffortForModel(provider, modelId, modelRegistry),
        },
        modelRegistry,
        supportedEffortsForProvider(providerOptions, provider),
        providerSupportedModelAliases,
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
      title: "Cancel automation?",
      description: AUTOMATION_CANCEL_CONFIRMATION_DESCRIPTION,
      confirmText: "Cancel automation",
      pendingText: "Cancelling...",
      variant: "destructive",
    });
    if (confirmed) {
      stopMutation.mutate();
    }
  };

  const handleDelete = async () => {
    const automationDetail = detail.data;
    if (!automationDetail) {
      return;
    }
    const confirmed = await confirm({
      title: "Delete draft automation?",
      description: describeAutomationDeleteConsequences(
        automationDetail.automation,
        automationDetail.runs,
      ),
      confirmText: "Delete draft",
      pendingText: "Deleting...",
      variant: "destructive",
    });
    if (confirmed) {
      deleteMutation.mutate();
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
  const runView = getAutomationRunView(automation, run);
  const goalItems = parseAutomationGoalItems(automation.goalItemsJson);
  const activeGoalItem = findInProgressAutomationGoalItemFromItems(goalItems);
  const stage = runView.stageLabel;
  const failureReason = describeRunFailure(run);
  const idleAfterCancelledRun = isIdleAfterCancelledRun(automation, run);
  const judgeRecovery = getAutomationJudgeRecovery(automation, run);
  const showPausedReason =
    !failureReason && automation.status === "paused" && Boolean(automation.pausedReasonCode);
  const actionPending =
    pauseMutation.isPending ||
    resumeMutation.isPending ||
    stopMutation.isPending ||
    restartMutation.isPending ||
    runNowMutation.isPending ||
    retryJudgeMutation.isPending ||
    retryPlanJudgeMutation.isPending ||
    deleteMutation.isPending;
  const canPause = automation.status === "active";
  const canResume = automation.status === "paused";
  const canStop = automation.status !== "completed" && automation.status !== "stopped";
  const canDelete = automation.status === "draft";
  const setupConversationId = automation.setupConversationId;
  const setupControlsDisabled =
    automation.status !== "draft" || !setupConversationId || setupEditGate.gated;
  const setupDisabledReason = !setupConversationId
    ? "Setup conversation link is missing, so settings cannot be updated here."
    : automation.status !== "draft"
      ? "Approved automation settings are read-only."
      : setupEditGate.reason;
  const showAutomationProposalCta =
    automation.status === "draft" &&
    Boolean(activeAutomationSetupQuestion) &&
    automationProposalOptionIndex >= 0;
  const showMissingSpecCta =
    automation.status === "draft" &&
    !showAutomationProposalCta &&
    Boolean(setupConversationId) &&
    (!automation.goalPrompt.trim() || goalItems.length === 0);
  const planGatePausedRun =
    automation.status === "paused" &&
    PLAN_GATE_PAUSED_REASON_CODES.has(automation.pausedReasonCode ?? "")
      ? [...runs]
          .sort((a, b) => b.runIndex - a.runIndex)
          .find(
            (candidate) =>
              candidate.status === "awaiting_plan_approval" &&
              Boolean(candidate.conversationId),
          ) ?? null
      : null;
  const showPlanGatePausedReason = Boolean(planGatePausedRun?.conversationId);
  const showGenericPausedReason = showPausedReason && !showPlanGatePausedReason;

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
        <SummaryRow
          label={runView.pr.rowLabel}
          value={runView.pr.value}
          testId="agents-automation-pr"
        />
      </div>

      <DetailSection title="Runs" testId="agents-automation-runs">
        {automation.status === "draft" || automation.status === "paused" ? (
          <MaxRunsEditor
            key={automation.maxRuns}
            currentMaxRuns={automation.maxRuns}
            runsUsed={runs.length}
            isSaving={maxRunsMutation.isPending}
            gateReason={settingsGate.reason}
            onSave={(maxRuns) => {
              if (!settingsGate.gated) maxRunsMutation.mutate(maxRuns);
            }}
          />
        ) : null}
        <AutomationRunsList
          automation={automation}
          automationId={automation.id}
          runs={runs}
          activeGoalItem={activeGoalItem}
          onCancelRun={(runId) => cancelRunMutation.mutate(runId)}
          cancelingRunId={
            cancelRunMutation.isPending
              ? (cancelRunMutation.variables ?? null)
              : null
          }
          cancelGateReason={cancelRunGate.reason}
          {...(onFocusAutomationRun ? { onFocusAutomationRun } : {})}
        />
      </DetailSection>

      <DetailSection title="Settings" testId="agents-automation-settings">
        <AutomationPlanGateSettings
          automation={automation}
          isSaving={automationSettingsMutation.isPending}
          pendingPatch={pendingAutomationSettingsPatch}
          gateReason={settingsGate.reason}
          onUpdate={(patch) => {
            if (!settingsGate.gated) automationSettingsMutation.mutate(patch);
          }}
        />
      </DetailSection>

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
          <AutomationPhaseProgress value={automation.goalItemsJson} />
        ) : (
          <p className="text-xs" style={{ color: "var(--text-muted)" }}>
            No phases configured yet.
          </p>
        )}
      </DetailSection>

      <DetailSection title="Spec" testId="agents-automation-spec">
        <AutomationSpecView specArtifactId={automation.specArtifactId} />
      </DetailSection>

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

      {judgeRecovery ? (
        <div
          className="flex flex-wrap items-center justify-between gap-3 rounded-md px-3 py-2 text-xs"
          style={{
            backgroundColor: "var(--bg-surface)",
            borderColor: "var(--border-default)",
            borderStyle: "solid",
            borderWidth: "1px",
            color: "var(--text-secondary)",
          }}
          data-testid={`agents-automation-${judgeRecovery.kind}-judge-recovery`}
        >
          <span className="min-w-0">
            <strong style={{ color: "var(--text-primary)" }}>
              {judgeRecovery.statusLabel}.
            </strong>{" "}
            {judgeRecovery.description}
          </span>
          <Button
            type="button"
            variant="secondary"
            size="sm"
            disabled={actionPending || (judgeRecovery.kind === "plan" ? retryPlanJudgeGate.gated : retryJudgeGate.gated)}
            title={(judgeRecovery.kind === "plan" ? retryPlanJudgeGate.reason : retryJudgeGate.reason) ?? undefined}
            onClick={() =>
              judgeRecovery.kind === "plan"
                ? retryPlanJudgeMutation.mutate()
                : retryJudgeMutation.mutate()
            }
          >
            {judgeRecovery.actionLabel}
          </Button>
        </div>
      ) : null}

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
      ) : showPlanGatePausedReason && planGatePausedRun?.conversationId ? (
        <div
          className="flex flex-wrap items-center justify-between gap-3 rounded-md px-3 py-2 text-xs"
          style={{
            backgroundColor: "var(--bg-surface)",
            borderColor: "var(--border-default)",
            borderStyle: "solid",
            borderWidth: "1px",
            color: "var(--text-secondary)",
          }}
          data-testid="agents-automation-plan-gate-paused"
        >
          <span className="min-w-0">
            {planGatePausedCopy(automation.pausedReasonCode)}
            {automation.pausedReasonDetail ? ` ${automation.pausedReasonDetail}` : ""}
          </span>
          <Button
            type="button"
            variant="secondary"
            size="sm"
            disabled={!onFocusAutomationRun}
            onClick={() =>
              planGatePausedRun.conversationId
                ? onFocusAutomationRun?.(
                    automation.id,
                    planGatePausedRun.id,
                    planGatePausedRun.conversationId,
                    getAutomationRunFocusOptions(planGatePausedRun),
                  )
                : undefined
            }
            data-testid="agents-automation-plan-gate-open"
          >
            Open run conversation
          </Button>
        </div>
      ) : idleAfterCancelledRun ? (
        <div
          className="flex flex-wrap items-center justify-between gap-3 rounded-md px-3 py-2 text-xs"
          style={{
            backgroundColor: "var(--bg-surface)",
            borderColor: "var(--border-default)",
            borderStyle: "solid",
            borderWidth: "1px",
            color: "var(--text-secondary)",
          }}
          data-testid="agents-automation-idle-after-cancelled"
        >
          <span className="min-w-0">{CANCELLED_RUN_RESTART_DESCRIPTION}</span>
          <Button
            type="button"
            variant="secondary"
            size="sm"
            disabled={actionPending || runNowGate.gated}
            title={runNowGate.reason ?? undefined}
            onClick={() => runNowMutation.mutate()}
          >
            Run now
          </Button>
        </div>
      ) : showGenericPausedReason ? (
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
            disabled={actionPending || pauseGate.gated}
            title={pauseGate.reason ?? undefined}
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
            disabled={actionPending || resumeGate.gated}
            title={resumeGate.reason ?? undefined}
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
            disabled={actionPending || stopGate.gated}
            title={stopGate.reason ?? undefined}
            onClick={handleStop}
            data-testid="agents-automation-stop"
          >
            <Square className="h-4 w-4" />
            Cancel automation
          </Button>
        ) : null}
        {automation.status === "stopped" ? (
          <Button
            type="button"
            variant="secondary"
            size="sm"
            className="gap-2"
            disabled={actionPending || restartGate.gated}
            title={restartGate.reason ?? undefined}
            onClick={() => restartMutation.mutate()}
            data-testid="agents-automation-restart"
          >
            <RotateCcw className="h-4 w-4" aria-hidden="true" />
            Restart automation
          </Button>
        ) : null}
        {canDelete ? (
          <Button
            type="button"
            variant="outline"
            size="sm"
            className="gap-2 text-[var(--status-error)]"
            disabled={actionPending || deleteGate.gated}
            title={deleteGate.reason ?? undefined}
            onClick={handleDelete}
            data-testid="agents-automation-delete"
          >
            <Trash2 className="h-4 w-4" />
            Delete draft
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
