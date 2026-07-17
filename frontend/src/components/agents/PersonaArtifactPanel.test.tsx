import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { TooltipProvider } from "@/components/ui/tooltip";
import { FEATURE_FLAGS_QUERY_KEY } from "@/hooks/useFeatureFlags";
import { EventProvider } from "@/providers/EventProvider";
import { useAgentSessionStore } from "@/stores/agentSessionStore";
import { useUiStore } from "@/stores/uiStore";
import type { ChatConversation } from "@/types/chat-conversation";
import type { FeatureFlags } from "@/types/feature-flags";

import { PersonaArtifactPanel } from "./PersonaArtifactPanel";

const rawDraft = {
  id: "draft-1",
  artifact_id: "artifact-1",
  project_id: null,
  slug: "support-voice",
  name: "Support Voice",
  description: "Calm customer support.",
  content: "## Voice\n\nEmpathetic, direct.",
  status: "draft",
  version: 3,
  content_hash: "hash-3",
  source_session_id: "conversation-1",
  source_persona_id: null,
  source_content_hash: null,
  created_at: "2026-07-17T08:00:00Z",
  updated_at: "2026-07-17T10:00:00Z",
};

const rawApproved = {
  ...rawDraft,
  id: "persona-1",
  status: "active",
};

const rawHistory = [
  {
    id: "artifact-3",
    version: 3,
    name: "Support Voice",
    created_at: "2026-07-17T10:00:00Z",
    created_by: "user",
    metadata: { persona_version: 3, created_by: "user" },
  },
  {
    id: "artifact-2",
    version: 2,
    name: "Support Voice",
    created_at: "2026-07-17T09:00:00Z",
    created_by: "agent",
    metadata: { persona_version: 2, created_by: "agent" },
  },
  {
    id: "artifact-1",
    version: 1,
    name: "Support Voice",
    created_at: "2026-07-17T08:00:00Z",
    created_by: "agent",
    metadata: { persona_version: 1, created_by: "agent" },
  },
];

function conversation(
  overrides: Partial<ChatConversation> = {},
): ChatConversation {
  return {
    id: "conversation-1",
    contextType: "project",
    contextId: "project-1",
    agentMode: "persona_builder",
    builderDraftId: null,
    builderResultPersonaId: null,
    ...overrides,
  } as ChatConversation;
}

function createQueryClient() {
  return new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
}

const featureFlags: FeatureFlags = {
  activityPage: true,
  extensibilityPage: true,
  ideationPage: false,
  automationsPage: true,
  battleMode: true,
  teamMode: false,
  atlassianOauth: false,
  ticketingDashboard: false,
  agentPersonas: true,
  standaloneConversations: true,
};

function renderPanel(
  value: ChatConversation,
  queryClient = createQueryClient(),
  standaloneConversations = true,
) {
  queryClient.setQueryData(FEATURE_FLAGS_QUERY_KEY, {
    ...featureFlags,
    standaloneConversations,
  });
  function Wrapper({ children }: { children: ReactNode }) {
    return (
      <QueryClientProvider client={queryClient}>
        <TooltipProvider delayDuration={0}>
          <EventProvider>{children}</EventProvider>
        </TooltipProvider>
      </QueryClientProvider>
    );
  }

  return {
    queryClient,
    ...render(<PersonaArtifactPanel conversation={value} />, { wrapper: Wrapper }),
  };
}

function mockPersonaQueries(persona = rawDraft) {
  vi.mocked(invoke).mockImplementation(async (command) => {
    if (command === "get_persona") return persona;
    if (command === "get_artifact_version_history") return rawHistory;
    if (command === "get_artifact_at_version") {
      return {
        id: "artifact-1",
        name: "Support Voice",
        artifact_type: "persona",
        content_type: "inline",
        content: "## Voice\n\nOriginal agent draft.",
        created_at: "2026-07-17T08:00:00Z",
        created_by: "agent",
        version: 1,
        bucket_id: "persona-library",
        task_id: null,
        process_id: null,
        derived_from: [],
      };
    }
    return null;
  });
}

