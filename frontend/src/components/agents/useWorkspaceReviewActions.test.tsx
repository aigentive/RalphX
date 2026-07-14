import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import {
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
});
