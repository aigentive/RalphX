import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  confirm: vi.fn(),
  exit: vi.fn(),
}));

vi.mock("@/hooks/useConfirmation", () => ({
  useConfirmation: () => ({
    confirm: mocks.confirm,
    confirmationDialogProps: {},
    ConfirmationDialog: () => null,
  }),
}));

vi.mock("@/hooks/useManagedTeam", () => ({
  useManagedTeamMemberActions: () => ({
    addMember: { isPending: false, mutateAsync: vi.fn() },
    assignMember: { isPending: false, mutateAsync: vi.fn() },
    stopMember: { isPending: false, mutateAsync: vi.fn() },
    exit: { isPending: false, mutateAsync: mocks.exit },
  }),
}));

import { TeamMemberActions } from "./TeamMemberActions";

describe("TeamMemberActions", () => {
  beforeEach(() => {
    mocks.confirm.mockReset().mockResolvedValue(true);
    mocks.exit.mockReset().mockResolvedValue(undefined);
  });

  it("confirms and sends the selected staged Team exit through trusted authority", async () => {
    render(
      <TeamMemberActions
        conversationId="conversation-1"
        authority={{ conversationId: "conversation-1", agentRunId: "run-1" }}
        members={[]}
        tasks={[]}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Suspend Team" }));
    await waitFor(() => {
      expect(mocks.exit).toHaveBeenCalledWith({
        authority: { conversationId: "conversation-1", agentRunId: "run-1" },
        action: "suspend",
      });
    });

    fireEvent.click(screen.getByRole("button", { name: "Drain and close" }));
    await waitFor(() => {
      expect(mocks.exit).toHaveBeenCalledWith({
        authority: { conversationId: "conversation-1", agentRunId: "run-1" },
        action: "drain_and_close",
      });
    });
  });
});
