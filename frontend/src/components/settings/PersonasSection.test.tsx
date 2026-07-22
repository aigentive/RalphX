import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { TooltipProvider } from "@/components/ui/tooltip";
import { afterEach, describe, expect, it, vi } from "vitest";

import { splitPersonaBody } from "@/lib/personaContent";
import { useAgentSessionStore } from "@/stores/agentSessionStore";
import { useChatStore } from "@/stores/chatStore";
import { useProjectStore } from "@/stores/projectStore";
import { useUiStore } from "@/stores/uiStore";
import type { Project } from "@/types/project";

import { PersonasManagementSection } from "./PersonasManagementSection";

const toastError = vi.hoisted(() => vi.fn());

vi.mock("sonner", () => ({
  toast: { error: toastError },
}));

type RawPersona = {
  id: string;
  slug: string;
  name: string;
  description: string;
  content: string;
  status: "draft" | "active" | "archived";
  version: number;
  project_id: string | null;
  content_hash: string;
  source_session_id: string | null;
  created_at: string;
  updated_at: string;
};

const activePersona: RawPersona = {
  id: "persona-active",
  slug: "reviewer-voice",
  name: "Reviewer Voice",
  description: "A careful reviewer.",
  content: "---\nname: reviewer-voice\n---\nReview carefully.",
  status: "active",
  version: 3,
  project_id: null,
  content_hash: "active-hash",
  source_session_id: null,
  created_at: "2026-07-10T10:00:00Z",
  updated_at: "2026-07-12T08:00:00Z",
};

const draftPersona: RawPersona = {
  ...activePersona,
  id: "persona-draft",
  slug: "terse-arch",
  name: "Terse Architect",
  status: "draft",
  version: 1,
  content_hash: "draft-hash",
};

const archivedPersona: RawPersona = {
  ...activePersona,
  id: "persona-archived",
  slug: "old-voice",
  name: "Old Voice",
  status: "archived",
};

const ralphxProject: Project = {
  id: "project-ralphx",
  name: "RalphX",
  workingDirectory: "/projects/ralphx",
  gitMode: "worktree",
  baseBranch: "main",
  worktreeParentDirectory: null,
  useFeatureBranches: true,
  mergeValidationMode: "block",
  detectedAnalysis: null,
  customAnalysis: null,
  analyzedAt: null,
  githubPrEnabled: false,
  createdAt: "2026-07-01T00:00:00Z",
  updatedAt: "2026-07-01T00:00:00Z",
};

const atlasProject: Project = {
  ...ralphxProject,
  id: "project-atlas",
  name: "Atlas",
  workingDirectory: "/projects/atlas",
};

function createQueryClient() {
  return new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: 0 },
      mutations: { retry: false },
    },
  });
}

function renderSection(standaloneConversations = true) {
  useProjectStore.getState().setProjects([ralphxProject, atlasProject]);
  return render(
    <QueryClientProvider client={createQueryClient()}>
      <TooltipProvider delayDuration={0}>
        <PersonasManagementSection
          standaloneConversations={standaloneConversations}
        />
      </TooltipProvider>
    </QueryClientProvider>,
  );
}

function mockPersonaCommands(personas: RawPersona[]) {
  const store = personas.map((persona) => ({ ...persona }));
  vi.mocked(invoke).mockImplementation(async (command, args) => {
    const input = (args as { input?: Record<string, unknown> } | undefined)?.input;
    if (command === "list_personas") return store;
    if (command === "create_persona_draft") {
      const created: RawPersona = {
        ...draftPersona,
        id: "persona-new",
        slug: typeof input?.slug === "string" ? input.slug : "new-persona",
        name: "New Persona",
        project_id: typeof input?.projectId === "string" ? input.projectId : null,
        content: typeof input?.content === "string" ? input.content : "",
      };
      store.push(created);
      return created;
    }
    if (command === "update_persona") {
      return { ...activePersona, content: input?.content ?? activePersona.content };
    }
    if (command === "update_persona_draft") {
      const persona = store.find((item) => item.id === input?.id);
      if (!persona) throw new Error("persona missing");
      persona.content = typeof input?.content === "string" ? input.content : persona.content;
      persona.content_hash = "updated-draft-hash";
      return persona;
    }
    if (command === "get_persona") {
      const persona = store.find((item) => item.id === input?.id);
      if (!persona) throw new Error("persona missing");
      return persona;
    }
    if (command === "get_agent_conversation_summary") {
      return {
        id: "builder-conversation",
        context_type: "project",
        context_id: "project-ralphx",
        claude_session_id: null,
        title: "Persona builder",
        message_count: 1,
        last_message_at: null,
        created_at: "2026-07-10T10:00:00Z",
        updated_at: "2026-07-10T10:00:00Z",
      };
    }
    if (command === "approve_persona") {
      const persona = store.find((item) => item.id === input?.id);
      if (!persona) throw new Error("persona missing");
      persona.status = "active";
      return persona;
    }
    if (command === "archive_persona") {
      const persona = store.find((item) => item.id === input?.id);
      if (!persona) throw new Error("persona missing");
      persona.status = "archived";
      return persona;
    }
    if (command === "delete_persona_draft") {
      const index = store.findIndex((item) => item.id === input?.id);
      if (index >= 0) store.splice(index, 1);
      return undefined;
    }
    if (command === "unarchive_persona") {
      const persona = store.find((item) => item.id === input?.id);
      if (!persona) throw new Error("persona missing");
      const collision = store.find(
        (item) =>
          item.id !== persona.id &&
          item.slug === persona.slug &&
          item.status === "active" &&
          item.project_id === persona.project_id,
      );
      if (collision) {
        throw new Error(
          `Cannot restore persona: active persona \`${collision.name}\` already uses slug \`${persona.slug}\` in this scope`,
        );
      }
      persona.status = "active";
      return persona;
    }
    if (command === "list_persona_usage") {
      return store.map((persona) => ({
        personaId: persona.id,
        boundConversationCount: persona.id === "persona-active" ? 2 : 0,
        lastRunAt: persona.id === "persona-active" ? "2026-07-21T09:00:00Z" : null,
      }));
    }
    throw new Error(`Unexpected command: ${command}`);
  });
}

