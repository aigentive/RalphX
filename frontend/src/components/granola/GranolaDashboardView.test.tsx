import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { granolaApi } from "@/api/granola";
import * as chatHooks from "@/hooks/useChat";
import { TooltipProvider } from "@/components/ui/tooltip";
import type { Project } from "@/types/project";

import { GranolaDashboardView } from "./GranolaDashboardView";

vi.mock("@/api/granola", () => ({
  granolaApi: {
    getSettings: vi.fn(),
    listNotes: vi.fn(),
    getNoteDetail: vi.fn(),
    assignAgentConversationGranolaNote: vi.fn(),
  },
}));

vi.mock("@/hooks/useChat", () => ({
  useConversations: vi.fn(),
}));

vi.mock("@/components/agents/agentGranolaNoteQueries", () => ({
  invalidateAgentConversationGranolaNote: vi.fn().mockResolvedValue(undefined),
}));

const project: Project = {
  id: "project-1",
  name: "Current Project",
  workingDirectory: "/repo/current",
  gitMode: "worktree",
  baseBranch: "main",
  worktreeParentDirectory: null,
  useFeatureBranches: true,
  mergeValidationMode: "block",
  detectedAnalysis: null,
  customAnalysis: null,
  analyzedAt: null,
  githubPrEnabled: false,
  createdAt: "2026-06-19T22:00:00.000Z",
  updatedAt: "2026-06-19T22:00:00.000Z",
};

const targetProject: Project = {
  ...project,
  id: "project-2",
  name: "Target Project",
  workingDirectory: "/repo/target",
};

const granolaNote = {
  id: "not_1234567890ABCD",
  title: "Weekly planning",
  url: "https://granola.ai/notes/not_1234567890ABCD",
  summary: "Discussed release priorities.",
  createdAt: "2026-06-19T21:00:00.000Z",
  updatedAt: "2026-06-19T22:00:00.000Z",
};

function createWrapper() {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: 0 },
    },
  });

  return function Wrapper({ children }: { children: ReactNode }) {
    return (
      <QueryClientProvider client={queryClient}>
        <TooltipProvider>{children}</TooltipProvider>
      </QueryClientProvider>
    );
  };
}

function renderGranolaView(
  props: Partial<Parameters<typeof GranolaDashboardView>[0]> = {},
) {
  const Wrapper = createWrapper();
  return render(
    <GranolaDashboardView
      projectId="project-1"
      project={project}
      projects={[project, targetProject]}
      onStartConversation={vi.fn()}
      {...props}
    />,
    { wrapper: Wrapper },
  );
}

