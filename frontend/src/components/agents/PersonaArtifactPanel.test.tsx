import { LOCAL_ENVIRONMENT_ID } from "@/stores/environmentStore";
import { resetTransportEnvironmentId } from "@/lib/remote/active-environment";
import { resetQueryClient } from "@/lib/queryClient";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { TooltipProvider } from "@/components/ui/tooltip";
import { FEATURE_FLAGS_QUERY_KEY } from "@/hooks/useFeatureFlags";
import { personaKeys } from "@/hooks/usePersonas";
import { EventProvider } from "@/providers/EventProvider";
import { useAgentSessionStore } from "@/stores/agentSessionStore";
import { useUiStore } from "@/stores/uiStore";
import { useEnvironmentStore } from "@/stores/environmentStore";
import type { ChatConversation } from "@/types/chat-conversation";
import type { FeatureFlags } from "@/types/feature-flags";
import { PersonaResponseSchema, transformPersona } from "@/types/persona";

import { PersonaArtifactPanel } from "./PersonaArtifactPanel";

// Gate tests park the store on a remote environment; without this the next file in
// the same worker inherits it and resolves a different keyed QueryClient. That is
// what broke EnvironmentScopedProviders under CI sharding.
afterEach(() => {
  resetQueryClient();
  resetTransportEnvironmentId();
  useEnvironmentStore.setState({ activeEnvironmentId: LOCAL_ENVIRONMENT_ID });
});

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

const rawPersonaArtifact = {
  id: "artifact-1",
  name: "Support Voice",
  artifact_type: "persona",
  content_type: "inline",
  content:
    "---\nname: support-voice\nkind: persona\ndescription: Calm customer support.\n---\n\n# Support Voice\n\n## Voice\n\nEmpathetic, direct.",
  created_at: "2026-07-17T08:00:00Z",
  created_by: "agent",
  version: 3,
  bucket_id: "persona-library",
  task_id: null,
  process_id: null,
  derived_from: [],
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
  automationsPage: true,
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
    ...render(<PersonaArtifactPanel conversation={value} />, {
      wrapper: Wrapper,
    }),
  };
}

