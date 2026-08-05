import { Info, Settings2 } from "lucide-react";
import { type ReactNode, useEffect, useMemo, useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";

import {
  chatApi,
  type AgentConversationWorkspace,
  type AgentConversationWorkspacePublicationEvent,
} from "@/api/chat";
import type { SettingsSectionId } from "@/components/settings/settings-registry";
import { Switch } from "@/components/ui/switch";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { useConfirmation } from "@/hooks/useConfirmation";
import { useReviewSettings } from "@/hooks/useReviewSettings";
import { extractErrorMessage } from "@/lib/errors";
import { useUiStore } from "@/stores/uiStore";
import {
  agentWorkspaceKeys,
  invalidateWorkspaceQueries,
} from "./agentWorkspaceQueries";
import {
  deriveAgentsPublishAutomationSnapshot,
  type AgentsPublishAutomationSnapshot,
} from "./agentsPublishAutomationSnapshot";
import { WORKSPACE_REVIEW_AUTOMATION_COPY } from "./workspaceReviewAutomationCopy";

type PrSupervisionResultOverride = {
  sourceWorkspaceUpdatedAt: string | null;
  workspace: AgentConversationWorkspace;
};

type AutoPublishMutationInput = {
  autoPublishEnabled: boolean;
};

type PrSupervisionMutationInput = {
  autoFixEnabled: boolean;
  autoMergeDesired: boolean;
};

type ReviewAutomationMutationInput = {
  enabled: boolean | null;
};

type ReviewAutomationResultOverride = {
  sourceWorkspaceUpdatedAt: string | null;
  workspace: AgentConversationWorkspace;
};

const PR_AUTOFIX_LEDGER_STEPS = new Set([
  "repair_fingerprint_hold",
  "repair_budget_exhausted",
  "repair_sent",
]);

function PublishSwitchInfoTooltip({
  label,
  children,
  settingsSection,
}: {
  label: string;
  children: ReactNode;
  settingsSection?: SettingsSectionId;
}) {
  const openModal = useUiStore((s) => s.openModal);
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <button
          type="button"
          aria-label={label}
          className="inline-flex h-5 w-5 shrink-0 cursor-help items-center justify-center rounded-full border-0 bg-transparent p-0 text-[var(--text-muted)] transition-colors hover:text-[var(--text-secondary)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--focus-ring)]"
        >
          <Info className="h-3.5 w-3.5" aria-hidden="true" />
        </button>
      </TooltipTrigger>
      <TooltipContent
        side="top"
        align="center"
        className="max-w-[300px] text-xs leading-relaxed"
      >
        <div className="space-y-2">
          <div>{children}</div>
          {settingsSection && (
            <button
              type="button"
              className="inline-flex items-center gap-1.5 rounded-[6px] px-1.5 py-1 text-[11px] font-medium text-[var(--accent-primary)] hover:bg-[var(--bg-hover)] focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-[var(--focus-ring)]"
              onClick={() =>
                openModal("settings", { section: settingsSection })
              }
              data-testid={`agents-tooltip-settings-${settingsSection}`}
            >
              <Settings2 className="h-3 w-3" aria-hidden="true" />
              Change defaults in Settings
            </button>
          )}
        </div>
      </TooltipContent>
    </Tooltip>
  );
}

