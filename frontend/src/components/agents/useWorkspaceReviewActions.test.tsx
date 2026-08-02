import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  AgentWorkspaceHttpError,
  chatApi,
  type AgentWorkspaceReviewContext,
  type AgentWorkspaceReviewFixerConfirmation,
  type AgentWorkspaceReviewStartConfirmation,
  type AgentWorkspaceReviewStartPreview,
} from "@/api/chat";
import { logger } from "@/lib/logger";

import { useWorkspaceReviewActions } from "./useWorkspaceReviewActions";

const reviewerRuntime = {
  provider: "claude",
  model: "sonnet",
  effort: "high",
  serviceTier: "provider_default" as const,
  coordinationMode: "solo" as const,
  personaId: null,
};

vi.mock("@/api/chat", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/api/chat")>();
  return {
    ...actual,
    chatApi: {
      ...actual.chatApi,
      getAgentWorkspaceReviewContext: vi.fn(),
      getAgentWorkspaceReviewStartPreview: vi.fn(),
    },
  };
});

vi.mock("@/lib/logger", () => ({
  logger: {
    debug: vi.fn(),
    log: vi.fn(),
    warn: vi.fn(),
    error: vi.fn(),
  },
}));

vi.mock("@/hooks/useAgentModels", () => ({
  useAgentModels: () => ({
    registry: {
      claude: [
        {
          id: "sonnet",
          label: "Sonnet",
          menuLabel: "Sonnet",
          defaultEffort: "high",
          supportedEfforts: ["high"],
        },
      ],
    },
  }),
}));

vi.mock("@/hooks/usePersonas", () => ({
  usePersonas: () => ({ data: [] }),
}));

vi.mock("@/api/manual-role-defaults", () => ({
  manualRoleDefaultsApi: {
    list: vi.fn().mockResolvedValue({
      roles: [
        {
          role: "workspace_reviewer",
          displayName: "Reviewer",
          familyDisplayName: "Feedback Loops",
          description: "Reviews local workspace changes.",
          configured: null,
          effective: {
            provider: "claude",
            model: "sonnet",
            effort: "high",
            serviceTier: "provider_default",
            coordinationMode: "solo",
            personaId: null,
          },
          source: "provider_default",
          diagnostics: [],
          controls: {
            capabilities: [{ value: "solo", enabled: true, disabledReason: null }],
            speeds: [
              { value: "provider_default", enabled: true, disabledReason: null },
            ],
            persona: { enabled: false, disabledReason: null },
          },
        },
        {
          role: "workspace_repair",
          displayName: "Repair",
          familyDisplayName: "Feedback Loops",
          description: "Repairs blocking local workspace review findings.",
          configured: null,
          effective: {
            provider: "claude",
            model: "sonnet",
            effort: "high",
            serviceTier: "provider_default",
            coordinationMode: "solo",
            personaId: null,
          },
          source: "provider_default",
          diagnostics: [],
          controls: {
            capabilities: [{ value: "solo", enabled: true, disabledReason: null }],
            speeds: [
              { value: "provider_default", enabled: true, disabledReason: null },
            ],
            persona: { enabled: false, disabledReason: null },
          },
        },
      ],
    }),
  },
}));

vi.mock("@/api/harness-providers", () => ({
  harnessProvidersApi: {
    list: vi.fn().mockResolvedValue({
      defaultProvider: "claude",
      requiresOnboarding: false,
      providers: [
        {
          provider: "claude",
          enabled: true,
          available: true,
          missingCoreExecFeatures: [],
          error: null,
          status: "ready",
        },
      ],
    }),
  },
}));

function Harness({
  onStartReview,
  reviewAutomation = null,
}: {
  onStartReview: (input: {
    force: boolean;
    confirmation: AgentWorkspaceReviewStartConfirmation;
    enableReviewAutomation?: boolean;
  }) => Promise<unknown>;
  reviewAutomation?: {
    effectiveLoopActive: boolean;
    overrideOn: boolean;
  } | null;
}) {
  const { startReview, confirmationDialogProps, ConfirmationDialog } =
    useWorkspaceReviewActions({
      conversationId: "conversation-1",
      projectId: "project-1",
      onStartReview,
      onStartFixer: vi.fn(),
      reviewAutomation,
    });
  return (
    <>
      <button type="button" onClick={() => startReview(false)}>
        Run review
      </button>
      <ConfirmationDialog {...confirmationDialogProps} />
    </>
  );
}