describe("GranolaDashboardView", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: {
        writeText: vi.fn().mockResolvedValue(undefined),
      },
    });
    vi.mocked(chatHooks.useConversations).mockReturnValue({
      data: [],
      isLoading: false,
      isError: false,
      error: null,
    } as unknown as ReturnType<typeof chatHooks.useConversations>);
    vi.mocked(granolaApi.getSettings).mockResolvedValue({
      enabled: true,
      hasApiToken: true,
      validationStatus: "valid",
      lastValidatedAt: "2026-06-19T22:00:00.000Z",
      lastError: null,
      updatedAt: "2026-06-19T22:00:00.000Z",
    });
    vi.mocked(granolaApi.listNotes).mockResolvedValue({
      notes: [granolaNote],
      hasMore: false,
      cursor: null,
    });
    vi.mocked(granolaApi.getNoteDetail).mockResolvedValue({
      id: granolaNote.id,
      title: granolaNote.title,
      url: granolaNote.url,
      summary: "### Release priorities\n\n- Ship Granola note browsing.",
      transcript: [
        {
          speaker: "Ada",
          text: "We should finish the standalone Granola dashboard.",
          startMs: 0,
          endMs: 2400,
        },
      ],
    });
    vi.mocked(granolaApi.assignAgentConversationGranolaNote).mockResolvedValue({
      conversationId: "conversation-1",
      projectId: "project-1",
      provider: "granola",
      noteId: granolaNote.id,
      noteUrl: granolaNote.url,
      title: granolaNote.title,
      summaryMarkdown: "### Release priorities\n\n- Ship Granola note browsing.",
      transcript: [],
      includeTranscript: true,
      lastRefreshedAt: "2026-06-19T22:00:00.000Z",
      refreshStatus: "loaded",
      refreshError: null,
      assignedAt: "2026-06-19T22:00:00.000Z",
      assignedFromMessageId: null,
      manuallyAssigned: true,
      createdAt: "2026-06-19T22:00:00.000Z",
      updatedAt: "2026-06-19T22:00:00.000Z",
    });
  });

  it("renders grouped notes and copies summary and transcript text", async () => {
    renderGranolaView();

    expect(await screen.findByTestId("granola-dashboard-view")).toBeInTheDocument();
    const row = await screen.findByTestId(`granola-note-row-${granolaNote.id}`);
    expect(row).toHaveTextContent("Weekly planning");
    expect(row).toHaveTextContent(/Jun (19|20)/);
    expect(row).toHaveTextContent(/\d{1,2}:00/);
    expect(granolaApi.listNotes).toHaveBeenCalledWith({
      pageSize: 30,
      projectId: "project-1",
    });
    expect(chatHooks.useConversations).toHaveBeenCalledWith({
      view: "granola",
      projectId: "project-1",
    });

    expect(await screen.findByRole("heading", { name: "Release priorities" })).toBeInTheDocument();
    expect(await screen.findByText("We should finish the standalone Granola dashboard.")).toBeInTheDocument();
    expect(granolaApi.getNoteDetail).toHaveBeenCalledWith({
      noteId: granolaNote.id,
      includeTranscript: true,
    });

    fireEvent.click(screen.getByRole("button", { name: "Copy summary" }));

    await waitFor(() => {
      expect(navigator.clipboard.writeText).toHaveBeenCalledWith(
        "### Release priorities\n\n- Ship Granola note browsing.",
      );
    });

    fireEvent.click(screen.getByRole("button", { name: "Copy full transcript" }));

    await waitFor(() => {
      expect(navigator.clipboard.writeText).toHaveBeenCalledWith(
        "Ada: We should finish the standalone Granola dashboard.",
      );
    });
  });

  it("filters notes from the standalone header controls", async () => {
    renderGranolaView();

    await screen.findByTestId(`granola-note-row-${granolaNote.id}`);

    fireEvent.change(screen.getByPlaceholderText("Search notes, summaries, or links"), {
      target: { value: "missing" },
    });
    expect(screen.getByText("No Granola notes match these filters.")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Reset filters" }));
    expect(screen.getByTestId(`granola-note-row-${granolaNote.id}`)).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "No summary 0" }));
    expect(screen.getByText("No Granola notes match these filters.")).toBeInTheDocument();
  });

  it("shows RX conversation, ticket, and PR associations for a Granola note", async () => {
    vi.mocked(granolaApi.listNotes).mockResolvedValue({
      notes: [
        {
          ...granolaNote,
          rxConversationCount: 1,
          rxConversations: [
            {
              conversationId: "conversation-1",
              title: "Planning agent",
            },
          ],
          ticketCount: 1,
          ticketLinks: [
            {
              provider: "clickup",
              label: "TASK-123",
              title: "ClickUp implementation ticket",
              url: "https://app.clickup.com/t/TASK-123",
            },
          ],
          prCount: 1,
          pullRequests: [
            {
              number: 466,
              url: "https://github.com/aigentive/ralphx.app/pull/466",
              status: "merged",
            },
          ],
        },
      ],
      hasMore: false,
      cursor: null,
    });

    renderGranolaView();

    await screen.findByTestId(`granola-note-row-${granolaNote.id}`);
    await waitFor(() => {
      expect(screen.getAllByLabelText("1 RalphX conversation attached")).toHaveLength(2);
      expect(screen.getAllByLabelText("1 ticket attached")).toHaveLength(2);
      expect(screen.getAllByLabelText("1 pull request attached")).toHaveLength(2);
    });
    expect(screen.getByText("Planning agent")).toBeInTheDocument();
    expect(screen.getByText("ClickUp TASK-123")).toBeInTheDocument();
    expect(screen.getByText("PR #466 merged")).toBeInTheDocument();

    fireEvent.change(screen.getByPlaceholderText("Search notes, summaries, or links"), {
      target: { value: "TASK-123" },
    });

    expect(screen.getByTestId(`granola-note-row-${granolaNote.id}`)).toBeInTheDocument();
  });

  it("starts a new conversation from a note or binds it to an existing conversation", async () => {
    const onStartConversation = vi.fn();
    vi.mocked(chatHooks.useConversations).mockReturnValue({
      data: [{ id: "conversation-1", title: "Existing agent conversation" }],
      isLoading: false,
      isError: false,
      error: null,
    } as unknown as ReturnType<typeof chatHooks.useConversations>);

    renderGranolaView({ onStartConversation });

    await screen.findByText("We should finish the standalone Granola dashboard.");

    fireEvent.click(screen.getByRole("button", { name: "Add as context" }));

    expect(screen.getByRole("dialog", { name: "Add Granola Context" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Open composer" }));

    expect(onStartConversation).toHaveBeenCalledWith(
      expect.objectContaining({
        id: granolaNote.id,
        title: granolaNote.title,
      }),
      "project-1",
    );

    fireEvent.click(screen.getByRole("button", { name: "Add as context" }));
    fireEvent.click(screen.getByRole("combobox", { name: "Existing conversation" }));
    fireEvent.click(
      within(screen.getByRole("listbox", { name: "Existing conversation" })).getByRole(
        "option",
        { name: "Existing agent conversation" },
      ),
    );
    fireEvent.click(screen.getByRole("button", { name: "Bind existing conversation" }));

    await waitFor(() => {
      expect(granolaApi.assignAgentConversationGranolaNote).toHaveBeenCalledWith({
        conversationId: "conversation-1",
        projectId: "project-1",
        noteId: granolaNote.id,
        title: granolaNote.title,
        noteUrl: granolaNote.url,
        summary: "### Release priorities\n\n- Ship Granola note browsing.",
        includeTranscript: true,
        refresh: true,
      });
    });
  });
});
