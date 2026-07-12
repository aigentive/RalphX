import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi, beforeEach } from "vitest";

import { TooltipProvider } from "@/components/ui/tooltip";
import { PersonaChip } from "./PersonaChip";

const mockMutateAsync = vi.fn();
const mockConfirm = vi.fn();

vi.mock("@/hooks/usePersonas", () => ({
  usePersonas: () => ({
    data: [
      { id: "reviewer", name: "Reviewer Voice", status: "active" },
      { id: "architect", name: "Terse Architect", status: "active" },
      { id: "archived", name: "Archived Voice", status: "archived" },
    ],
    isLoading: false,
  }),
  useSwitchConversationPersona: () => ({ mutateAsync: mockMutateAsync }),
}));

vi.mock("@/hooks/useConfirmation", () => ({
  useConfirmation: () => ({
    confirm: mockConfirm,
    confirmationDialogProps: {},
    ConfirmationDialog: () => null,
  }),
}));

function renderChip(props: Partial<React.ComponentProps<typeof PersonaChip>> = {}) {
  return render(
    <TooltipProvider delayDuration={0}>
      <PersonaChip
        conversationId="conversation-1"
        personaId="reviewer"
        isAgentRunning={false}
        {...props}
      />
    </TooltipProvider>,
  );
}

describe("PersonaChip", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockMutateAsync.mockResolvedValue(undefined);
    mockConfirm.mockResolvedValue(true);
  });

  it("uses the exact project-only tooltip and an accessible chip trigger", async () => {
    renderChip();

    const trigger = screen.getByRole("button", {
      name: "Switch conversation persona",
    });
    fireEvent.pointerMove(trigger);

    expect(
      await screen.findByRole("tooltip", {
        name: "Applies to this conversation only — not to delegated, subagent, or pipeline work in v1.",
      }),
    ).toBeInTheDocument();
  });

  it("lists active personas only and removes the binding immediately while idle", async () => {
    renderChip();
    fireEvent.click(screen.getByRole("button", { name: "Switch conversation persona" }));

    expect(screen.getByRole("menuitemradio", { name: "Reviewer Voice" })).toBeInTheDocument();
    expect(screen.getByRole("menuitemradio", { name: "Terse Architect" })).toBeInTheDocument();
    expect(screen.queryByRole("menuitemradio", { name: "Archived Voice" })).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("menuitem", { name: "Remove persona" }));

    expect(mockConfirm).not.toHaveBeenCalled();
    expect(mockMutateAsync).toHaveBeenCalledWith({
      conversationId: "conversation-1",
      personaId: null,
    });
  });

  it("confirms the exact warning before switching a running conversation", async () => {
    renderChip({ isAgentRunning: true });
    fireEvent.click(screen.getByRole("button", { name: "Switch conversation persona" }));
    fireEvent.click(screen.getByRole("menuitemradio", { name: "Terse Architect" }));

    expect(mockConfirm).toHaveBeenCalledWith(
      expect.objectContaining({
        description:
          "Changing the persona stops the current run. Conversation history is preserved and the next message resumes the same session.",
      }),
    );
    await waitFor(() => {
      expect(mockMutateAsync).toHaveBeenCalledWith({
        conversationId: "conversation-1",
        personaId: "architect",
      });
    });
  });
});