function mockPersonaQueries(persona = rawDraft) {
  vi.mocked(invoke).mockImplementation(async (command) => {
    if (command === "get_persona") return persona;
    if (command === "get_artifact") return rawPersonaArtifact;
    if (command === "get_artifact_version_history") return rawHistory;
    if (command === "get_artifact_at_version") {
      return {
        id: "artifact-1",
        name: "Support Voice",
        artifact_type: "persona",
        content_type: "inline",
        content:
          "---\nname: support-voice\nkind: persona\ndescription: Original support voice.\n---\n\n# Support Voice\n\n## Voice\n\nOriginal agent draft.",
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
    useEnvironmentStore.setState({
      activeEnvironmentId: "local",
      environments: [{ id: "local", name: "This Mac", kind: "local" }],
      effectiveScopes: {},
      connectionPresentations: {},
    });
  });

  it("soft-disables persona approval remotely without agent control and exposes why", async () => {
    mockPersonaQueries();
    useEnvironmentStore.setState({
      activeEnvironmentId: "remote",
      environments: [
        { id: "local", name: "This Mac", kind: "local" },
        { id: "remote", name: "Studio Mac", kind: "remote" },
      ],
      effectiveScopes: { remote: ["ui:read", "ui:operate"] },
      connectionPresentations: {
        remote: {
          presentation: "connected",
          blockedFailure: null,
          blockedMessage: null,
        },
      },
    });
    renderPanel(conversation({ builderDraftId: "draft-1" }));
    const button = await screen.findByRole("button", {
      name: "Approve Persona",
    });

    expect(button).toHaveAttribute("aria-disabled", "true");
    expect(button).not.toBeDisabled();
    fireEvent.click(button);
    expect(invoke).not.toHaveBeenCalledWith(
      "approve_persona",
      expect.anything(),
    );
    button.focus();
    expect(
      (
        await screen.findAllByText(
          "Agent control is off for this device — enable it on the host.",
        )
      ).length,
    ).toBeGreaterThan(0);
  });

  it("approves a persona when remote agent control is granted", async () => {
    mockPersonaQueries();
    useEnvironmentStore.setState({
      activeEnvironmentId: "remote",
      environments: [
        { id: "local", name: "This Mac", kind: "local" },
        { id: "remote", name: "Studio Mac", kind: "remote" },
      ],
      effectiveScopes: { remote: ["ui:read", "ui:operate", "ui:agent"] },
      connectionPresentations: {
        remote: {
          presentation: "connected",
          blockedFailure: null,
          blockedMessage: null,
        },
      },
    });
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "get_persona") return rawDraft;
      if (command === "get_artifact") return rawPersonaArtifact;
      if (command === "approve_persona") return rawApproved;
      return null;
    });
    renderPanel(conversation({ builderDraftId: "draft-1" }));

    fireEvent.click(
      await screen.findByRole("button", { name: "Approve Persona" }),
    );
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("approve_persona", {
        input: { id: "draft-1" },
      }),
    );
  });

  it("renders empty state without persona actions when no binding exists", () => {
    renderPanel(conversation());

    expect(screen.getByText("Persona not created yet")).toBeInTheDocument();
    expect(
      screen.getByText(
        "The agent will draft the persona here after its first pass",
      ),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /Approve Persona/ }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Open in Settings" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Refine with Agent" }),
    ).not.toBeInTheDocument();
  });

  it("renders the draft through the canonical versioned artifact surface", async () => {
    mockPersonaQueries();
    renderPanel(conversation({ builderDraftId: "draft-1" }));

    expect(await screen.findByText("Empathetic, direct.")).toBeInTheDocument();
    expect(screen.getByTestId("plan-display-chromeless")).toBeInTheDocument();
    expect(
      screen.getByRole("heading", { name: "Support Voice" }),
    ).toBeInTheDocument();
    expect(screen.queryByText("Global")).not.toBeInTheDocument();
    expect(screen.queryByText("Draft")).not.toBeInTheDocument();
    expect(
      screen.queryByTestId("persona-version-history"),
    ).not.toBeInTheDocument();
    expect(screen.getByTestId("persona-frontmatter")).toHaveTextContent(
      "Calm customer support.",
    );
    expect(screen.queryByText(/name: support-voice/)).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Approve Persona" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Open in Settings" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Refine with Agent" }),
    ).not.toBeInTheDocument();
  });

  it("reveals the conversation-owned draft immediately after the first agent save", async () => {
    mockPersonaQueries();
    renderPanel(conversation());

    await waitFor(() => expect(window.__eventBus).toBeDefined());
    act(() => {
      window.__eventBus?.emit("persona:draft_updated", {
        draft_id: "draft-1",
        version: 3,
        content_hash: "hash-3",
        artifact_id: "artifact-1",
        builder_conversation_id: "conversation-1",
      });
    });

    expect(await screen.findByText("Empathetic, direct.")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Approve Persona" }),
    ).toBeInTheDocument();
  });

  it("renders a legacy null-artifact persona through the same document surface", async () => {
    mockPersonaQueries({ ...rawDraft, artifact_id: null, version: 1 });
    renderPanel(conversation({ builderDraftId: "draft-1" }));

    expect(
      await screen.findByTestId("plan-display-chromeless"),
    ).toBeInTheDocument();
    expect(screen.getByText("Empathetic, direct.")).toBeInTheDocument();
    expect(invoke).not.toHaveBeenCalledWith("get_artifact", expect.anything());
    expect(screen.queryByTitle("View version history")).not.toBeInTheDocument();
  });

  it("fails closed when a bound Persona artifact is missing", async () => {
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "get_persona") return rawDraft;
      if (command === "get_artifact") return null;
      return [];
    });
    renderPanel(conversation({ builderDraftId: "draft-1" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Persona artifact unavailable",
    );
    expect(
      screen.queryByRole("button", { name: "Approve Persona" }),
    ).not.toBeInTheDocument();
  });

  it("renders approved and archived states with the correct action availability", async () => {
    mockPersonaQueries(rawApproved);
    const { rerender } = renderPanel(
      conversation({ builderResultPersonaId: "persona-1" }),
    );

    expect(
      await screen.findByRole("button", { name: "Open in Settings" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Refine with Agent" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /Approve Persona/ }),
    ).not.toBeInTheDocument();

    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "get_persona")
        return { ...rawApproved, status: "archived" };
      if (command === "get_artifact_version_history") return rawHistory;
      return null;
    });
    rerender(
      <PersonaArtifactPanel
        conversation={conversation({
          builderResultPersonaId: "archived-persona",
        })}
      />,
    );

    expect(
      await screen.findByRole("button", { name: "Open in Settings" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Refine with Agent" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /Approve Persona/ }),
    ).not.toBeInTheDocument();
  });

  it("uses the canonical artifact version picker and makes history read-only", async () => {
    const user = userEvent.setup();
    mockPersonaQueries();
    renderPanel(conversation({ builderDraftId: "draft-1" }));

    await user.click(await screen.findByTitle("View version history"));
    await user.click(await screen.findByText(/^v1\b/));

    expect(
      await screen.findByText("Original agent draft."),
    ).toBeInTheDocument();
    expect(invoke).toHaveBeenCalledWith("get_artifact_at_version", {
      id: "artifact-1",
      version: 1,
    });
    expect(screen.getByText("Viewing version 1 of 3")).toBeInTheDocument();
    expect(screen.getByTestId("persona-frontmatter")).toHaveTextContent(
      "Original support voice.",
    );
    expect(
      screen.getByRole("button", { name: "Back to latest" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Approve Persona" }),
    ).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Back to latest" }));
    expect(screen.getByTestId("persona-frontmatter")).toHaveTextContent(
      "Calm customer support.",
    );
    expect(
      screen.getByRole("button", { name: "Approve Persona" }),
    ).toBeInTheDocument();
  });

  it("switches an open Agent pane to the artifact tip returned by a manual Persona save", async () => {
    const updatedArtifact = {
      ...rawPersonaArtifact,
      id: "artifact-4",
      content:
        "---\nname: support-voice\nkind: persona\ndescription: Updated support guidance.\n---\n\n# Support Voice\n\nManual v4 content.",
      created_at: "2026-07-17T11:00:00Z",
      created_by: "user",
      version: 4,
    };
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      if (command === "get_persona") return rawApproved;
      if (command === "get_artifact") {
        return (args as { id: string }).id === "artifact-4"
          ? updatedArtifact
          : rawPersonaArtifact;
      }
      if (command === "get_artifact_version_history") return rawHistory;
      return null;
    });
    const { queryClient } = renderPanel(
      conversation({ builderResultPersonaId: "persona-1" }),
    );
    expect(await screen.findByText("Empathetic, direct.")).toBeInTheDocument();

    act(() => {
      queryClient.setQueryData(
        personaKeys.detail("persona-1"),
        transformPersona(
          PersonaResponseSchema.parse({
            ...rawApproved,
            artifact_id: "artifact-4",
            content: updatedArtifact.content,
            description: "Updated support guidance.",
            version: 4,
            content_hash: "hash-4",
            updated_at: "2026-07-17T11:00:00Z",
          }),
        ),
      );
    });

    expect(await screen.findByText("Manual v4 content.")).toBeInTheDocument();
    expect(screen.getByTestId("persona-frontmatter")).toHaveTextContent(
      "Updated support guidance.",
    );
    expect(invoke).toHaveBeenCalledWith("get_artifact", { id: "artifact-4" });
    expect(screen.getByTitle("View version history")).toHaveTextContent("v4");
  });

  it("approves through the existing command and transitions live to approved", async () => {
    const user = userEvent.setup();
    mockPersonaQueries();
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "get_persona") return rawDraft;
      if (command === "get_artifact") return rawPersonaArtifact;
      if (command === "get_artifact_version_history") return rawHistory;
      if (command === "approve_persona") return rawApproved;
      return null;
    });
    const { rerender } = renderPanel(
      conversation({ builderDraftId: "draft-1" }),
    );

    await user.click(
      await screen.findByRole("button", { name: "Approve Persona" }),
    );

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("approve_persona", {
        input: { id: "draft-1" },
      });
    });
    expect(
      screen.queryByRole("button", { name: "Approve Persona" }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Open in Settings" }),
    ).toBeInTheDocument();

    rerender(<PersonaArtifactPanel conversation={conversation()} />);
    expect(
      screen.getByRole("button", { name: "Open in Settings" }),
    ).toBeInTheDocument();
  });

  it("offers approve-as-new only for seeded drafts", async () => {
    const user = userEvent.setup();
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "get_persona") {
        return { ...rawDraft, source_persona_id: "persona-source" };
      }
      if (command === "get_artifact") return rawPersonaArtifact;
      if (command === "get_artifact_version_history") return rawHistory;
      if (command === "approve_persona_as_new") return rawApproved;
      return null;
    });
    renderPanel(conversation({ builderDraftId: "draft-1" }));

    expect(
      await screen.findByRole("button", { name: "Approve Persona" }),
    ).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Approve as new" }));
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("approve_persona_as_new", {
        input: { id: "draft-1" },
      });
    });
  });

  it("uses the current Persona Builder conversation as the refinement surface", async () => {
    mockPersonaQueries(rawApproved);
    renderPanel(conversation({ builderResultPersonaId: "persona-1" }));

    expect(
      await screen.findByRole("button", { name: "Open in Settings" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Refine with Agent" }),
    ).not.toBeInTheDocument();
  });

  it("reuses Settings and refine deep links outside a Persona Builder conversation", async () => {
    const user = userEvent.setup();
    mockPersonaQueries(rawApproved);
    renderPanel(
      conversation({
        agentMode: "edit",
        builderResultPersonaId: "persona-1",
      }),
    );

    await user.click(
      await screen.findByRole("button", { name: "Open in Settings" }),
    );
    expect(useUiStore.getState()).toMatchObject({
      activeModal: "settings",
      modalContext: {
        section: "personas",
        personaId: "persona-1",
        conversationId: "conversation-1",
      },
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
      conversation({ agentMode: "edit", builderResultPersonaId: "persona-1" }),
      createQueryClient(),
      false,
    );

    const refine = await screen.findByRole("button", {
      name: "Refine with Agent",
    });
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
      conversation({ agentMode: "edit", builderResultPersonaId: "persona-1" }),
      createQueryClient(),
      false,
    );

    const refine = await screen.findByRole("button", {
      name: "Refine with Agent",
    });
    expect(refine).toBeEnabled();
    await user.click(refine);
    expect(
      useAgentSessionStore.getState().startConversationDraft,
    ).toMatchObject({
      projectId: "project-1",
      sourcePersonaId: "persona-1",
    });
  });

  it("toggles a read-only diff against the previous artifact version", async () => {
    const user = userEvent.setup();
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      const id = (args as { id?: string } | undefined)?.id;
      if (command === "get_persona") return rawDraft;
      if (command === "get_artifact") {
        if (id === "artifact-2") {
          return {
            ...rawPersonaArtifact,
            id: "artifact-2",
            version: 2,
            content:
              "---\nname: support-voice\nkind: persona\ndescription: Calm customer support.\n---\n\n# Support Voice\n\n## Voice\n\nEarlier draft body.",
          };
        }
        return rawPersonaArtifact;
      }
      if (command === "get_artifact_version_history") return rawHistory;
      return null;
    });
    renderPanel(conversation({ builderDraftId: "draft-1" }));

    await screen.findByText("Empathetic, direct.");
    await user.click(screen.getByTestId("persona-show-changes-toggle"));

    expect(await screen.findByTestId("persona-diff")).toBeInTheDocument();
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("get_artifact_version_history", {
        id: "artifact-1",
      }),
    );
    // Read-only proof: no persona/artifact mutation commands fired.
    for (const command of [
      "approve_persona",
      "approve_persona_as_new",
      "update_persona_draft",
      "reseed_persona_draft",
    ]) {
      expect(invoke).not.toHaveBeenCalledWith(command, expect.anything());
    }

    await user.click(screen.getByTestId("persona-show-changes-toggle"));
    expect(await screen.findByText("Empathetic, direct.")).toBeInTheDocument();
  });

  it("hides the show-changes toggle for first versions", async () => {
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "get_persona") return rawDraft;
      if (command === "get_artifact")
        return { ...rawPersonaArtifact, version: 1 };
      return null;
    });
    renderPanel(conversation({ builderDraftId: "draft-1" }));

    await screen.findByText("Empathetic, direct.");
    expect(
      screen.queryByTestId("persona-show-changes-toggle"),
    ).not.toBeInTheDocument();
  });

  it("shows the stale-source banner for seeded drafts with a hash mismatch and rebases", async () => {
    const user = userEvent.setup();
    const seededDraft = {
      ...rawDraft,
      source_persona_id: "persona-source",
      source_content_hash: "seed-hash",
    };
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      const input = (args as { input?: { id?: string } } | undefined)?.input;
      if (command === "get_persona") {
        if (input?.id === "persona-source") {
          return {
            ...rawApproved,
            id: "persona-source",
            content_hash: "moved-on-hash",
          };
        }
        return seededDraft;
      }
      if (command === "get_artifact") return rawPersonaArtifact;
      if (command === "reseed_persona_draft") {
        return { ...seededDraft, source_content_hash: "moved-on-hash" };
      }
      return null;
    });
    renderPanel(conversation({ builderDraftId: "draft-1" }));

    expect(
      await screen.findByTestId("persona-stale-source-banner"),
    ).toHaveTextContent("Source persona changed since this draft was seeded.");
    await user.click(
      screen.getByRole("button", { name: "Rebase draft on current source" }),
    );
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("reseed_persona_draft", {
        input: { id: "draft-1" },
      }),
    );
  });

  it("keeps the stale banner hidden while the seed hash still matches", async () => {
    const seededDraft = {
      ...rawDraft,
      source_persona_id: "persona-source",
      source_content_hash: "seed-hash",
    };
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      const input = (args as { input?: { id?: string } } | undefined)?.input;
      if (command === "get_persona") {
        if (input?.id === "persona-source") {
          return {
            ...rawApproved,
            id: "persona-source",
            content_hash: "seed-hash",
          };
        }
        return seededDraft;
      }
      if (command === "get_artifact") return rawPersonaArtifact;
      return null;
    });
    renderPanel(conversation({ builderDraftId: "draft-1" }));

    await screen.findByText("Empathetic, direct.");
    expect(
      screen.queryByTestId("persona-stale-source-banner"),
    ).not.toBeInTheDocument();
  });

  it("reveals the rebase action when approval reports the seed conflict", async () => {
    const user = userEvent.setup();
    const seededDraft = {
      ...rawDraft,
      source_persona_id: "persona-source",
      source_content_hash: "seed-hash",
    };
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      const input = (args as { input?: { id?: string } } | undefined)?.input;
      if (command === "get_persona") {
        if (input?.id === "persona-source") {
          return {
            ...rawApproved,
            id: "persona-source",
            content_hash: "seed-hash",
          };
        }
        return seededDraft;
      }
      if (command === "get_artifact") return rawPersonaArtifact;
      if (command === "approve_persona") {
        throw new Error(
          "SourceChangedSinceSeed: source persona persona-source changed after draft draft-1 was seeded",
        );
      }
      return null;
    });
    renderPanel(conversation({ builderDraftId: "draft-1" }));

    await screen.findByText("Empathetic, direct.");
    expect(
      screen.queryByTestId("persona-stale-source-banner"),
    ).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Approve Persona" }));
    expect(
      await screen.findByTestId("persona-stale-source-banner"),
    ).toBeInTheDocument();
    const alerts = await screen.findAllByRole("alert");
    expect(
      alerts.some((alert) =>
        alert.textContent?.includes("SourceChangedSinceSeed:"),
      ),
    ).toBe(true);
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

    expect(
      screen.getByRole("status", { name: "Loading persona..." }),
    ).toBeInTheDocument();
    expect(screen.queryByText("Empathetic, direct.")).not.toBeInTheDocument();
    expect(resolvePersona).toBeTypeOf("function");
  });
});
