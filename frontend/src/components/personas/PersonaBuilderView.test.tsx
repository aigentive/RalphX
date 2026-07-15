import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { TooltipProvider } from "@/components/ui/tooltip";
import { EventProvider } from "@/providers/EventProvider";
import { afterEach, describe, expect, it, vi } from "vitest";

import { PersonaBuilderView } from "./PersonaBuilderView";

vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));
vi.mock("@/components/Chat/IntegratedChatPanel", () => ({
  IntegratedChatPanel: ({
    conversationIdOverride,
    hideSessionToolbar,
  }: {
    conversationIdOverride?: string;
    hideSessionToolbar?: boolean;
  }) => (
    <div
      data-testid="integrated-chat-panel"
      data-conversation-id={conversationIdOverride ?? ""}
      data-hide-session-toolbar={String(hideSessionToolbar)}
    />
  ),
}));

const rawPersona = {
  id: "draft-1",
  slug: "reviewer-voice",
  name: "Reviewer Voice",
  description: "A careful reviewer.",
  content: "# Reviewer Voice\n\nBe careful.",
  status: "draft",
  version: 2,
  content_hash: "fresh-hash",
  source_session_id: null,
  created_at: "2026-07-12T10:00:00Z",
  updated_at: "2026-07-12T10:00:00Z",
};

function renderBuilder() {
  return render(
    <QueryClientProvider
      client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}
    >
      <EventProvider>
        <TooltipProvider delayDuration={0}>
          <PersonaBuilderView projectId="project-1" onBack={vi.fn()} />
        </TooltipProvider>
      </EventProvider>
    </QueryClientProvider>,
  );
}

function mockBuilderCommands({ ingestLive = true }: { ingestLive?: boolean } = {}) {
  let currentIngestLive = ingestLive;
  vi.mocked(invoke).mockImplementation(async (command) => {
    if (command === "create_persona_builder_conversation") {
      return { id: "builder-conversation" };
    }
    if (command === "get_persona_builder_ingest_status") {
      return { live: currentIngestLive };
    }
    if (command === "get_persona") return rawPersona;
    if (command === "approve_persona") return { ...rawPersona, status: "active" };
    if (command === "list_personas") return [{ ...rawPersona, status: "active" }];
    if (command === "ingest_persona_context") {
      currentIngestLive = true;
      return {
        copied: [{ path: "guidelines.md" }],
        skipped: [{ path: "ignored.log", reason: "ignored" }],
        rejected: [{ path: "outside", reason: "symlink" }],
      };
    }
    throw new Error(`Unexpected command: ${command}`);
  });
}

async function emitDraftUpdated() {
  await act(async () => {
    window.__eventBus?.emit("persona:draft_updated", {
      draft_id: "draft-1",
      version: 2,
      content_hash: "fresh-hash",
      poisoned_body: "DO NOT RENDER THIS EVENT BODY",
    });
  });
}

function openContextPicker(trigger: HTMLElement) {
  fireEvent.pointerDown(trigger, { button: 0, ctrlKey: false });
}

