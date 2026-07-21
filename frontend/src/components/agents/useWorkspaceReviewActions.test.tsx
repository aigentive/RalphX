import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import {
  AgentWorkspaceHttpError,
  chatApi,
  type AgentWorkspaceReviewStartConfirmation,
  type AgentWorkspaceReviewStartPreview,
} from "@/api/chat";

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
      getAgentWorkspaceReviewStartPreview: vi.fn(),
    },
  };
});

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
      ],
    }),
  },
}));

function Harness({
  onStartReview,
}: {
  onStartReview: (input: {
    force: boolean;
    confirmation: AgentWorkspaceReviewStartConfirmation;
  }) => Promise<unknown>;
}) {
  const { startReview, confirmationDialogProps, ConfirmationDialog } =
    useWorkspaceReviewActions({
      conversationId: "conversation-1",
      projectId: "project-1",
      onStartReview,
      onStartFixer: vi.fn(),
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

describe("useWorkspaceReviewActions", () => {
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
});
