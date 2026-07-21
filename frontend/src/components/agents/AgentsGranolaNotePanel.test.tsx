import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  granolaApi,
  type AgentConversationGranolaNote,
  type GranolaNoteDetail,
  type GranolaNoteSummary,
} from "@/api/granola";
import { TooltipProvider } from "@/components/ui/tooltip";

import { AgentsGranolaNotePanel } from "./AgentsGranolaNotePanel";

vi.mock("@/api/granola", async () => {
  const actual = await vi.importActual<typeof import("@/api/granola")>("@/api/granola");
  return {
    ...actual,
    granolaApi: {
      getAgentConversationGranolaNote: vi.fn(),
      assignAgentConversationGranolaNote: vi.fn(),
      refreshAgentConversationGranolaNote: vi.fn(),
      clearAgentConversationGranolaNote: vi.fn(),
      listNotes: vi.fn(),
      getNoteDetail: vi.fn(),
    },
  };
});

vi.mock("sonner", () => ({
  toast: {
    error: vi.fn(),
    success: vi.fn(),
  },
}));

const getNoteMock = vi.mocked(granolaApi.getAgentConversationGranolaNote);
const listNotesMock = vi.mocked(granolaApi.listNotes);
const detailMock = vi.mocked(granolaApi.getNoteDetail);
const assignMock = vi.mocked(granolaApi.assignAgentConversationGranolaNote);

function noteSummary(): GranolaNoteSummary {
  return {
    id: "not_1234567890ABCD",
    title: "Planning sync",
    url: "https://granola.ai/notes/not_1234567890ABCD",
    summary: "Discussed the plan",
    createdAt: "2026-06-20T12:00:00Z",
    updatedAt: "2026-06-20T13:00:00Z",
  };
}

function noteDetail(): GranolaNoteDetail {
  return {
    ...noteSummary(),
    transcript: [{ speaker: "Alex", text: "Ship it", startMs: 10, endMs: 20 }],
  };
}

function boundNote(): AgentConversationGranolaNote {
  return {
    conversationId: "conversation-1",
    projectId: "project-1",
    provider: "granola",
    noteId: "not_1234567890ABCD",
    noteUrl: "https://granola.ai/notes/not_1234567890ABCD",
    title: "Planning sync",
    summaryMarkdown: "Discussed the plan",
    transcript: [{ speaker: "Alex", text: "Ship it", startMs: 10, endMs: 20 }],
    includeTranscript: true,
    lastRefreshedAt: "2026-06-20T13:00:00Z",
    refreshStatus: "loaded",
    refreshError: null,
    assignedAt: "2026-06-20T12:00:00Z",
    assignedFromMessageId: null,
    manuallyAssigned: true,
    createdAt: "2026-06-20T12:00:00Z",
    updatedAt: "2026-06-20T13:00:00Z",
  };
}

function renderPanel() {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  });
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={queryClient}>
      <TooltipProvider>{children}</TooltipProvider>
    </QueryClientProvider>
  );
  render(
    <AgentsGranolaNotePanel conversationId="conversation-1" projectId="project-1" />,
    { wrapper },
  );
}

describe("AgentsGranolaNotePanel", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    getNoteMock.mockResolvedValue(null);
    listNotesMock.mockResolvedValue({
      notes: [noteSummary()],
      hasMore: false,
      cursor: null,
    });
    detailMock.mockResolvedValue(noteDetail());
    assignMock.mockResolvedValue(boundNote());
  });

  it("lists Granola notes, previews detail, and binds a note", async () => {
    renderPanel();

    fireEvent.click(await screen.findByRole("button", { name: /Planning sync/i }));
    expect(await screen.findByText("Ship it")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Bind" }));

    await waitFor(() =>
      expect(assignMock).toHaveBeenCalledWith({
        conversationId: "conversation-1",
        projectId: "project-1",
        noteId: "not_1234567890ABCD",
        title: "Planning sync",
        noteUrl: "https://granola.ai/notes/not_1234567890ABCD",
        summary: "Discussed the plan",
        includeTranscript: true,
      }),
    );
  });

  it("uses theme-aware prose colors for bound note markdown", async () => {
    getNoteMock.mockResolvedValue({
      ...boundNote(),
      summaryMarkdown: "### Progress\n\n- Ship Granola note browsing.",
    });

    renderPanel();

    const markdown = await screen.findByTestId("agent-granola-note-markdown");
    expect(markdown).toHaveClass("theme-aware-prose");
    expect(
      within(markdown).getByRole("heading", { name: "Progress" }),
    ).toBeInTheDocument();
  });

  it("removes line selection while keeping bound Granola content selectable", async () => {
    getNoteMock.mockResolvedValue(boundNote());

    renderPanel();

    const summary = await screen.findByText("Discussed the plan");
    expect(
      screen.queryByRole("button", { name: "Select Granola note lines" }),
    ).not.toBeInTheDocument();
    expect(
      summary.closest("[data-artifact-selectable-region='true']"),
    ).not.toBeNull();
  });
});