export function AgentsPublishAutomationTab({
  workspace,
  hasPublishedPr,
  isPipelinePrAutomationWorkspace,
  canConfigurePrSupervision,
  hasUncommittedChanges,
  terminalPrLabel,
  publicationEvents,
  onSnapshotChange,
}: {
  workspace: AgentConversationWorkspace;
  hasPublishedPr: boolean;
  isPipelinePrAutomationWorkspace: boolean;
  canConfigurePrSupervision: boolean;
  hasUncommittedChanges: boolean;
  terminalPrLabel: string;
  publicationEvents: AgentConversationWorkspacePublicationEvent[];
  onSnapshotChange: (snapshot: AgentsPublishAutomationSnapshot) => void;
}) {
  const queryClient = useQueryClient();
  const conversationId = workspace.conversationId;
  const [prSupervisionResultOverride, setPrSupervisionResultOverride] =
    useState<PrSupervisionResultOverride | null>(null);
  const [reviewAutomationResultOverride, setReviewAutomationResultOverride] =
    useState<ReviewAutomationResultOverride | null>(null);
  const reviewSettingsQuery = useReviewSettings();
  const { confirm, confirmationDialogProps, ConfirmationDialog } =
    useConfirmation();
  const autoPublishMutation = useMutation<
    AgentConversationWorkspace,
    Error,
    AutoPublishMutationInput
  >({
    mutationFn: (input) =>
      chatApi.setAgentConversationWorkspaceAutoPublish(conversationId!, input),
    onSuccess: (updatedWorkspace) => {
      queryClient.setQueryData(
        agentWorkspaceKeys.workspace(updatedWorkspace.conversationId),
        updatedWorkspace,
      );
      void queryClient.invalidateQueries({
        queryKey: agentWorkspaceKeys.publicationEvents(
          updatedWorkspace.conversationId,
        ),
      });
    },
    onError: (error) => {
      toast.error(
        error instanceof Error
          ? error.message
          : "Unable to update Auto Publish",
      );
    },
  });
  const prSupervisionMutation = useMutation<
    AgentConversationWorkspace,
    unknown,
    PrSupervisionMutationInput
  >({
    mutationFn: (input) =>
      chatApi.setAgentConversationWorkspacePrSupervision(conversationId!, {
        autoFixEnabled: input.autoFixEnabled,
        autoMergeDesired: input.autoMergeDesired,
        autoMergeMethod: workspace.prAutoMergeMethod ?? "squash",
      }),
    onSuccess: (updatedWorkspace) => {
      queryClient.setQueryData(
        agentWorkspaceKeys.workspace(updatedWorkspace.conversationId),
        updatedWorkspace,
      );
      setPrSupervisionResultOverride({
        sourceWorkspaceUpdatedAt: workspace.updatedAt ?? null,
        workspace: updatedWorkspace,
      });
      void invalidateWorkspaceQueries(
        queryClient,
        updatedWorkspace.conversationId,
      );
    },
    onError: (error) => {
      toast.error(
        extractErrorMessage(error, "Unable to update PR supervision"),
      );
    },
  });
  const reviewAutomationMutation = useMutation<
    AgentConversationWorkspace,
    Error,
    ReviewAutomationMutationInput
  >({
    mutationFn: (input) =>
      chatApi.setAgentConversationWorkspaceReviewAutomation(conversationId, input),
    onSuccess: (updatedWorkspace) => {
      queryClient.setQueryData(
        agentWorkspaceKeys.workspace(updatedWorkspace.conversationId),
        updatedWorkspace,
      );
      setReviewAutomationResultOverride({
        sourceWorkspaceUpdatedAt: workspace.updatedAt ?? null,
        workspace: updatedWorkspace,
      });
      void invalidateWorkspaceQueries(queryClient, updatedWorkspace.conversationId);
    },
    onError: (error) => {
      toast.error(extractErrorMessage(error, "Unable to update Auto Review & Fix"));
    },
  });
  useEffect(() => {
    if (!prSupervisionResultOverride) {
      return;
    }
    if (
      workspace.conversationId !==
      prSupervisionResultOverride.workspace.conversationId
    ) {
      setPrSupervisionResultOverride(null);
      return;
    }

    const updatedWorkspace = prSupervisionResultOverride.workspace;
    const workspaceMatchesResult =
      workspace.prAutofixEnabled === updatedWorkspace.prAutofixEnabled &&
      workspace.prAutoMergeDesired === updatedWorkspace.prAutoMergeDesired &&
      workspace.prSupervisionStatus === updatedWorkspace.prSupervisionStatus;
    const workspaceRefreshedAfterMutation =
      prSupervisionResultOverride.sourceWorkspaceUpdatedAt !== null &&
      workspace.updatedAt !==
        prSupervisionResultOverride.sourceWorkspaceUpdatedAt;

    if (workspaceMatchesResult || workspaceRefreshedAfterMutation) {
      setPrSupervisionResultOverride(null);
    }
  }, [prSupervisionResultOverride, workspace]);
  useEffect(() => {
    if (!reviewAutomationResultOverride) {
      return;
    }
    if (
      workspace.conversationId !==
      reviewAutomationResultOverride.workspace.conversationId
    ) {
      setReviewAutomationResultOverride(null);
      return;
    }
    const updatedWorkspace = reviewAutomationResultOverride.workspace;
    const workspaceMatchesResult =
      workspace.reviewAutomationOverride ===
      updatedWorkspace.reviewAutomationOverride;
    const workspaceRefreshedAfterMutation =
      reviewAutomationResultOverride.sourceWorkspaceUpdatedAt !== null &&
      workspace.updatedAt !==
        reviewAutomationResultOverride.sourceWorkspaceUpdatedAt;
    if (workspaceMatchesResult || workspaceRefreshedAfterMutation) {
      setReviewAutomationResultOverride(null);
    }
  }, [reviewAutomationResultOverride, workspace]);

  const pendingAutoPublish = autoPublishMutation.isPending
    ? autoPublishMutation.variables
    : null;
  const pendingPrSupervision = prSupervisionMutation.isPending
    ? prSupervisionMutation.variables
    : null;
  const pendingReviewAutomation = reviewAutomationMutation.isPending
    ? reviewAutomationMutation.variables
    : null;
  const settledPrSupervisionWorkspace =
    prSupervisionResultOverride?.workspace.conversationId ===
    workspace.conversationId
      ? prSupervisionResultOverride.workspace
      : null;
  const settledReviewAutomationWorkspace =
    reviewAutomationResultOverride?.workspace.conversationId ===
    workspace.conversationId
      ? reviewAutomationResultOverride.workspace
      : null;
  const snapshot = useMemo(
    () =>
      deriveAgentsPublishAutomationSnapshot({
        workspace,
        hasPublishedPr,
        pendingAutoPublish,
        pendingPrSupervision,
        pendingReviewAutomation,
        settledPrSupervisionWorkspace,
        settledReviewAutomationWorkspace,
        isAutoPublishSaving: autoPublishMutation.isPending,
        isPrSupervisionSaving: prSupervisionMutation.isPending,
        isReviewAutomationSaving: reviewAutomationMutation.isPending,
      }),
    [
      autoPublishMutation.isPending,
      hasPublishedPr,
      pendingAutoPublish,
      pendingPrSupervision,
      pendingReviewAutomation,
      prSupervisionMutation.isPending,
      reviewAutomationMutation.isPending,
      settledReviewAutomationWorkspace,
      settledPrSupervisionWorkspace,
      workspace,
    ],
  );
  useEffect(() => {
    onSnapshotChange(snapshot);
  }, [onSnapshotChange, snapshot]);
  const {
    autoPublishEnabled,
    canRunPrSupervisionAutomation,
    isAutoPublishSaving,
    isPrSupervisionSaving,
    isReviewAutomationSaving,
    prAutofixEnabled,
    prAutoMergeDesired,
    reviewAutomationOverride,
  } = snapshot;
  const canConfigureAutoPublish = canConfigurePrSupervision;
  const prAutofixLedger = useMemo(
    () =>
      publicationEvents.filter((event) =>
        PR_AUTOFIX_LEDGER_STEPS.has(event.step),
      ),
    [publicationEvents],
  );
  const prAutofixSpend = workspace.prAutofixFingerprintSpend;

  const updatePrSupervisionPreferences = (next: {
    autoFixEnabled: boolean;
    autoMergeDesired: boolean;
  }) => {
    if (
      !canConfigurePrSupervision ||
      !canRunPrSupervisionAutomation ||
      isPrSupervisionSaving
    ) {
      return;
    }
    prSupervisionMutation.mutate(next);
  };

  const confirmAutoPublishChange = (nextEnabled: boolean) => {
    if (!canConfigureAutoPublish || autoPublishMutation.isPending) {
      return;
    }
    const enablingDirtyWarning =
      nextEnabled && hasUncommittedChanges
        ? " The next automatic trigger may commit current local workspace changes."
        : "";
    const isInitialPublishToggle = !hasPublishedPr;
    void confirm({
      title: isInitialPublishToggle
        ? nextEnabled
          ? "Enable Auto Publish?"
          : "Disable Auto Publish?"
        : nextEnabled
          ? "Resume Auto Publish?"
          : "Pause Auto Publish?",
      description: isInitialPublishToggle
        ? nextEnabled
          ? `When the agent finishes, RalphX will run Commit & Publish for this workspace and open a draft pull request.${enablingDirtyWarning}`
          : "RalphX will wait for manual Commit & Publish before opening the first pull request."
        : nextEnabled
          ? `Background publish, PR autofix, and auto-merge automation will resume for ${terminalPrLabel}.${enablingDirtyWarning}`
          : `Background publish, stale-base publish scans, PR autofix publishing, and auto-merge automation will pause for ${terminalPrLabel}. Manual Commit & Publish remains available.`,
      confirmText: isInitialPublishToggle
        ? nextEnabled
          ? "Enable Auto Publish"
          : "Disable Auto Publish"
        : nextEnabled
          ? "Resume Auto Publish"
          : "Pause Auto Publish",
      pendingText: "Saving...",
      onConfirm: () =>
        autoPublishMutation.mutateAsync({ autoPublishEnabled: nextEnabled }),
    });
  };
  const confirmReviewAutomationChange = (enabled: boolean | null) => {
    if (isReviewAutomationSaving) {
      return;
    }
    const action =
      enabled === null
        ? {
            title: "Inherit Auto Review & Fix?",
            confirmText: "Use global Review settings",
            detail: "This conversation will follow the global Review settings again.",
          }
        : enabled
          ? {
              title: "Turn on Auto Review & Fix?",
              confirmText: "Turn on Auto Review & Fix",
              detail: "This conversation will keep the Review → Fix → Review loop armed.",
            }
          : {
              title: "Turn off Auto Review & Fix?",
              confirmText: "Turn off Auto Review & Fix",
              detail: "Manual Workspace Review and Fix Issues remain available.",
            };
    void confirm({
      title: action.title,
      description: `${WORKSPACE_REVIEW_AUTOMATION_COPY} ${action.detail}`,
      confirmText: action.confirmText,
      pendingText: "Saving...",
      onConfirm: () => reviewAutomationMutation.mutateAsync({ enabled }),
    });
  };
  const globalReviewLoopActive = Boolean(
    reviewSettingsQuery.data?.require_workspace_review &&
      reviewSettingsQuery.data?.autofix_workspace_review_blocking_findings,
  );

  const cardStyle = {
    backgroundColor: "var(--bg-subtle)",
    borderColor: "var(--border-subtle)",
    borderStyle: "solid",
    borderWidth: "1px",
  } as const;

  return (
    <>
      <div
        className="grid gap-3 text-xs"
        data-testid="agents-pr-supervision-controls"
      >
        <div className="rounded-lg p-3" style={cardStyle}>
          <div className="flex min-h-8 items-center gap-1.5 text-[var(--text-secondary)]">
            <label className="flex min-h-8 items-center gap-2">
              <Switch
                checked={autoPublishEnabled}
                disabled={!canConfigureAutoPublish || isAutoPublishSaving}
                onCheckedChange={confirmAutoPublishChange}
                aria-label="Auto Publish"
                data-testid="agents-auto-publish-switch"
              />
              <span>Auto Publish</span>
            </label>
            <PublishSwitchInfoTooltip label="About Auto Publish">
              {isPipelinePrAutomationWorkspace
                ? "Controls PR autofix publishing and auto-merge automation for this task-managed PR."
                : hasPublishedPr
                  ? "Controls background publishing for this PR, including publish-after-turn, stale-base scans, PR autofix publishing, and auto-merge automation."
                  : "Runs Commit & Publish automatically when the agent finishes before a pull request exists."}
            </PublishSwitchInfoTooltip>
          </div>
        </div>
        <div className="rounded-lg p-3" style={cardStyle}>
          <div className="flex items-start justify-between gap-3">
            <div className="min-w-0">
              <div className="flex items-center gap-1.5 text-[var(--text-secondary)]">
                <span className="text-xs font-medium">Auto Review &amp; Fix</span>
              </div>
              <p className="mt-1 text-xs leading-relaxed text-[var(--text-muted)]">
                {WORKSPACE_REVIEW_AUTOMATION_COPY}
              </p>
              {reviewAutomationOverride === null && (
                <p className="mt-1 text-[11px] text-[var(--text-muted)]">
                  Follows global Review settings (currently {globalReviewLoopActive ? "on" : "off"})
                </p>
              )}
            </div>
            <div
              className="inline-flex shrink-0 rounded-md border p-0.5"
              style={{ borderColor: "var(--border-subtle)" }}
              role="group"
              aria-label="Auto Review & Fix preference"
            >
              {([
                [null, "Inherit"],
                [true, "On"],
                [false, "Off"],
              ] as const).map(([value, label]) => (
                <button
                  key={label}
                  type="button"
                  aria-pressed={reviewAutomationOverride === value}
                  disabled={isReviewAutomationSaving}
                  onClick={() => confirmReviewAutomationChange(value)}
                  className="rounded px-2 py-1 text-[11px] font-medium text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] aria-pressed:bg-[var(--accent-muted)] aria-pressed:text-[var(--accent-primary)] focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-[var(--focus-ring)] disabled:opacity-60"
                >
                  {label}
                </button>
              ))}
            </div>
          </div>
        </div>
        <div className="rounded-lg p-3" style={cardStyle}>
          <div className="flex min-h-8 items-center gap-1.5 text-[var(--text-secondary)]">
            <label className="flex min-h-8 items-center gap-2">
              <Switch
                checked={prAutofixEnabled}
                disabled={
                  !canConfigurePrSupervision ||
                  !canRunPrSupervisionAutomation ||
                  isPrSupervisionSaving
                }
                onCheckedChange={(checked) =>
                  updatePrSupervisionPreferences({
                    autoFixEnabled: checked,
                    autoMergeDesired: prAutoMergeDesired,
                  })
                }
                aria-label="Autofix CI & Reviews"
                data-testid="agents-pr-autofix-switch"
              />
              <span>Autofix CI &amp; Reviews</span>
            </label>
            <PublishSwitchInfoTooltip
              label="About Autofix CI and Reviews"
              settingsSection="workspace"
            >
              RalphX monitors this PR for failing checks and review feedback,
              then publishes follow-up fixes from the workspace automatically.
            </PublishSwitchInfoTooltip>
          </div>
        </div>
        <div className="rounded-lg p-3" style={cardStyle}>
          <div className="flex min-h-8 items-center gap-1.5 text-[var(--text-secondary)]">
            <label className="flex min-h-8 items-center gap-2">
              <Switch
                checked={prAutoMergeDesired}
                disabled={
                  !canConfigurePrSupervision ||
                  !canRunPrSupervisionAutomation ||
                  isPrSupervisionSaving
                }
                onCheckedChange={(checked) =>
                  updatePrSupervisionPreferences({
                    autoFixEnabled: prAutofixEnabled,
                    autoMergeDesired: checked,
                  })
                }
                aria-label="GitHub auto-merge"
                data-testid="agents-pr-auto-merge-switch"
              />
              <span>GitHub auto-merge</span>
            </label>
            <PublishSwitchInfoTooltip
              label="About GitHub auto-merge"
              settingsSection="workspace"
            >
              RalphX asks GitHub to merge the PR after required checks and
              review requirements pass.
            </PublishSwitchInfoTooltip>
          </div>
        </div>
        {(prAutofixSpend || prAutofixLedger.length > 0) && (
          <div className="rounded-lg p-3" style={cardStyle}>
            <div className="text-[0.625rem] font-medium uppercase tracking-[0.14em] text-[var(--text-muted)]">
              Autofix repair budget
            </div>
            {prAutofixSpend && (
              <div
                className="mt-1 text-xs font-medium text-[var(--text-primary)]"
                data-testid="agents-pr-autofix-budget"
              >
                {prAutofixSpend.minutes} / {prAutofixSpend.budgetMinutes} min ·{" "}
                {prAutofixSpend.generations} generations
                {prAutofixSpend.isExhausted
                  ? " · Repair budget exhausted"
                  : ""}
              </div>
            )}
            {prAutofixLedger.length > 0 && (
              <ul
                className="mt-2 space-y-1 text-[0.6875rem] text-[var(--text-secondary)]"
                data-testid="agents-pr-autofix-ledger"
              >
                {prAutofixLedger.map((event) => (
                  <li key={event.id}>{event.summary}</li>
                ))}
              </ul>
            )}
          </div>
        )}
      </div>
      <ConfirmationDialog {...confirmationDialogProps} />
    </>
  );
}