describe("PersonasManagementSection", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    useUiStore.setState({
      activeModal: null,
      modalContext: undefined,
      currentView: "agents",
    });
    useAgentSessionStore.setState({
      focusedProjectId: null,
      selectedProjectId: null,
      selectedConversationId: null,
      startConversationDraft: null,
    });
    useChatStore.setState({ activeConversationIds: {} });
  });

  it("filters archived personas from the list and renders the v1 limits copy", async () => {
    mockPersonaCommands([activePersona, draftPersona, archivedPersona]);
    renderSection();

    expect(await screen.findByText("Reviewer Voice")).toBeInTheDocument();
    expect(screen.getByText("Terse Architect")).toBeInTheDocument();
    expect(screen.queryByText("Old Voice")).not.toBeInTheDocument();
    expect(screen.getByText(/applies to this conversation only/i)).toBeInTheDocument();
    expect(screen.getByText(/delegated, subagent, or pipeline work/i)).toBeInTheDocument();
  });

  it("filters all, global, and project scopes in both directions and renders scope badges", async () => {
    const user = userEvent.setup();
    const globalPersona = activePersona;
    const projectPersona: RawPersona = {
      ...draftPersona,
      project_id: "project-ralphx",
    };
    const deletedProjectPersona: RawPersona = {
      ...activePersona,
      id: "persona-deleted-project",
      name: "Orphan Voice",
      project_id: "project-deleted",
    };
    mockPersonaCommands([globalPersona, projectPersona, deletedProjectPersona]);
    renderSection();

    expect(await screen.findByText("Reviewer Voice")).toBeInTheDocument();
    expect(screen.getByText("Terse Architect")).toBeInTheDocument();
    expect(screen.getByText("Orphan Voice")).toBeInTheDocument();
    expect(screen.getByTestId("persona-scope-persona-active")).toHaveTextContent("Global");
    expect(screen.getByTestId("persona-scope-persona-draft")).toHaveTextContent("RalphX");
    expect(screen.getByTestId("persona-scope-persona-deleted-project")).toHaveTextContent(
      "project-deleted",
    );

    await user.selectOptions(screen.getByLabelText("Scope filter"), "global");
    expect(screen.getByText("Reviewer Voice")).toBeInTheDocument();
    expect(screen.queryByText("Terse Architect")).not.toBeInTheDocument();
    expect(screen.queryByText("Orphan Voice")).not.toBeInTheDocument();

    await user.selectOptions(screen.getByLabelText("Scope filter"), "project-ralphx");
    expect(screen.getByText("Terse Architect")).toBeInTheDocument();
    expect(screen.queryByText("Reviewer Voice")).not.toBeInTheDocument();
    expect(screen.queryByText("Orphan Voice")).not.toBeInTheDocument();
  });

  it("keeps the filtered empty treatment and names the active project", async () => {
    const user = userEvent.setup();
    mockPersonaCommands([activePersona]);
    renderSection();

    await screen.findByText("Reviewer Voice");
    await user.selectOptions(screen.getByLabelText("Scope filter"), "project-atlas");

    expect(screen.getByText("No personas for Atlas.")).toBeInTheDocument();
    expect(screen.queryByText("Reviewer Voice")).not.toBeInTheDocument();
  });

  it("shows the empty state when every persona is archived", async () => {
    mockPersonaCommands([archivedPersona]);
    renderSection();

    expect(await screen.findByText("No personas yet. Create a draft to get started.")).toBeInTheDocument();
    expect(screen.queryByText("Old Voice")).not.toBeInTheDocument();
  });

  it("shows the builder entry by default when the feature is enabled", async () => {
    mockPersonaCommands([activePersona]);
    renderSection();

    await screen.findByText("Reviewer Voice");
    expect(screen.getByRole("button", { name: "Build with Agent" })).toBeInTheDocument();
  });

  it("defaults builder scope to Global and gates project starts until a project is chosen", async () => {
    const user = userEvent.setup();
    mockPersonaCommands([activePersona]);
    useProjectStore.getState().setProjects([ralphxProject, atlasProject]);
    useProjectStore.getState().selectProject(null);
    renderSection();

    await user.click(await screen.findByRole("button", { name: "Build with Agent" }));
    expect(screen.getByRole("dialog", { name: "Build persona with agent" })).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: /Global/ })).toBeChecked();
    expect(screen.getByRole("button", { name: "Start build" })).toBeEnabled();

    await user.click(screen.getByRole("radio", { name: /Project:/ }));
    expect(screen.getByRole("button", { name: "Start build" })).toBeDisabled();
    await user.selectOptions(screen.getByLabelText("Build persona project"), "project-atlas");
    expect(screen.getByRole("button", { name: "Start build" })).toBeEnabled();
  });

  it("hides Global when standalone is off and dispatches the exact locked draft before navigation", async () => {
    const user = userEvent.setup();
    const calls: string[] = [];
    mockPersonaCommands([activePersona]);
    useProjectStore.getState().setProjects([ralphxProject, atlasProject]);
    useProjectStore.getState().selectProject("project-atlas");
    useChatStore
      .getState()
      .setActiveConversation("project:project-atlas", "previous-conversation");
    useUiStore.getState().openModal("settings", { section: "personas" });
    const sessionState = useAgentSessionStore.getState();
    const uiState = useUiStore.getState();
    const originalSessionActions = {
      setStartConversationDraft: sessionState.setStartConversationDraft,
      setFocusedProject: sessionState.setFocusedProject,
      clearSelection: sessionState.clearSelection,
    };
    const originalUiActions = {
      closeModal: uiState.closeModal,
      setCurrentView: uiState.setCurrentView,
    };
    vi.spyOn(sessionState, "setStartConversationDraft").mockImplementation((draft) => {
      calls.push("draft");
      useAgentSessionStore.setState({ startConversationDraft: draft });
    });
    vi.spyOn(uiState, "closeModal").mockImplementation(() => {
      calls.push("close");
      useUiStore.setState({ modal: null });
    });
    vi.spyOn(sessionState, "setFocusedProject").mockImplementation((projectId) => {
      calls.push("focus");
      useAgentSessionStore.setState({ focusedProjectId: projectId });
    });
    vi.spyOn(sessionState, "clearSelection").mockImplementation(() => {
      calls.push("clear");
      useAgentSessionStore.setState({ selectedProjectId: null, selectedConversationId: null });
    });
    vi.spyOn(uiState, "setCurrentView").mockImplementation((view) => {
      calls.push("view");
      useUiStore.setState({ currentView: view });
    });
    renderSection(false);

    await user.click(await screen.findByRole("button", { name: "Build with Agent" }));
    expect(screen.queryByRole("radio", { name: /Global/ })).not.toBeInTheDocument();
    expect(screen.getByLabelText("Build persona project")).toHaveValue("project-atlas");
    await user.click(screen.getByRole("button", { name: "Start build" }));

    expect(useAgentSessionStore.getState().startConversationDraft).toEqual({
      projectId: "project-atlas",
      projectLocked: true,
      mode: "persona_builder",
    });
    expect(
      useChatStore.getState().activeConversationIds["project:project-atlas"],
    ).toBeNull();
    expect(calls).toEqual(["draft", "close", "focus", "clear", "view"]);
    useAgentSessionStore.setState(originalSessionActions);
    useUiStore.setState(originalUiActions);
  });

  it.each([
    ["global", activePersona, null],
    ["project", { ...activePersona, id: "project-persona", project_id: "project-ralphx" }, "project-ralphx"],
  ])("refines a %s persona without opening the chooser", async (_scope, persona, projectId) => {
    const user = userEvent.setup();
    mockPersonaCommands([persona]);
    renderSection();

    const refine = await screen.findByRole("button", {
      name: `Refine ${persona.name} with Agent`,
    });
    expect(refine).toBeEnabled();
    await user.click(refine);

    expect(screen.queryByRole("dialog", { name: "Build persona with agent" })).not.toBeInTheDocument();
    expect(useAgentSessionStore.getState().startConversationDraft).toEqual({
      projectId,
      projectLocked: true,
      mode: "persona_builder",
      sourcePersonaId: persona.id,
      sourcePersonaName: persona.name,
    });
  });

  it("blocks global refinement with an explanatory tooltip when standalone conversations are off", async () => {
    const user = userEvent.setup();
    const projectPersona: RawPersona = {
      ...activePersona,
      id: "project-persona",
      name: "Project Reviewer",
      project_id: "project-ralphx",
    };
    mockPersonaCommands([activePersona, projectPersona]);
    renderSection(false);

    const globalRefine = await screen.findByRole("button", {
      name: "Refine Reviewer Voice with Agent",
    });
    expect(globalRefine).toBeDisabled();
    await user.hover(globalRefine.parentElement!);
    expect(await screen.findByRole("tooltip")).toHaveTextContent(
      "Global persona refinement requires standalone conversations",
    );
    fireEvent.click(globalRefine);
    expect(useAgentSessionStore.getState().startConversationDraft).toBeNull();

    const projectRefine = screen.getByRole("button", {
      name: "Refine Project Reviewer with Agent",
    });
    expect(projectRefine).toBeEnabled();
    await user.click(projectRefine);
    expect(useAgentSessionStore.getState().startConversationDraft).toMatchObject({
      projectId: "project-ralphx",
      sourcePersonaId: "project-persona",
    });
  });

  it("creates a persona from structured fields and auto-fills the slug from its name", async () => {
    const user = userEvent.setup();
    const personas = [activePersona];
    mockPersonaCommands(personas);
    renderSection();

    await screen.findByText("Reviewer Voice");
    await user.click(screen.getByRole("button", { name: "New persona" }));
    expect(screen.getByLabelText("Scope")).toHaveValue("global");
    await user.type(screen.getByLabelText("Name"), "New Persona");
    expect(screen.getByLabelText("Slug")).toHaveValue("new-persona");
    await user.type(screen.getByLabelText("Description"), "A crisp design voice");
    await user.type(screen.getByLabelText("Instructions"), "Prefer concrete language.");
    await user.click(screen.getByRole("button", { name: /^Save/ }));

    await screen.findByText("New Persona");
    expect(invoke).toHaveBeenCalledWith("create_persona_draft", {
      input: {
        projectId: null,
        slug: "new-persona",
        description: "A crisp design voice",
        body: "Prefer concrete language.",
      },
    });

    await user.click(screen.getByRole("button", { name: "Activate New Persona" }));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("approve_persona", {
        input: { id: "persona-new" },
      }),
    );
  });

  it("stamps the selected project scope when creating a persona", async () => {
    const user = userEvent.setup();
    mockPersonaCommands([activePersona]);
    renderSection();

    await screen.findByText("Reviewer Voice");
    await user.click(screen.getByRole("button", { name: "New persona" }));
    await user.selectOptions(screen.getByLabelText("Scope"), "project-ralphx");
    await user.type(screen.getByLabelText("Name"), "Project Voice");
    await user.type(screen.getByLabelText("Description"), "Scoped to RalphX");
    await user.type(screen.getByLabelText("Instructions"), "Use project context.");
    await user.click(screen.getByRole("button", { name: /^Save/ }));

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("create_persona_draft", {
        input: {
          projectId: "project-ralphx",
          slug: "project-voice",
          description: "Scoped to RalphX",
          body: "Use project context.",
        },
      }),
    );
  });

  it("stops synchronizing the slug after it is manually edited", async () => {
    const user = userEvent.setup();
    mockPersonaCommands([activePersona]);
    renderSection();

    await screen.findByText("Reviewer Voice");
    await user.click(screen.getByRole("button", { name: "New persona" }));
    await user.type(screen.getByLabelText("Name"), "Design Voice");
    await user.clear(screen.getByLabelText("Slug"));
    await user.type(screen.getByLabelText("Slug"), "custom-slug");
    await user.type(screen.getByLabelText("Name"), " Updated");

    expect(screen.getByLabelText("Slug")).toHaveValue("custom-slug");
  });

  it("sends a pasted persona document as explicit content", async () => {
    const user = userEvent.setup();
    mockPersonaCommands([activePersona]);
    renderSection();

    await screen.findByText("Reviewer Voice");
    await user.click(screen.getByRole("button", { name: "New persona" }));
    await user.type(screen.getByLabelText("Name"), "Pasted Persona");
    await user.type(
      screen.getByLabelText("Instructions"),
      "---\nname: pasted-persona\nkind: persona\ndescription: Pasted\n---\nUse the pasted document.",
    );
    await user.click(screen.getByRole("button", { name: /^Save/ }));

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("create_persona_draft", {
        input: {
          projectId: null,
          slug: "pasted-persona",
          content:
            "---\nname: pasted-persona\nkind: persona\ndescription: Pasted\n---\nUse the pasted document.",
        },
      }),
    );
  });

  it("disables save until the required structured persona fields are present", async () => {
    const user = userEvent.setup();
    mockPersonaCommands([activePersona]);
    renderSection();

    await screen.findByText("Reviewer Voice");
    await user.click(screen.getByRole("button", { name: "New persona" }));
    expect(screen.getByRole("button", { name: /^Save/ })).toBeDisabled();

    await user.type(screen.getByLabelText("Name"), "Ready Persona");
    await user.type(screen.getByLabelText("Description"), "Ready to write");
    expect(screen.getByRole("button", { name: /^Save/ })).toBeDisabled();

    await user.type(screen.getByLabelText("Instructions"), "Write with care.");
    expect(screen.getByRole("button", { name: /^Save/ })).toBeEnabled();
  });

  it("shows a save validation error inline and keeps the active edit form populated", async () => {
    const user = userEvent.setup();
    mockPersonaCommands([activePersona]);
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      if (command === "list_personas") return [activePersona];
      if (command === "update_persona") {
        throw new Error("body contains blocked structural tag");
      }
      throw new Error(`Unexpected command: ${command} ${String(args)}`);
    });
    renderSection();

    await screen.findByText("Reviewer Voice");
    await user.click(screen.getByRole("button", { name: "Edit Reviewer Voice" }));
    expect(screen.getByText("Scope")).toBeInTheDocument();
    expect(screen.getByTestId("persona-editor-scope")).toHaveTextContent("Global");
    const instructions = screen.getByLabelText("Instructions");
    fireEvent.change(instructions, { target: { value: "<blocked-tag>" } });
    await user.click(screen.getByRole("button", { name: /^Save/ }));

    expect(await screen.findByText(/Save failed: body contains blocked structural tag/)).toBeInTheDocument();
    expect(screen.getByLabelText("Instructions")).toHaveValue("<blocked-tag>");
    expect(toastError).not.toHaveBeenCalled();
  });

  it("opens a deep-linked persona directly in the existing editor and keeps Back on the list", async () => {
    const user = userEvent.setup();
    mockPersonaCommands([activePersona]);
    useUiStore.getState().openModal("settings", {
      section: "personas",
      personaId: "persona-active",
      conversationId: "originating-conversation",
    });

    renderSection();

    expect(await screen.findByText("Edit persona: Reviewer Voice")).toBeInTheDocument();
    expect(screen.getByLabelText("Description")).toHaveValue("A careful reviewer.");
    expect(screen.getByLabelText("Instructions")).toHaveValue("Review carefully.");
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("get_agent_conversation_summary", {
        conversationId: "originating-conversation",
      }),
    );
    const openInAgent = screen.getByRole("button", { name: "Open in Agent" });
    await waitFor(() => expect(openInAgent).toBeEnabled());
    await user.click(openInAgent);
    expect(useAgentSessionStore.getState().selectedConversationId).toBe(
      "originating-conversation",
    );
    expect(useUiStore.getState().activeModal).toBeNull();

    await user.click(screen.getByRole("button", { name: "Back to personas" }));

    expect(await screen.findByText("Reviewer Voice")).toBeInTheDocument();
    expect(screen.queryByText("Edit persona: Reviewer Voice")).not.toBeInTheDocument();
  });

  it.each([
    ["missing", "persona-missing"],
    ["archived", "persona-archived"],
  ])("leaves the personas list visible for a %s deep-link target", async (_case, personaId) => {
    mockPersonaCommands([activePersona, archivedPersona]);
    useUiStore.getState().openModal("settings", {
      section: "personas",
      personaId,
    });

    renderSection();

    expect(await screen.findByText("Reviewer Voice")).toBeInTheDocument();
    expect(screen.queryByLabelText("Persona editor")).not.toBeInTheDocument();
    expect(screen.queryByText("Old Voice")).not.toBeInTheDocument();
  });

  it("updates an active persona with the hook input contract", async () => {
    const user = userEvent.setup();
    mockPersonaCommands([activePersona]);
    renderSection();

    await screen.findByText("Reviewer Voice");
    await user.click(screen.getByRole("button", { name: "Edit Reviewer Voice" }));
    expect(screen.getByLabelText("Description")).toHaveValue("A careful reviewer.");
    expect(screen.getByLabelText("Instructions")).toHaveValue("Review carefully.");
    fireEvent.change(screen.getByLabelText("Instructions"), {
      target: { value: "Updated body" },
    });
    await user.click(screen.getByRole("button", { name: /^Save/ }));

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("update_persona", {
        input: {
          id: "persona-active",
          description: "A careful reviewer.",
          body: "Updated body",
        },
      }),
    );
  });

  it("cancels an active persona edit without saving the modified draft", async () => {
    const user = userEvent.setup();
    mockPersonaCommands([activePersona]);
    renderSection();

    await screen.findByText("Reviewer Voice");
    await user.click(screen.getByRole("button", { name: "Edit Reviewer Voice" }));
    await user.clear(screen.getByLabelText("Instructions"));
    await user.type(screen.getByLabelText("Instructions"), "Unsaved revision");
    await user.click(screen.getByRole("button", { name: "Cancel" }));

    expect(screen.getByText("Reviewer Voice")).toBeInTheDocument();
    expect(vi.mocked(invoke)).not.toHaveBeenCalledWith("update_persona", expect.anything());
  });

  it("edits and saves a draft with the current content hash CAS token", async () => {
    const user = userEvent.setup();
    mockPersonaCommands([draftPersona]);
    renderSection();

    await screen.findByText("Terse Architect");
    await user.click(screen.getByRole("button", { name: "Edit Terse Architect" }));

    expect(screen.getByLabelText("Description")).toBeEnabled();
    expect(screen.getByLabelText("Instructions")).toBeEnabled();
    expect(screen.getByRole("button", { name: /^Save/ })).toBeVisible();

    fireEvent.change(screen.getByLabelText("Instructions"), {
      target: { value: "Updated draft body" },
    });
    await user.click(screen.getByRole("button", { name: /^Save/ }));

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("update_persona_draft", {
        input: {
          id: "persona-draft",
          content:
            "---\nname: terse-arch\nkind: persona\ndescription: \"A careful reviewer.\"\n---\n\nUpdated draft body\n",
          expectedContentHash: "draft-hash",
        },
      }),
    );
    expect(screen.getByText("Terse Architect")).toBeInTheDocument();
    expect(screen.queryByLabelText("Instructions")).not.toBeInTheDocument();
  });

  it("reloads a conflicting draft and discards stale local edits", async () => {
    const user = userEvent.setup();
    const freshDraft: RawPersona = {
      ...draftPersona,
      description: "Fresh description.",
      content:
        "---\nname: terse-arch\nkind: persona\ndescription: Fresh description.\n---\n\nFresh builder body.\n",
      content_hash: "fresh-hash",
      version: 2,
    };
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "list_personas") return [draftPersona];
      if (command === "update_persona_draft") {
        throw new Error(
          "PERSONA_DRAFT_CONFLICT: expected content hash `draft-hash` but current hash is `fresh-hash`",
        );
      }
      if (command === "get_persona") return freshDraft;
      throw new Error(`Unexpected command: ${command}`);
    });
    renderSection();

    await screen.findByText("Terse Architect");
    await user.click(screen.getByRole("button", { name: "Edit Terse Architect" }));
    await user.clear(screen.getByLabelText("Instructions"));
    await user.type(screen.getByLabelText("Instructions"), "Stale local edit");
    await user.click(screen.getByRole("button", { name: /^Save/ }));

    expect(
      await screen.findByText("This draft changed since you loaded it."),
    ).toBeInTheDocument();
    expect(screen.queryByText(/Save failed:/)).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Reload draft" }));

    await waitFor(() =>
      expect(screen.getByLabelText("Instructions")).toHaveValue("Fresh builder body.\n"),
    );
    expect(screen.getByLabelText("Description")).toHaveValue("Fresh description.");
    expect(screen.getByLabelText("Instructions")).not.toHaveValue("Stale local edit");
    expect(screen.queryByText("This draft changed since you loaded it.")).not.toBeInTheDocument();
  });

  it("keeps non-conflict draft save failures on the generic error path", async () => {
    const user = userEvent.setup();
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "list_personas") return [draftPersona];
      if (command === "update_persona_draft") throw new Error("invalid persona body");
      throw new Error(`Unexpected command: ${command}`);
    });
    renderSection();

    await screen.findByText("Terse Architect");
    await user.click(screen.getByRole("button", { name: "Edit Terse Architect" }));
    await user.click(screen.getByRole("button", { name: /^Save/ }));

    expect(await screen.findByText("Save failed: invalid persona body")).toBeInTheDocument();
    expect(screen.queryByText("This draft changed since you loaded it.")).not.toBeInTheDocument();
  });

  it("reopens a source-linked Persona Builder conversation in its project", async () => {
    const user = userEvent.setup();
    const builderDraft: RawPersona = {
      ...draftPersona,
      source_session_id: "builder-conversation",
    };
    const manualDraft: RawPersona = {
      ...draftPersona,
      id: "manual-draft",
      slug: "manual-draft",
      name: "Manual Draft",
      source_session_id: null,
    };
    mockPersonaCommands([builderDraft, manualDraft]);
    renderSection();

    await screen.findByText("Terse Architect");
    await user.click(screen.getByRole("button", { name: "Edit Terse Architect" }));
    expect(screen.getByText("Persona Builder conversation")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Open in Agent" }),
    ).toBeInTheDocument();
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("get_agent_conversation_summary", {
        conversationId: "builder-conversation",
      }),
    );
    await user.click(screen.getByRole("button", { name: "Open in Agent" }));
    await waitFor(() =>
      expect(useAgentSessionStore.getState().selectedConversationId).toBe(
        "builder-conversation",
      ),
    );

    await user.click(screen.getByRole("button", { name: "Back to personas" }));
    await user.click(screen.getByRole("button", { name: "Edit Manual Draft" }));
    expect(screen.queryByRole("button", { name: "Open in Agent" })).not.toBeInTheDocument();
    expect(screen.queryByText("Persona Builder conversation")).not.toBeInTheDocument();
  });

  it("reopens a standalone Persona Builder conversation without borrowing the active project", async () => {
    const user = userEvent.setup();
    const standalonePersona: RawPersona = {
      ...activePersona,
      source_session_id: "standalone-builder-conversation",
    };
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "list_personas") return [standalonePersona];
      if (command === "get_agent_conversation_summary") {
        return {
          id: "standalone-builder-conversation",
          context_type: "standalone",
          context_id: "standalone-builder-conversation",
          claude_session_id: null,
          title: "Global Persona builder",
          message_count: 1,
          last_message_at: null,
          created_at: "2026-07-10T10:00:00Z",
          updated_at: "2026-07-10T10:00:00Z",
        };
      }
      throw new Error(`Unexpected command: ${command}`);
    });
    useProjectStore.getState().selectProject("project-atlas");
    renderSection();

    await user.click(
      await screen.findByRole("button", { name: "Edit Reviewer Voice" }),
    );
    const openInAgent = screen.getByRole("button", { name: "Open in Agent" });
    await waitFor(() => expect(openInAgent).toBeEnabled());
    await user.click(openInAgent);

    expect(useAgentSessionStore.getState()).toMatchObject({
      selectedProjectId: null,
      selectedConversationId: "standalone-builder-conversation",
    });
  });

  it("gives the Markdown instructions editor meaningful modal height", async () => {
    const user = userEvent.setup();
    mockPersonaCommands([activePersona]);
    renderSection();

    await user.click(
      await screen.findByRole("button", { name: "Edit Reviewer Voice" }),
    );

    expect(screen.getByLabelText("Instructions")).toHaveClass("min-h-[50vh]");
  });

  it("archives active personas and hard-deletes drafts through confirmation", async () => {
    const user = userEvent.setup();
    const personas = [activePersona, draftPersona];
    mockPersonaCommands(personas);
    renderSection();

    await screen.findByText("Reviewer Voice");
    await user.click(screen.getByRole("button", { name: "Archive Reviewer Voice" }));
    expect(await screen.findByText(/archive clears conversation bindings/i)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Archive persona" }));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("archive_persona", {
        input: { id: "persona-active" },
      }),
    );

    await user.click(screen.getByRole("button", { name: "Delete Terse Architect" }));
    await user.click(screen.getByRole("button", { name: "Delete draft" }));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("delete_persona_draft", {
        input: { id: "persona-draft" },
      }),
    );
  });

  it("gives destructive icon-only actions labels and app tooltips", async () => {
    const user = userEvent.setup();
    mockPersonaCommands([activePersona, draftPersona]);
    renderSection();

    await screen.findByText("Reviewer Voice");
    const archive = screen.getByRole("button", { name: "Archive Reviewer Voice" });
    const removeDraft = screen.getByRole("button", { name: "Delete Terse Architect" });
    expect(archive).toHaveAttribute("aria-label", "Archive Reviewer Voice");
    expect(removeDraft).toHaveAttribute("aria-label", "Delete Terse Architect");

    await user.hover(archive);
    expect(await screen.findByRole("tooltip")).toHaveTextContent("Archive Reviewer Voice");
  });

  it("filters personas by search across name, slug, and description", async () => {
    const user = userEvent.setup();
    mockPersonaCommands([activePersona, draftPersona]);
    renderSection();

    await screen.findByText("Reviewer Voice");
    const search = screen.getByLabelText("Search personas");
    await user.type(search, "terse");
    expect(screen.getByText("Terse Architect")).toBeInTheDocument();
    expect(screen.queryByText("Reviewer Voice")).not.toBeInTheDocument();

    await user.clear(search);
    await user.type(search, "careful reviewer");
    expect(screen.getByText("Reviewer Voice")).toBeInTheDocument();

    await user.clear(search);
    await user.type(search, "no-such-persona");
    expect(
      screen.getByText('No personas match "no-such-persona".'),
    ).toBeInTheDocument();
  });

  it("shows archived personas on the Archived tab and restores them", async () => {
    const user = userEvent.setup();
    mockPersonaCommands([activePersona, archivedPersona]);
    renderSection();

    await screen.findByText("Reviewer Voice");
    expect(screen.queryByText("Old Voice")).not.toBeInTheDocument();

    await user.click(screen.getByRole("tab", { name: /Archived/ }));
    expect(screen.getByText("Old Voice")).toBeInTheDocument();
    expect(screen.queryByText("Reviewer Voice")).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Edit Old Voice" }),
    ).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Restore Old Voice" }));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("unarchive_persona", {
        input: { id: "persona-archived" },
      }),
    );
    await user.click(screen.getByRole("tab", { name: /All/ }));
    expect(await screen.findByText("Old Voice")).toBeInTheDocument();
  });

  it("surfaces a restore slug collision as an inline actionable error", async () => {
    const user = userEvent.setup();
    const archivedTwin: RawPersona = {
      ...archivedPersona,
      slug: activePersona.slug,
    };
    mockPersonaCommands([activePersona, archivedTwin]);
    renderSection();

    await screen.findByText("Reviewer Voice");
    await user.click(screen.getByRole("tab", { name: /Archived/ }));
    await user.click(screen.getByRole("button", { name: "Restore Old Voice" }));

    expect(
      await screen.findByText(
        /active persona `Reviewer Voice` already uses slug `reviewer-voice`/,
      ),
    ).toBeInTheDocument();
    expect(screen.getByText("Old Voice")).toBeInTheDocument();
  });

  it("renders derived usage per row and description text", async () => {
    mockPersonaCommands([activePersona, draftPersona]);
    renderSection();

    await screen.findByText("Reviewer Voice");
    expect(
      await screen.findByTestId("persona-usage-persona-active"),
    ).toHaveTextContent(/2 conversations · last run/);
    expect(screen.getByTestId("persona-usage-persona-draft")).toHaveTextContent(
      "never used",
    );
    expect(screen.getAllByText("A careful reviewer.").length).toBeGreaterThan(0);
  });

  it("renders an em-dash instead of zero when the usage query fails", async () => {
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "list_personas") return [activePersona];
      if (command === "list_persona_usage") throw new Error("usage backend down");
      throw new Error(`Unexpected command: ${command}`);
    });
    renderSection();

    await screen.findByText("Reviewer Voice");
    expect(
      await screen.findByTestId("persona-usage-error-persona-active"),
    ).toHaveTextContent("—");
    expect(screen.queryByText(/0 conversations/)).not.toBeInTheDocument();
  });

  it("labels the edit save button with the version it will create", async () => {
    const user = userEvent.setup();
    mockPersonaCommands([activePersona]);
    renderSection();

    await screen.findByText("Reviewer Voice");
    await user.click(screen.getByRole("button", { name: "Edit Reviewer Voice" }));
    expect(
      screen.getByRole("button", { name: "Save (creates v4)" }),
    ).toBeInTheDocument();
  });

  it("previews and diffs instructions without mutating persona state", async () => {
    const user = userEvent.setup();
    mockPersonaCommands([activePersona]);
    renderSection();

    await screen.findByText("Reviewer Voice");
    await user.click(screen.getByRole("button", { name: "Edit Reviewer Voice" }));
    fireEvent.change(screen.getByLabelText("Instructions"), {
      target: { value: "Review **boldly**." },
    });

    await user.click(screen.getByRole("tab", { name: "Preview" }));
    expect(
      screen.getByTestId("persona-instructions-preview"),
    ).toHaveTextContent("Review boldly.");

    await user.click(screen.getByRole("tab", { name: "Diff vs v3" }));
    expect(await screen.findByTestId("persona-diff")).toBeInTheDocument();
    expect(screen.getByText("Review **boldly**.")).toBeInTheDocument();
    expect(screen.getByText("Review carefully.")).toBeInTheDocument();

    await user.click(screen.getByRole("tab", { name: "Write" }));
    expect(screen.getByLabelText("Instructions")).toHaveValue("Review **boldly**.");
    expect(vi.mocked(invoke)).not.toHaveBeenCalledWith(
      "update_persona",
      expect.anything(),
    );
  });
});

describe("splitPersonaBody", () => {
  it("strips YAML frontmatter from a persona document", () => {
    expect(splitPersonaBody("---\nname: design-voice\n---\n\nUse concise prose.")).toBe(
      "Use concise prose.",
    );
  });

  it("returns content without YAML frontmatter unchanged", () => {
    expect(splitPersonaBody("Use concise prose.")).toBe("Use concise prose.");
  });
});
