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
      onStartReview,
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
  const unfinishedGitDetail =
    "Resolve conflicts and complete or abort the merge or rebase before retrying Workspace Review.";

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
    await user.click(within(dialog).getByRole("button", { name: "Start review" }));

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