describe("PersonaArtifactPanel", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useAgentSessionStore.setState({
      focusedProjectId: null,
      selectedProjectId: null,
      selectedConversationId: null,
      startConversationDraft: null,
    });
    useUiStore.setState({ activeModal: null, modalContext: undefined });
  });

  it("renders empty state without persona actions when no binding exists", () => {
    renderPanel(conversation());

    expect(screen.getByRole("heading", { name: "Persona" })).toBeInTheDocument();
    expect(
      screen.getByText("The agent will draft the persona here after its first pass"),
    ).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Approve persona/ })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Open in Settings" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Refine with Agent" })).not.toBeInTheDocument();
  });

  it("renders draft state with current actions and no approved-only actions", async () => {
    mockPersonaQueries();
    renderPanel(conversation({ builderDraftId: "draft-1" }));

    expect(await screen.findByText("Empathetic, direct.")).toBeInTheDocument();
    expect(screen.getByText("Global")).toBeInTheDocument();
    expect(screen.getByText("Draft")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Approve persona" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Open in Settings" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Refine with Agent" })).not.toBeInTheDocument();
  });

  it("renders approved and archived states with the correct action availability", async () => {
    mockPersonaQueries(rawApproved);
    const { rerender } = renderPanel(
      conversation({ builderResultPersonaId: "persona-1" }),
    );

    expect(await screen.findByText("Approved")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Open in Settings" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Refine with Agent" })).toBeEnabled();
    expect(screen.queryByRole("button", { name: /Approve persona/ })).not.toBeInTheDocument();

    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "get_persona") return { ...rawApproved, status: "archived" };
      if (command === "get_artifact_version_history") return rawHistory;
      return null;
    });
    rerender(
      <PersonaArtifactPanel
        conversation={conversation({ builderResultPersonaId: "archived-persona" })}
      />,
    );

    expect(await screen.findByText("Archived")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Open in Settings" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Refine with Agent" })).toBeDisabled();
    expect(screen.queryByRole("button", { name: /Approve persona/ })).not.toBeInTheDocument();
  });

  it("labels version attribution and makes historical selection read-only", async () => {
    mockPersonaQueries();
    renderPanel(conversation({ builderDraftId: "draft-1" }));

    const versions = await screen.findByLabelText("Persona version");
    expect(screen.getByRole("option", { name: /v3 you \(manual edit\)/ })).toBeInTheDocument();
    expect(screen.getByRole("option", { name: /v2 agent/ })).toBeInTheDocument();
    fireEvent.change(versions, { target: { value: "1" } });

    expect(await screen.findByText("Original agent draft.")).toBeInTheDocument();
    expect(screen.getByText("Historical version · read-only")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Approve persona" })).not.toBeInTheDocument();
  });

  it("approves through the existing command and transitions live to approved", async () => {
    const user = userEvent.setup();
    mockPersonaQueries();
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "get_persona") return rawDraft;
      if (command === "get_artifact_version_history") return rawHistory;
      if (command === "approve_persona") return rawApproved;
      return null;
    });
    const { rerender } = renderPanel(conversation({ builderDraftId: "draft-1" }));

    await user.click(await screen.findByRole("button", { name: "Approve persona" }));

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("approve_persona", {
        input: { id: "draft-1" },
      });
    });
    expect(await screen.findByText("Approved")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Approve persona" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Open in Settings" })).toBeInTheDocument();

    rerender(<PersonaArtifactPanel conversation={conversation()} />);
    expect(screen.getByText("Approved")).toBeInTheDocument();
  });

  it("offers approve-as-new only for seeded drafts", async () => {
    const user = userEvent.setup();
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "get_persona") {
        return { ...rawDraft, source_persona_id: "persona-source" };
      }
      if (command === "get_artifact_version_history") return rawHistory;
      if (command === "approve_persona_as_new") return rawApproved;
      return null;
    });
    renderPanel(conversation({ builderDraftId: "draft-1" }));

    expect(await screen.findByRole("button", { name: "Approve persona" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Approve as new" }));
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("approve_persona_as_new", {
        input: { id: "draft-1" },
      });
    });
  });

  it("reuses Settings and refine deep links for an approved persona", async () => {
    const user = userEvent.setup();
    mockPersonaQueries(rawApproved);
    renderPanel(conversation({ builderResultPersonaId: "persona-1" }));

    await user.click(await screen.findByRole("button", { name: "Open in Settings" }));
    expect(useUiStore.getState()).toMatchObject({
      activeModal: "settings",
      modalContext: { section: "personas" },
    });

    const refine = screen.getByRole("button", { name: "Refine with Agent" });
    expect(refine).toBeEnabled();
    await user.click(refine);
    expect(useAgentSessionStore.getState().startConversationDraft).toEqual({
      projectId: null,
      projectLocked: true,
      mode: "persona_builder",
      sourcePersonaId: "persona-1",
      sourcePersonaName: "Support Voice",
    });
  });

  it("blocks global refinement when standalone conversations are off without dispatching", async () => {
    const user = userEvent.setup();
    const setStartConversationDraft = vi.spyOn(
      useAgentSessionStore.getState(),
      "setStartConversationDraft",
    );
    mockPersonaQueries(rawApproved);
    renderPanel(
      conversation({ builderResultPersonaId: "persona-1" }),
      createQueryClient(),
      false,
    );

    const refine = await screen.findByRole("button", { name: "Refine with Agent" });
    expect(refine).toBeDisabled();
    await user.hover(refine.parentElement!);
    expect(await screen.findByRole("tooltip")).toHaveTextContent(
      "Global persona refinement requires standalone conversations",
    );
    fireEvent.click(refine);
    expect(useAgentSessionStore.getState().startConversationDraft).toBeNull();
    expect(setStartConversationDraft).not.toHaveBeenCalled();
  });

  it("keeps project persona refinement enabled when standalone conversations are off", async () => {
    const user = userEvent.setup();
    mockPersonaQueries({ ...rawApproved, project_id: "project-1" });
    renderPanel(
      conversation({ builderResultPersonaId: "persona-1" }),
      createQueryClient(),
      false,
    );

    const refine = await screen.findByRole("button", { name: "Refine with Agent" });
    expect(refine).toBeEnabled();
    await user.click(refine);
    expect(useAgentSessionStore.getState().startConversationDraft).toMatchObject({
      projectId: "project-1",
      sourcePersonaId: "persona-1",
    });
  });

  it("paints the Persona shell and skeleton before the persona fetch resolves", () => {
    let resolvePersona: ((value: typeof rawDraft) => void) | undefined;
    const deferredPersona = new Promise<typeof rawDraft>((resolve) => {
      resolvePersona = resolve;
    });
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "get_persona") return deferredPersona;
      return [];
    });

    renderPanel(conversation({ builderDraftId: "draft-1" }));

    expect(screen.getByRole("heading", { name: "Persona" })).toBeInTheDocument();
    expect(screen.getByLabelText("Loading persona")).toBeInTheDocument();
    expect(screen.queryByText("Empathetic, direct.")).not.toBeInTheDocument();
    expect(resolvePersona).toBeTypeOf("function");
  });
});
