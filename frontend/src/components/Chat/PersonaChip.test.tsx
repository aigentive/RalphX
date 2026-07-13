import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi, beforeEach } from "vitest";

import { TooltipProvider } from "@/components/ui/tooltip";
import { PersonaChip } from "./PersonaChip";

const mockMutateAsync = vi.fn();
const mockConfirm = vi.fn();
const mockPersonaQuery = vi.fn(() => ({
  data: [
    { id: "reviewer", slug: "reviewer", name: "Reviewer Voice", status: "active" },
    { id: "architect", slug: "architect", name: "Terse Architect", status: "active" },
    { id: "75b56f7e-d80a-4648-835c-c5b10a8b6df7", slug: "design-voice", name: "Archived Voice", status: "archived" },
  ],
  isLoading: false,
  isError: false,
}));

vi.mock("@/hooks/usePersonas", () => ({
  usePersonas: () => mockPersonaQuery(),
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
    mockPersonaQuery.mockReturnValue({
      data: [
        { id: "reviewer", slug: "reviewer", name: "Reviewer Voice", status: "active" },
        { id: "architect", slug: "architect", name: "Terse Architect", status: "active" },
        { id: "75b56f7e-d80a-4648-835c-c5b10a8b6df7", slug: "design-voice", name: "Archived Voice", status: "archived" },
      ],
      isLoading: false,
      isError: false,
    });
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

  it("uses the generic label when no active persona is bound", () => {
    renderChip({ personaId: null });

    expect(screen.getByRole("button", { name: "Switch conversation persona" })).toHaveTextContent("No persona");
    expect(screen.getByRole("button", { name: "Switch conversation persona" })).not.toHaveTextContent("Reviewer Voice");
  });

  it("shows the applied persona slug and opens the existing switcher", () => {
    renderChip({
      lastRunPersonaId: "reviewer",
      lastRunPersonaSlug: "reviewer",
      lastRunPersonaInjected: true,
    });

    const trigger = screen.getByRole("button", {
      name: "Switch conversation persona",
    });
    expect(trigger).toHaveTextContent("reviewer");

    fireEvent.click(trigger);
    expect(
      screen.getByRole("menu", { name: "Conversation persona" }),
    ).toBeInTheDocument();
  });

  it("warns when the bound persona was not applied to the last run", async () => {
    renderChip({
      lastRunPersonaId: "reviewer",
      lastRunPersonaSlug: "reviewer",
      lastRunPersonaInjected: false,
      lastRunPersonaSkippedReason: "native_agent_flag",
    });

    const trigger = screen.getByRole("button", {
      name: "Switch conversation persona",
    });
    expect(trigger).toHaveTextContent("reviewer not applied");
    fireEvent.pointerMove(trigger);
    expect(
      await screen.findByRole("tooltip", {
        name: "Native agent mode does not support personas",
      }),
    ).toBeInTheDocument();
  });

  it("renders an archived bound persona slug from the last run and never its raw id", async () => {
    const archivedPersonaId = "75b56f7e-d80a-4648-835c-c5b10a8b6df7";
    renderChip({
      personaId: archivedPersonaId,
      lastRunPersonaId: archivedPersonaId,
      lastRunPersonaSlug: "design-voice",
    });

    const trigger = screen.getByRole("button", {
      name: "Switch conversation persona",
    });
    expect(trigger).toHaveTextContent("design-voice (archived)");
    // The raw persona id must never leak into the label.
    expect(trigger).not.toHaveTextContent(archivedPersonaId);
    fireEvent.pointerMove(trigger);
    expect(
      await screen.findByRole("tooltip", {
        name: "design-voice is archived. It remains attributed to the last run.",
      }),
    ).toBeInTheDocument();
  });

  it("does not fabricate archived state while persona reads are unknown", () => {
    mockPersonaQuery.mockReturnValue({
      data: [],
      isLoading: false,
      isError: true,
    });

    renderChip({
      personaId: "reviewer",
      lastRunPersonaSlug: "stale-run-persona",
    });

    const trigger = screen.getByRole("button", {
      name: "Switch conversation persona",
    });
    expect(trigger).toHaveTextContent("No persona");
    expect(trigger).not.toHaveTextContent("archived");
    expect(trigger).not.toHaveTextContent("stale-run-persona");
  });

  it("keeps the picker open and reports a failed persona switch", async () => {
    mockMutateAsync.mockRejectedValueOnce(new Error("Persona is archived"));
    renderChip();
    fireEvent.click(screen.getByRole("button", { name: "Switch conversation persona" }));
    fireEvent.click(screen.getByRole("menuitemradio", { name: "Terse Architect" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("Persona is archived");
    expect(screen.getByRole("menu", { name: "Conversation persona" })).toBeInTheDocument();
  });
});
