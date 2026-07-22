import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi, beforeEach } from "vitest";

import { TooltipProvider } from "@/components/ui/tooltip";
import { PersonaChip } from "./PersonaChip";

const mockMutateAsync = vi.fn();
const mockConfirm = vi.fn();
const mockOverlayPreview = vi.fn();

type MockPersona = {
  id: string;
  slug: string;
  name: string;
  status: string;
  description: string;
  projectId: string | null;
  version?: number;
};

const defaultPersonas: MockPersona[] = [
  {
    id: "reviewer",
    slug: "reviewer",
    name: "Reviewer Voice",
    status: "active",
    description: "",
    projectId: null,
    version: 3,
  },
  {
    id: "architect",
    slug: "architect",
    name: "Terse Architect",
    status: "active",
    description: "",
    projectId: "project-1",
  },
  {
    id: "75b56f7e-d80a-4648-835c-c5b10a8b6df7",
    slug: "design-voice",
    name: "Archived Voice",
    status: "archived",
    description: "",
    projectId: null,
  },
  {
    id: "other-project-persona",
    slug: "other-voice",
    name: "Other Project Voice",
    status: "active",
    description: "",
    projectId: "project-2",
  },
];

const mockPersonaQuery = vi.fn(() => ({
  data: defaultPersonas,
  isLoading: false,
  isError: false,
  refetch: vi.fn(),
}));

vi.mock("@/hooks/usePersonas", () => ({
  usePersonas: () => mockPersonaQuery(),
  useSwitchConversationPersona: () => ({ mutateAsync: mockMutateAsync }),
  usePersonaOverlayPreview: (conversationId: string, enabled: boolean) =>
    mockOverlayPreview(conversationId, enabled),
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
        projectId="project-1"
        projectName="RalphX"
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
      data: defaultPersonas,
      isLoading: false,
      isError: false,
      refetch: vi.fn(),
    });
    mockMutateAsync.mockResolvedValue(undefined);
    mockConfirm.mockResolvedValue(true);
    mockOverlayPreview.mockReturnValue({
      isPending: true,
      isError: false,
      data: undefined,
      error: null,
    });
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

  it("never offers a persona outside global plus the conversation's project", () => {
    renderChip();
    fireEvent.click(screen.getByRole("button", { name: "Switch conversation persona" }));

    expect(
      screen.queryByRole("menuitemradio", { name: "Other Project Voice" }),
    ).not.toBeInTheDocument();
    expect(screen.getByRole("group", { name: "Global" })).toBeInTheDocument();
    expect(screen.getByRole("group", { name: "RalphX" })).toBeInTheDocument();
  });

  it("offers a direct project Persona Builder action when no persona is bound", async () => {
    const onBuildPersona = vi.fn();
    renderChip({ personaId: null, onBuildPersona });

    fireEvent.click(screen.getByRole("button", { name: "Switch conversation persona" }));
    fireEvent.click(
      screen.getByRole("menuitem", { name: "Create persona for this project" }),
    );

    expect(onBuildPersona).toHaveBeenCalledOnce();
    expect(
      screen.queryByRole("menu", { name: "Choose persona" }),
    ).not.toBeInTheDocument();
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

  it("shows the applied persona slug with its version and opens the switcher", () => {
    renderChip({
      lastRunPersonaId: "reviewer",
      lastRunPersonaSlug: "reviewer",
      lastRunPersonaVersion: 3,
      lastRunPersonaInjected: true,
    });

    const trigger = screen.getByRole("button", {
      name: "Switch conversation persona",
    });
    expect(trigger).toHaveTextContent("reviewer v3");
    expect(trigger).not.toHaveTextContent("not applied");
    expect(
      trigger.querySelector("svg.lucide-triangle-alert"),
    ).not.toBeInTheDocument();

    fireEvent.click(trigger);
    expect(
      screen.getByRole("menu", { name: "Choose persona" }),
    ).toBeInTheDocument();
    expect(screen.getByText("Applied last run: reviewer v3")).toBeInTheDocument();
  });

  it("treats a reason-less false persona attribution as unknown", async () => {
    renderChip({
      lastRunPersonaId: "reviewer",
      lastRunPersonaSlug: "reviewer",
      lastRunPersonaInjected: false,
      lastRunPersonaSkippedReason: null,
    });

    const trigger = screen.getByRole("button", {
      name: "Switch conversation persona",
    });
    expect(trigger).toHaveTextContent("reviewer");
    expect(trigger).not.toHaveTextContent("not applied");
    expect(
      trigger.querySelector("svg.lucide-triangle-alert"),
    ).not.toBeInTheDocument();
    fireEvent.pointerMove(trigger);
    expect(
      await screen.findByRole("tooltip", {
        name: "Applies to this conversation only — not to delegated, subagent, or pipeline work in v1.",
      }),
    ).toBeInTheDocument();
  });

  it("renders a bound persona with absent run attribution without warning", () => {
    renderChip();

    const trigger = screen.getByRole("button", {
      name: "Switch conversation persona",
    });
    expect(trigger).toHaveTextContent("reviewer");
    expect(trigger).not.toHaveTextContent("not applied");
    expect(
      trigger.querySelector("svg.lucide-triangle-alert"),
    ).not.toBeInTheDocument();
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
      refetch: vi.fn(),
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
    expect(screen.getByRole("menu", { name: "Choose persona" })).toBeInTheDocument();
  });

  it("opens the injected-prompt dialog shell before the preview resolves", async () => {
    renderChip();
    fireEvent.click(screen.getByRole("button", { name: "Switch conversation persona" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "View injected prompt" }));

    // Shell is visible synchronously while the preview query is still pending.
    expect(
      screen.getByRole("dialog", { name: "Injected persona — next send" }),
    ).toBeInTheDocument();
    expect(
      screen.getByTestId("persona-injected-prompt-loading"),
    ).toBeInTheDocument();
    // The preview query is enabled only once the dialog is open.
    expect(mockOverlayPreview).toHaveBeenLastCalledWith("conversation-1", true);
  });

  it("renders the exact rendered block returned by the preview command", () => {
    mockOverlayPreview.mockReturnValue({
      isPending: false,
      isError: false,
      data: {
        personaId: "reviewer",
        slug: "reviewer",
        version: 3,
        renderedBlock: "<ralphx_agent_persona>exact block</ralphx_agent_persona>",
        skippedReason: null,
      },
      error: null,
    });
    renderChip();
    fireEvent.click(screen.getByRole("button", { name: "Switch conversation persona" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "View injected prompt" }));

    expect(
      screen.getByTestId("persona-injected-prompt-content"),
    ).toHaveTextContent("<ralphx_agent_persona>exact block</ralphx_agent_persona>");
  });

  it("shows an explicit error card when the preview query fails", () => {
    mockOverlayPreview.mockReturnValue({
      isPending: false,
      isError: true,
      data: undefined,
      error: new Error("backend unavailable"),
    });
    renderChip();
    fireEvent.click(screen.getByRole("button", { name: "Switch conversation persona" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "View injected prompt" }));

    expect(screen.getByRole("alert")).toHaveTextContent(
      "Could not load the injected prompt: backend unavailable",
    );
  });
});