function FixerHarness({
  context,
  onStartFixer,
}: {
  context: AgentWorkspaceReviewContext;
  onStartFixer: (input: {
    confirmation: AgentWorkspaceReviewFixerConfirmation;
    runtimeOverride: typeof reviewerRuntime;
  }) => Promise<unknown>;
}) {
  const { startFixer, confirmationDialogProps, ConfirmationDialog } =
    useWorkspaceReviewActions({
      conversationId: "conversation-1",
      projectId: "project-1",
      onStartReview: vi.fn(),
      onStartFixer,
    });
  return (
    <>
      <button type="button" onClick={() => startFixer(context)}>
        Fix issues
      </button>
      <ConfirmationDialog {...confirmationDialogProps} />
    </>
  );
}

describe("useWorkspaceReviewActions", () => {
  const unfinishedGitDetail =
    "Resolve conflicts and complete or abort the merge or rebase before retrying Workspace Review.";

  beforeEach(() => {
    vi.mocked(logger.debug).mockReset();
    vi.mocked(chatApi.getAgentWorkspaceReviewStartPreview).mockReset();
    vi.mocked(chatApi.getAgentWorkspaceReviewContext).mockReset();
  });

  it("requires a prepared receipt before starting a manual review", async () => {
    const preview: AgentWorkspaceReviewStartPreview = {
      success: true,
      target: {
        scope: "workspace_delta",
        baseRef: "main",
        baseSha: "base",
        headRef: "HEAD",
        headSha: "head",
        diffFingerprint: "fingerprint",
        sourcePullRequestNumber: null,
      },
      willDisableAutoMerge: true,
      prNumber: 42,
      mergeMethod: "squash",
      restoreAfterPublish: true,
      confirmation: {
        targetScope: "workspace_delta",
        diffFingerprint: "fingerprint",
        headSha: "head",
        prNumber: 42,
        willDisableAutoMerge: true,
        mergeMethod: "squash",
        restoreAfterPublish: true,
      },
    };
    vi.mocked(chatApi.getAgentWorkspaceReviewStartPreview).mockResolvedValue(preview);
    const onStartReview = vi.fn().mockResolvedValue(undefined);
    const user = userEvent.setup();

    render(<Harness onStartReview={onStartReview} />);
    await user.click(screen.getByRole("button", { name: "Run review" }));

    const dialog = await screen.findByRole("alertdialog");
    await waitFor(() => {
      expect(
        within(dialog).getByText(/GitHub auto-merge is enabled on PR #42/),
      ).toBeInTheDocument();
    });
    expect(onStartReview).not.toHaveBeenCalled();

    await user.click(within(dialog).getByRole("button", { name: "Start review" }));

    await waitFor(() => {
      expect(onStartReview).toHaveBeenCalledWith({
        force: false,
        confirmation: preview.confirmation,
        runtimeOverride: reviewerRuntime,
      });
    });
    for (const phase of [
      "load_role_defaults",
      "refresh_provider_runtime",
      "prepare_description",
      "prepare_completed",
      "confirm_action",
    ]) {
      expect(logger.debug).toHaveBeenCalledWith(
        "[RoleRuntimeConfirmationTiming]",
        expect.objectContaining({
          role: "workspace_reviewer",
          phase,
          elapsedMs: expect.any(Number),
          totalElapsedMs: expect.any(Number),
          outcome: "completed",
        }),
      );
    }
  });

  it("offers and forwards the conversation-only automation opt-in when the loop is not effective", async () => {
    const preview: AgentWorkspaceReviewStartPreview = {
      success: true,
      target: {
        scope: "workspace_delta",
        baseRef: "main",
        baseSha: "base",
        headRef: "HEAD",
        headSha: "head",
        diffFingerprint: "fingerprint",
        sourcePullRequestNumber: null,
      },
      willDisableAutoMerge: false,
      prNumber: null,
      mergeMethod: null,
      restoreAfterPublish: false,
      confirmation: {
        targetScope: "workspace_delta",
        diffFingerprint: "fingerprint",
        headSha: "head",
        prNumber: null,
        willDisableAutoMerge: false,
        mergeMethod: null,
        restoreAfterPublish: false,
      },
    };
    vi.mocked(chatApi.getAgentWorkspaceReviewStartPreview).mockResolvedValue(preview);
    const onStartReview = vi.fn().mockResolvedValue(undefined);
    const user = userEvent.setup();

    render(
      <Harness
        onStartReview={onStartReview}
        reviewAutomation={{ effectiveLoopActive: false, overrideOn: false }}
      />,
    );
    await user.click(screen.getByRole("button", { name: "Run review" }));

    const dialog = await screen.findByRole("alertdialog");
    const automationSwitch = await within(dialog).findByRole("switch", {
      name: "Auto Review & Fix until passing",
    });
    expect(automationSwitch).not.toBeChecked();
    await user.click(automationSwitch);
    await user.click(within(dialog).getByRole("button", { name: "Start review" }));

    await waitFor(() =>
      expect(onStartReview).toHaveBeenCalledWith({
        force: false,
        confirmation: preview.confirmation,
        runtimeOverride: reviewerRuntime,
        enableReviewAutomation: true,
      }),
    );
  });

  it("hides the opt-in when the full automation loop is already effective", async () => {
    vi.mocked(chatApi.getAgentWorkspaceReviewStartPreview).mockResolvedValue({
      success: true,
      target: null,
      willDisableAutoMerge: false,
      prNumber: null,
      mergeMethod: null,
      restoreAfterPublish: false,
      confirmation: {
        targetScope: "workspace_delta",
        diffFingerprint: "fingerprint",
        headSha: "head",
        prNumber: null,
        willDisableAutoMerge: false,
        mergeMethod: null,
        restoreAfterPublish: false,
      },
    });
    const user = userEvent.setup();
    render(
      <Harness
        onStartReview={vi.fn().mockResolvedValue(undefined)}
        reviewAutomation={{ effectiveLoopActive: true, overrideOn: false }}
      />,
    );
    await user.click(screen.getByRole("button", { name: "Run review" }));

    const dialog = await screen.findByRole("alertdialog");
    await within(dialog).findByRole("button", { name: "Start review" });
    expect(
      within(dialog).queryByRole("switch", {
        name: "Auto Review & Fix until passing",
      }),
    ).not.toBeInTheDocument();
  });

  it("pre-enables an existing override without sending another arm request", async () => {
    const preview: AgentWorkspaceReviewStartPreview = {
      success: true,
      target: null,
      willDisableAutoMerge: false,
      prNumber: null,
      mergeMethod: null,
      restoreAfterPublish: false,
      confirmation: {
        targetScope: "workspace_delta",
        diffFingerprint: "fingerprint",
        headSha: "head",
        prNumber: null,
        willDisableAutoMerge: false,
        mergeMethod: null,
        restoreAfterPublish: false,
      },
    };
    vi.mocked(chatApi.getAgentWorkspaceReviewStartPreview).mockResolvedValue(preview);
    const onStartReview = vi.fn().mockResolvedValue(undefined);
    const user = userEvent.setup();
    render(
      <Harness
        onStartReview={onStartReview}
        reviewAutomation={{ effectiveLoopActive: false, overrideOn: true }}
      />,
    );

    await user.click(screen.getByRole("button", { name: "Run review" }));
    const dialog = await screen.findByRole("alertdialog");
    expect(
      await within(dialog).findByRole("switch", {
        name: "Auto Review & Fix until passing",
      }),
    ).toBeChecked();
    await user.click(within(dialog).getByRole("button", { name: "Start review" }));

    await waitFor(() =>
      expect(onStartReview).toHaveBeenCalledWith({
        force: false,
        confirmation: preview.confirmation,
        runtimeOverride: reviewerRuntime,
      }),
    );
  });

  it("refreshes a stale receipt and requires a new confirmation after a start conflict", async () => {
    const preview: AgentWorkspaceReviewStartPreview = {
      success: true,
      target: {
        scope: "workspace_delta",
        baseRef: "main",
        baseSha: "base",
        headRef: "HEAD",
        headSha: "head",
        diffFingerprint: "fingerprint",
        sourcePullRequestNumber: null,
      },
      willDisableAutoMerge: true,
      prNumber: 42,
      mergeMethod: "squash",
      restoreAfterPublish: true,
      confirmation: {
        targetScope: "workspace_delta",
        diffFingerprint: "fingerprint",
        headSha: "head",
        prNumber: 42,
        willDisableAutoMerge: true,
        mergeMethod: "squash",
        restoreAfterPublish: true,
      },
    };
    const refreshedPreview: AgentWorkspaceReviewStartPreview = {
      ...preview,
      prNumber: 43,
      confirmation: {
        ...preview.confirmation,
        diffFingerprint: "refreshed-fingerprint",
        prNumber: 43,
      },
    };
    const conflict = new AgentWorkspaceHttpError(
      409,
      "Conflict",
      "Workspace Review receipt is stale",
    );
    vi.mocked(chatApi.getAgentWorkspaceReviewStartPreview)
      .mockResolvedValueOnce(preview)
      .mockResolvedValueOnce(refreshedPreview);
    const onStartReview = vi
      .fn()
      .mockRejectedValueOnce(conflict)
      .mockResolvedValueOnce(undefined);
    const user = userEvent.setup();

    render(<Harness onStartReview={onStartReview} />);
    await user.click(screen.getByRole("button", { name: "Run review" }));

    const dialog = await screen.findByRole("alertdialog");
    await user.click(
      await within(dialog).findByRole("button", { name: "Start review" }),
    );

    await waitFor(() => {
      expect(chatApi.getAgentWorkspaceReviewStartPreview).toHaveBeenCalledTimes(2);
      expect(
        within(dialog).getByText(/GitHub auto-merge is enabled on PR #43/),
      ).toBeInTheDocument();
    });
    expect(within(dialog).getByRole("button", { name: "Start review" })).toBeEnabled();
    expect(onStartReview).toHaveBeenCalledTimes(1);

    await user.click(within(dialog).getByRole("button", { name: "Start review" }));

    await waitFor(() => {
      expect(onStartReview).toHaveBeenLastCalledWith({
        force: false,
        confirmation: refreshedPreview.confirmation,
        runtimeOverride: reviewerRuntime,
      });
    });
  });

  it("refreshes a stale blocker receipt and requires reconfirmation before retrying Fix Issues", async () => {
    const context: AgentWorkspaceReviewContext = {
      success: true,
      workspace: {} as AgentWorkspaceReviewContext["workspace"],
      events: [],
      target: {
        scope: "workspace_delta",
        baseRef: "main",
        baseSha: "base",
        headRef: "HEAD",
        headSha: "head",
        diffFingerprint: "old-diff",
        sourcePullRequestNumber: null,
      },
      monitor: {
        reviewArtifactId: "artifact-old",
        reviewArtifactVersion: 1,
        reviewBlockingFingerprint: "old-blocker",
        reviewBlockingSummary: "Old finding",
      } as AgentWorkspaceReviewContext["monitor"],
      reviewArtifactIsCurrent: true,
      reviewArtifactIsOutdated: false,
      canMutateReviewState: true,
      reviewRuntimeState: "active_owned",
      isCurrent: true,
      isOutdated: false,
      shouldShowTab: true,
    };
    const refreshedContext: AgentWorkspaceReviewContext = {
      ...context,
      target: { ...context.target!, diffFingerprint: "new-diff" },
      monitor: {
        ...context.monitor,
        reviewArtifactId: "artifact-new",
        reviewArtifactVersion: 2,
        reviewBlockingFingerprint: "new-blocker",
        reviewBlockingSummary: "New finding",
      },
    };
    const conflict = new AgentWorkspaceHttpError(
      409,
      "Conflict",
      "Workspace Review blocker changed",
    );
    vi.mocked(chatApi.getAgentWorkspaceReviewContext).mockResolvedValue(
      refreshedContext,
    );
    const onStartFixer = vi
      .fn()
      .mockRejectedValueOnce(conflict)
      .mockResolvedValueOnce(undefined);
    const user = userEvent.setup();

    render(<FixerHarness context={context} onStartFixer={onStartFixer} />);
    await user.click(screen.getByRole("button", { name: "Fix issues" }));

    const dialog = await screen.findByRole("alertdialog");
    await user.click(await within(dialog).findByRole("button", { name: "Fix issues" }));

    await waitFor(() => {
      expect(chatApi.getAgentWorkspaceReviewContext).toHaveBeenCalledWith(
        "conversation-1",
        { refreshTarget: true },
      );
      expect(within(dialog).getByText(/New finding/)).toBeInTheDocument();
    });
    expect(within(dialog).getByRole("button", { name: "Fix issues" })).toBeEnabled();
    expect(onStartFixer).toHaveBeenCalledTimes(1);

    await user.click(within(dialog).getByRole("button", { name: "Fix issues" }));

    await waitFor(() => {
      expect(onStartFixer).toHaveBeenLastCalledWith({
        confirmation: {
          targetScope: "workspace_delta",
          diffFingerprint: "new-diff",
          artifactId: "artifact-new",
          artifactVersion: 2,
          blockingFingerprint: "new-blocker",
        },
        runtimeOverride: reviewerRuntime,
      });
    });
    for (const phase of [
      "load_role_defaults",
      "refresh_provider_runtime",
      "prepare_description",
      "prepare_completed",
      "confirm_action",
    ]) {
      expect(logger.debug).toHaveBeenCalledWith(
        "[RoleRuntimeConfirmationTiming]",
        expect.objectContaining({
          role: "workspace_repair",
          phase,
          elapsedMs: expect.any(Number),
          totalElapsedMs: expect.any(Number),
          outcome: "completed",
        }),
      );
    }
  });

  it("keeps fixer recovery open and disabled when authoritative refetch fails", async () => {
    const context: AgentWorkspaceReviewContext = {
      success: true,
      workspace: {} as AgentWorkspaceReviewContext["workspace"],
      events: [],
      target: {
        scope: "workspace_delta",
        baseRef: "main",
        baseSha: "base",
        headRef: "HEAD",
        headSha: "head",
        diffFingerprint: "old-diff",
        sourcePullRequestNumber: null,
      },
      monitor: {
        reviewArtifactId: "artifact-old",
        reviewArtifactVersion: 1,
        reviewBlockingFingerprint: "old-blocker",
        reviewBlockingSummary: "Old finding",
      } as AgentWorkspaceReviewContext["monitor"],
      reviewArtifactIsCurrent: true,
      reviewArtifactIsOutdated: false,
      canMutateReviewState: true,
      reviewRuntimeState: "active_owned",
      isCurrent: true,
      isOutdated: false,
      shouldShowTab: true,
    };
    vi.mocked(chatApi.getAgentWorkspaceReviewContext).mockRejectedValue(
      new Error("backend offline"),
    );
    const onStartFixer = vi.fn().mockRejectedValue(
      new AgentWorkspaceHttpError(409, "Conflict", "stale receipt"),
    );
    const user = userEvent.setup();

    render(<FixerHarness context={context} onStartFixer={onStartFixer} />);
    await user.click(screen.getByRole("button", { name: "Fix issues" }));
    const dialog = await screen.findByRole("alertdialog");
    await user.click(await within(dialog).findByRole("button", { name: "Fix issues" }));

    await waitFor(() => {
      expect(
        within(dialog).getByText("Could not prepare this action. Cancel and try again."),
      ).toBeInTheDocument();
    });
    expect(within(dialog).getByRole("button", { name: "Fix issues" })).toBeDisabled();
    expect(within(dialog).getByRole("button", { name: "Cancel" })).toBeEnabled();
  });

  it("renders an actionable disabled state when preview finds unfinished Git state", async () => {
    vi.mocked(chatApi.getAgentWorkspaceReviewStartPreview).mockRejectedValue(
      new AgentWorkspaceHttpError(409, "Conflict", unfinishedGitDetail),
    );
    const onStartReview = vi.fn().mockResolvedValue(undefined);
    const user = userEvent.setup();

    render(<Harness onStartReview={onStartReview} />);
    await user.click(screen.getByRole("button", { name: "Run review" }));
    const dialog = await screen.findByRole("alertdialog");

    await waitFor(() => {
      expect(within(dialog).getByText(unfinishedGitDetail)).toBeInTheDocument();
    });
    expect(within(dialog).getByRole("button", { name: "Start review" })).toBeDisabled();
    expect(within(dialog).getByRole("button", { name: "Cancel" })).toBeEnabled();
    expect(onStartReview).not.toHaveBeenCalled();
  });

  it("keeps the dialog blocked when a post conflict refetch finds unfinished Git state", async () => {
    const preview = {
      success: true,
      target: null,
      willDisableAutoMerge: false,
      prNumber: null,
      mergeMethod: null,
      restoreAfterPublish: false,
      confirmation: {
        targetScope: null,
        diffFingerprint: null,
        headSha: null,
        prNumber: null,
        willDisableAutoMerge: false,
        mergeMethod: null,
        restoreAfterPublish: false,
      },
    } satisfies AgentWorkspaceReviewStartPreview;
    const initialConflict = new AgentWorkspaceHttpError(409, "Conflict", "stale receipt");
    const blockedConflict = new AgentWorkspaceHttpError(
      409,
      "Conflict",
      unfinishedGitDetail,
    );
    vi.mocked(chatApi.getAgentWorkspaceReviewStartPreview)
      .mockResolvedValueOnce(preview)
      .mockRejectedValueOnce(blockedConflict);
    const onStartReview = vi.fn().mockRejectedValue(initialConflict);
    const user = userEvent.setup();

    render(<Harness onStartReview={onStartReview} />);
    await user.click(screen.getByRole("button", { name: "Run review" }));
    const dialog = await screen.findByRole("alertdialog");
    await waitFor(() => {
      expect(within(dialog).getByRole("button", { name: "Start review" })).toBeEnabled();
    });
    await user.click(within(dialog).getByRole("button", { name: "Start review" }));

    await waitFor(() => {
      expect(within(dialog).getByText(unfinishedGitDetail)).toBeInTheDocument();
    });
    expect(chatApi.getAgentWorkspaceReviewStartPreview).toHaveBeenCalledTimes(2);
    expect(within(dialog).getByRole("button", { name: "Start review" })).toBeDisabled();
    expect(within(dialog).getByRole("button", { name: "Cancel" })).toBeEnabled();
  });

  it("uses generic disabled copy when a post conflict refetch fails without a 409 detail", async () => {
    const preview = {
      success: true,
      target: null,
      willDisableAutoMerge: false,
      prNumber: null,
      mergeMethod: null,
      restoreAfterPublish: false,
      confirmation: {
        targetScope: null,
        diffFingerprint: null,
        headSha: null,
        prNumber: null,
        willDisableAutoMerge: false,
        mergeMethod: null,
        restoreAfterPublish: false,
      },
    } satisfies AgentWorkspaceReviewStartPreview;
    vi.mocked(chatApi.getAgentWorkspaceReviewStartPreview)
      .mockResolvedValueOnce(preview)
      .mockRejectedValueOnce(new Error("backend offline"));
    const onStartReview = vi
      .fn()
      .mockRejectedValue(new AgentWorkspaceHttpError(409, "Conflict", "stale receipt"));
    const user = userEvent.setup();

    render(<Harness onStartReview={onStartReview} />);
    await user.click(screen.getByRole("button", { name: "Run review" }));
    const dialog = await screen.findByRole("alertdialog");
    await waitFor(() => {
      expect(within(dialog).getByRole("button", { name: "Start review" })).toBeEnabled();
    });
    await user.click(within(dialog).getByRole("button", { name: "Start review" }));

    await waitFor(() => {
      expect(
        within(dialog).getByText("Could not prepare this action. Cancel and try again."),
      ).toBeInTheDocument();
    });
    expect(within(dialog).getByRole("button", { name: "Start review" })).toBeDisabled();
  });

  it("refreshes a stale blocker receipt and requires reconfirmation before retrying Fix Issues", async () => {
    const context: AgentWorkspaceReviewContext = {
      success: true,
      workspace: {} as AgentWorkspaceReviewContext["workspace"],
      events: [],
      target: {
        scope: "workspace_delta",
        baseRef: "main",
        baseSha: "base",
        headRef: "HEAD",
        headSha: "head",
        diffFingerprint: "old-diff",
        sourcePullRequestNumber: null,
      },
      monitor: {
        reviewArtifactId: "artifact-old",
        reviewArtifactVersion: 1,
        reviewBlockingFingerprint: "old-blocker",
        reviewBlockingSummary: "Old finding",
      } as AgentWorkspaceReviewContext["monitor"],
      reviewArtifactIsCurrent: true,
      reviewArtifactIsOutdated: false,
      canMutateReviewState: true,
      reviewRuntimeState: "active_owned",
      isCurrent: true,
      isOutdated: false,
      shouldShowTab: true,
    };
    const refreshedContext: AgentWorkspaceReviewContext = {
      ...context,
      target: { ...context.target!, diffFingerprint: "new-diff" },
      monitor: {
        ...context.monitor,
        reviewArtifactId: "artifact-new",
        reviewArtifactVersion: 2,
        reviewBlockingFingerprint: "new-blocker",
        reviewBlockingSummary: "New finding",
      },
    };
    const conflict = new AgentWorkspaceHttpError(
      409,
      "Conflict",
      "Workspace Review blocker changed",
    );
    vi.mocked(chatApi.getAgentWorkspaceReviewContext).mockResolvedValue(
      refreshedContext,
    );
    const onStartFixer = vi
      .fn()
      .mockRejectedValueOnce(conflict)
      .mockResolvedValueOnce(undefined);
    const user = userEvent.setup();

    render(<FixerHarness context={context} onStartFixer={onStartFixer} />);
    await user.click(screen.getByRole("button", { name: "Fix issues" }));

    const dialog = await screen.findByRole("alertdialog");
    await user.click(await within(dialog).findByRole("button", { name: "Fix issues" }));

    await waitFor(() => {
      expect(chatApi.getAgentWorkspaceReviewContext).toHaveBeenCalledWith(
        "conversation-1",
        { refreshTarget: true },
      );
      expect(within(dialog).getByText(/New finding/)).toBeInTheDocument();
    });
    expect(within(dialog).getByRole("button", { name: "Fix issues" })).toBeEnabled();
    expect(onStartFixer).toHaveBeenCalledTimes(1);

    await user.click(within(dialog).getByRole("button", { name: "Fix issues" }));

    await waitFor(() => {
      expect(onStartFixer).toHaveBeenLastCalledWith({
        confirmation: {
          targetScope: "workspace_delta",
          diffFingerprint: "new-diff",
          artifactId: "artifact-new",
          artifactVersion: 2,
          blockingFingerprint: "new-blocker",
        },
        runtimeOverride: reviewerRuntime,
      });
    });
  });

  it("renders an actionable disabled state when preview finds unfinished Git state", async () => {
    vi.mocked(chatApi.getAgentWorkspaceReviewStartPreview).mockRejectedValue(
      new AgentWorkspaceHttpError(409, "Conflict", unfinishedGitDetail),
    );
    const onStartReview = vi.fn().mockResolvedValue(undefined);
    const user = userEvent.setup();

    render(<Harness onStartReview={onStartReview} />);
    await user.click(screen.getByRole("button", { name: "Run review" }));
    const dialog = await screen.findByRole("alertdialog");

    await waitFor(() => {
      expect(within(dialog).getByText(unfinishedGitDetail)).toBeInTheDocument();
    });
    expect(within(dialog).getByRole("button", { name: "Start review" })).toBeDisabled();
    expect(within(dialog).getByRole("button", { name: "Cancel" })).toBeEnabled();
    expect(onStartReview).not.toHaveBeenCalled();
  });

  it("keeps the dialog blocked when a post conflict refetch finds unfinished Git state", async () => {
    const preview = {
      success: true,
      target: null,
      willDisableAutoMerge: false,
      prNumber: null,
      mergeMethod: null,
      restoreAfterPublish: false,
      confirmation: {
        targetScope: null,
        diffFingerprint: null,
        headSha: null,
        prNumber: null,
        willDisableAutoMerge: false,
        mergeMethod: null,
        restoreAfterPublish: false,
      },
    } satisfies AgentWorkspaceReviewStartPreview;
    const initialConflict = new AgentWorkspaceHttpError(409, "Conflict", "stale receipt");
    const blockedConflict = new AgentWorkspaceHttpError(
      409,
      "Conflict",
      unfinishedGitDetail,
    );
    vi.mocked(chatApi.getAgentWorkspaceReviewStartPreview)
      .mockResolvedValueOnce(preview)
      .mockRejectedValueOnce(blockedConflict);
    const onStartReview = vi.fn().mockRejectedValue(initialConflict);
    const user = userEvent.setup();

    render(<Harness onStartReview={onStartReview} />);
    await user.click(screen.getByRole("button", { name: "Run review" }));
    const dialog = await screen.findByRole("alertdialog");
    await waitFor(() => {
      expect(within(dialog).getByRole("button", { name: "Start review" })).toBeEnabled();
    });
    await user.click(within(dialog).getByRole("button", { name: "Start review" }));

    await waitFor(() => {
      expect(within(dialog).getByText(unfinishedGitDetail)).toBeInTheDocument();
    });
    expect(chatApi.getAgentWorkspaceReviewStartPreview).toHaveBeenCalledTimes(2);
    expect(within(dialog).getByRole("button", { name: "Start review" })).toBeDisabled();
    expect(within(dialog).getByRole("button", { name: "Cancel" })).toBeEnabled();
  });

  it("uses generic disabled copy when a post conflict refetch fails without a 409 detail", async () => {
    const preview = {
      success: true,
      target: null,
      willDisableAutoMerge: false,
      prNumber: null,
      mergeMethod: null,
      restoreAfterPublish: false,
      confirmation: {
        targetScope: null,
        diffFingerprint: null,
        headSha: null,
        prNumber: null,
        willDisableAutoMerge: false,
        mergeMethod: null,
        restoreAfterPublish: false,
      },
    } satisfies AgentWorkspaceReviewStartPreview;
    vi.mocked(chatApi.getAgentWorkspaceReviewStartPreview)
      .mockResolvedValueOnce(preview)
      .mockRejectedValueOnce(new Error("backend offline"));
    const onStartReview = vi
      .fn()
      .mockRejectedValue(new AgentWorkspaceHttpError(409, "Conflict", "stale receipt"));
    const user = userEvent.setup();

    render(<Harness onStartReview={onStartReview} />);
    await user.click(screen.getByRole("button", { name: "Run review" }));
    const dialog = await screen.findByRole("alertdialog");
    await waitFor(() => {
      expect(within(dialog).getByRole("button", { name: "Start review" })).toBeEnabled();
    });
    await user.click(within(dialog).getByRole("button", { name: "Start review" }));

    await waitFor(() => {
      expect(
        within(dialog).getByText("Could not prepare this action. Cancel and try again."),
      ).toBeInTheDocument();
    });
    expect(within(dialog).getByRole("button", { name: "Start review" })).toBeDisabled();
  });
});