describe("PersonaBuilderView", () => {
  afterEach(() => vi.useRealTimers());

  it("paints its shell before conversation creation and lazy chat hydration", async () => {
    vi.useFakeTimers();
    mockBuilderCommands();
    renderBuilder();

    expect(screen.getByLabelText("Persona Builder")).toBeInTheDocument();
    expect(screen.getByLabelText("Loading draft preview")).toBeInTheDocument();
    expect(vi.mocked(invoke)).not.toHaveBeenCalledWith(
      "create_persona_builder_conversation",
      expect.anything(),
    );

    await act(async () => {
      await vi.advanceTimersByTimeAsync(50);
    });
    expect(invoke).toHaveBeenCalledWith("create_persona_builder_conversation", {
      input: { projectId: "project-1" },
    });
    // Flush the ingest-status query behind the same fake clock.
    await act(async () => {
      await vi.advanceTimersByTimeAsync(50);
    });
    expect(screen.getByTestId("integrated-chat-panel")).toHaveAttribute(
      "data-conversation-id",
      "builder-conversation",
    );
    expect(screen.getByTestId("integrated-chat-panel")).toHaveAttribute(
      "data-hide-session-toolbar",
      "true",
    );
  });

  it("refetches the draft after a validated event without rendering event body", async () => {
    mockBuilderCommands();
    renderBuilder();

    await waitFor(() => expect(window.__eventBus).toBeDefined());
    await emitDraftUpdated();

    expect(await screen.findByText("Reviewer Voice")).toBeInTheDocument();
    expect(invoke).toHaveBeenCalledWith("get_persona", { input: { id: "draft-1" } });
    expect(screen.queryByText("DO NOT RENDER THIS EVENT BODY")).not.toBeInTheDocument();
  });

  it("renders context manifest warnings without blocking chat", async () => {
    mockBuilderCommands();
    vi.mocked(openDialog).mockResolvedValue("/tmp/context" as never);
    renderBuilder();
    await waitFor(() => expect(window.__eventBus).toBeDefined());
    await emitDraftUpdated();
    await screen.findByText("Reviewer Voice");
    expect(await screen.findByTestId("integrated-chat-panel")).toBeInTheDocument();

    openContextPicker(screen.getByRole("button", { name: "Add context…" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Add folder" }));

    expect(await screen.findByText(/1 copied · 1 skipped/)).toBeInTheDocument();
    expect(screen.getByText(/1 rejected/)).toBeInTheDocument();
    expect(screen.getByText(/ignored.log: ignored/)).toBeInTheDocument();
    expect(screen.getByText(/outside: symlink/)).toBeInTheDocument();
    expect(screen.getByTestId("integrated-chat-panel")).toBeInTheDocument();
  });

  it("gates the builder chat until context has been ingested", async () => {
    mockBuilderCommands({ ingestLive: false });
    vi.mocked(openDialog).mockResolvedValue("/tmp/context" as never);
    renderBuilder();

    expect(await screen.findByText("Add context to start")).toBeInTheDocument();
    expect(screen.queryByTestId("integrated-chat-panel")).not.toBeInTheDocument();

    openContextPicker(screen.getByTestId("persona-builder-empty-add-context"));
    fireEvent.click(screen.getByRole("menuitem", { name: "Add folder" }));

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("ingest_persona_context", {
        input: {
          conversationId: "builder-conversation",
          pickedPaths: ["/tmp/context"],
        },
      }),
    );
  });

  it("mounts the builder chat after a live ingest status response", async () => {
    mockBuilderCommands({ ingestLive: true });
    renderBuilder();

    expect(await screen.findByTestId("integrated-chat-panel")).toBeInTheDocument();
    expect(screen.queryByText("Add context to start")).not.toBeInTheDocument();
  });

  it("mounts the builder chat when ingestion copies context", async () => {
    mockBuilderCommands({ ingestLive: false });
    vi.mocked(openDialog).mockResolvedValue("/tmp/context" as never);
    renderBuilder();

    await screen.findByText("Add context to start");
    openContextPicker(screen.getByTestId("persona-builder-empty-add-context"));
    fireEvent.click(screen.getByRole("menuitem", { name: "Add folder" }));

    expect(await screen.findByTestId("integrated-chat-panel")).toBeInTheDocument();
  });

  it("fails closed to the context gate when ingest status lookup errors", async () => {
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "create_persona_builder_conversation") {
        return { id: "builder-conversation" };
      }
      if (command === "get_persona_builder_ingest_status") {
        throw new Error("ingest status unavailable");
      }
      throw new Error(`Unexpected command: ${command}`);
    });
    renderBuilder();

    expect(await screen.findByText("Add context to start")).toBeInTheDocument();
    expect(screen.queryByTestId("integrated-chat-panel")).not.toBeInTheDocument();
  });

  it("approves the authoritative draft and returns to the persona list", async () => {
    const onBack = vi.fn();
    mockBuilderCommands();
    render(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <EventProvider>
          <TooltipProvider><PersonaBuilderView projectId="project-1" onBack={onBack} /></TooltipProvider>
        </EventProvider>
      </QueryClientProvider>,
    );
    await waitFor(() => expect(window.__eventBus).toBeDefined());
    await emitDraftUpdated();
    await screen.findByText("Reviewer Voice");

    fireEvent.click(screen.getByRole("button", { name: "Approve" }));

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("approve_persona", { input: { id: "draft-1" } }),
    );
    expect(onBack).toHaveBeenCalled();
  });

  it("shows slug collision failures in the preview rail", async () => {
    mockBuilderCommands();
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "approve_persona") throw new Error("slug already exists");
      if (command === "create_persona_builder_conversation") return { id: "builder-conversation" };
      if (command === "get_persona") return rawPersona;
      throw new Error(`Unexpected command: ${command}`);
    });
    renderBuilder();
    await waitFor(() => expect(window.__eventBus).toBeDefined());
    await emitDraftUpdated();
    await screen.findByText("Reviewer Voice");

    fireEvent.click(screen.getByRole("button", { name: "Approve" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("slug already exists");
  });

  it("opens the context menu synchronously before starting a native picker", async () => {
    mockBuilderCommands();
    renderBuilder();

    expect(await screen.findByTestId("integrated-chat-panel")).toBeInTheDocument();
    openContextPicker(screen.getByRole("button", { name: "Add context…" }));

    expect(screen.getByRole("menuitem", { name: "Add files" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "Add folder" })).toBeInTheDocument();
    expect(openDialog).not.toHaveBeenCalled();
  });

  it("ingests multiple selected files as one batch", async () => {
    mockBuilderCommands();
    vi.mocked(openDialog).mockResolvedValue(["/tmp/one.md", "/tmp/two.txt"] as never);
    renderBuilder();

    expect(await screen.findByTestId("integrated-chat-panel")).toBeInTheDocument();
    openContextPicker(screen.getByRole("button", { name: "Add context…" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Add files" }));

    await waitFor(() =>
      expect(openDialog).toHaveBeenCalledWith({
        directory: false,
        multiple: true,
        title: "Add persona context files",
      }),
    );
    expect(invoke).toHaveBeenCalledWith("ingest_persona_context", {
      input: {
        conversationId: "builder-conversation",
        pickedPaths: ["/tmp/one.md", "/tmp/two.txt"],
      },
    });
  });

  it("keeps the builder open when context selection is cancelled", async () => {
    mockBuilderCommands();
    vi.mocked(openDialog).mockResolvedValue(null);
    renderBuilder();

    expect(await screen.findByTestId("integrated-chat-panel")).toBeInTheDocument();
    openContextPicker(screen.getByRole("button", { name: "Add context…" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Add folder" }));

    await waitFor(() =>
      expect(openDialog).toHaveBeenCalledWith({
        directory: true,
        multiple: false,
        title: "Add persona context folder",
      }),
    );
    expect(vi.mocked(invoke)).not.toHaveBeenCalledWith("ingest_persona_context", expect.anything());
    expect(screen.getByTestId("integrated-chat-panel")).toBeInTheDocument();
  });

  it("shows builder conversation creation errors without starting the embedded chat", async () => {
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "create_persona_builder_conversation") {
        throw new Error("Persona Builder is unavailable");
      }
      throw new Error(`Unexpected command: ${command}`);
    });
    renderBuilder();

    expect(await screen.findByRole("alert")).toHaveTextContent("Persona Builder is unavailable");
    expect(screen.queryByTestId("integrated-chat-panel")).not.toBeInTheDocument();
  });
});
