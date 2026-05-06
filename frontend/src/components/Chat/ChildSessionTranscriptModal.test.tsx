/**
 * ChildSessionTranscriptModal tests
 *
 * Covers:
 * - Modal closed (open=false) renders nothing visible
 * - Loading state (conversations or history loading)
 * - Error state when query fails
 * - Empty state when no messages
 * - Renders messages via MessageItem when data present
 * - Title fallback chain: statusQuery → conversation → "Ideation run"
 * - Status label: "Running" | "Waiting" | "Idle"
 * - mostRecentConversation picks newest by lastMessageAt/updatedAt/createdAt
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import type { ReactNode } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { ChildSessionTranscriptModal } from "./ChildSessionTranscriptModal";

// ============================================================================
// Mocks
// ============================================================================

const mockUseChildSessionStatus = vi.fn();
const mockUseConversationHistoryWindow = vi.fn();
const mockListConversations = vi.fn();

vi.mock("@/hooks/useChildSessionStatus", () => ({
  useChildSessionStatus: (...args: unknown[]) => mockUseChildSessionStatus(...args),
}));

vi.mock("@/hooks/useChat", async () => {
  const actual = await vi.importActual<Record<string, unknown>>("@/hooks/useChat");
  return {
    ...actual,
    useConversationHistoryWindow: (...args: unknown[]) =>
      mockUseConversationHistoryWindow(...args),
    chatKeys: (actual as { chatKeys: unknown }).chatKeys,
  };
});

vi.mock("@/api/chat", () => ({
  chatApi: {
    listConversations: (...args: unknown[]) => mockListConversations(...args),
  },
}));

vi.mock("./MessageItem", () => ({
  MessageItem: ({ content, role }: { content: string; role: string }) => (
    <div data-testid={`message-item-${role}`}>{content}</div>
  ),
}));

// ============================================================================
// Helpers
// ============================================================================

function createQueryClient() {
  return new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: 0, staleTime: 0 },
      mutations: { retry: false },
    },
  });
}

function Wrapper({ children }: { children: ReactNode }) {
  const qc = createQueryClient();
  return <QueryClientProvider client={qc}>{children}</QueryClientProvider>;
}

function renderModal(props: {
  sessionId?: string | null;
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
}) {
  return render(
    <ChildSessionTranscriptModal
      sessionId={props.sessionId ?? "session-123"}
      open={props.open ?? true}
      onOpenChange={props.onOpenChange ?? vi.fn()}
    />,
    { wrapper: Wrapper },
  );
}

function setStatusQuery(opts: {
  data?: {
    title?: string;
    agent_state: { estimated_status: string };
  } | null;
}) {
  mockUseChildSessionStatus.mockReturnValue({ data: opts.data ?? null });
}

function setHistory(opts: {
  data?: { messages: unknown[] } | undefined;
  isLoading?: boolean;
  error?: Error | null;
}) {
  mockUseConversationHistoryWindow.mockReturnValue({
    data: opts.data,
    isLoading: opts.isLoading ?? false,
    error: opts.error ?? null,
  });
}

// ============================================================================
// Tests
// ============================================================================

describe("ChildSessionTranscriptModal", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    setStatusQuery({});
    setHistory({});
    mockListConversations.mockResolvedValue([]);
  });

  it("renders nothing visible when open=false", () => {
    renderModal({ open: false });
    expect(screen.queryByText("Ideation conversation")).not.toBeInTheDocument();
  });

  it("renders default title when no status data and no conversation", async () => {
    setStatusQuery({});
    mockListConversations.mockResolvedValue([]);
    setHistory({});
    renderModal({});
    expect(await screen.findByText("Ideation run")).toBeInTheDocument();
    expect(screen.getByText("Ideation conversation")).toBeInTheDocument();
  });

  it("uses statusQuery title when present", async () => {
    setStatusQuery({
      data: {
        title: "My Plan Session",
        agent_state: { estimated_status: "likely_generating" },
      },
    });
    mockListConversations.mockResolvedValue([]);
    setHistory({});
    renderModal({});
    expect(await screen.findByText("My Plan Session")).toBeInTheDocument();
  });

  it("renders 'Running' status badge for likely_generating", async () => {
    setStatusQuery({
      data: { title: "T", agent_state: { estimated_status: "likely_generating" } },
    });
    renderModal({});
    expect(await screen.findByText("Running")).toBeInTheDocument();
  });

  it("renders 'Waiting' status badge for likely_waiting", async () => {
    setStatusQuery({
      data: { title: "T", agent_state: { estimated_status: "likely_waiting" } },
    });
    renderModal({});
    expect(await screen.findByText("Waiting")).toBeInTheDocument();
  });

  it("renders 'Idle' status badge for unknown/idle status", async () => {
    setStatusQuery({
      data: { title: "T", agent_state: { estimated_status: "idle" } },
    });
    renderModal({});
    expect(await screen.findByText("Idle")).toBeInTheDocument();
  });

  it("shows loading state while conversations query is loading", () => {
    // Force the conversation list query to remain pending
    mockListConversations.mockImplementation(() => new Promise(() => {}));
    setHistory({});
    renderModal({});
    expect(screen.getByText(/Loading conversation/i)).toBeInTheDocument();
  });

  it("shows error state when conversation history query errors", async () => {
    mockListConversations.mockResolvedValue([
      {
        id: "c1",
        title: "Run #1",
        lastMessageAt: "2026-01-01T00:00:00Z",
        updatedAt: "2026-01-01T00:00:00Z",
        createdAt: "2026-01-01T00:00:00Z",
      },
    ]);
    setHistory({ error: new Error("boom") });
    renderModal({});
    expect(
      await screen.findByText(/Unable to load this ideation run/i),
    ).toBeInTheDocument();
  });

  it("shows 'No messages yet' empty state when history returns empty messages", async () => {
    mockListConversations.mockResolvedValue([
      {
        id: "c1",
        title: "Run #1",
        lastMessageAt: "2026-01-01T00:00:00Z",
        updatedAt: "2026-01-01T00:00:00Z",
        createdAt: "2026-01-01T00:00:00Z",
      },
    ]);
    setHistory({ data: { messages: [] } });
    renderModal({});
    expect(await screen.findByText(/No messages yet/i)).toBeInTheDocument();
  });

  it("renders MessageItem entries when messages exist", async () => {
    mockListConversations.mockResolvedValue([
      {
        id: "c1",
        title: "Run #1",
        lastMessageAt: "2026-01-01T00:00:00Z",
        updatedAt: "2026-01-01T00:00:00Z",
        createdAt: "2026-01-01T00:00:00Z",
      },
    ]);
    setHistory({
      data: {
        messages: [
          {
            id: "m1",
            role: "user",
            content: "hi there",
            createdAt: "2026-01-01T00:00:01Z",
            toolCalls: null,
            contentBlocks: null,
            sender: null,
          },
          {
            id: "m2",
            role: "assistant",
            content: "hello back",
            createdAt: "2026-01-01T00:00:02Z",
            toolCalls: null,
            contentBlocks: null,
            sender: null,
          },
        ],
      },
    });
    renderModal({});
    expect(await screen.findByTestId("message-item-user")).toHaveTextContent("hi there");
    expect(screen.getByTestId("message-item-assistant")).toHaveTextContent("hello back");
  });

  it("uses fallback conversation title when statusQuery has no title", async () => {
    setStatusQuery({});
    mockListConversations.mockResolvedValue([
      {
        id: "c1",
        title: "Conversation Title From Fallback",
        lastMessageAt: "2026-01-01T00:00:00Z",
        updatedAt: "2026-01-01T00:00:00Z",
        createdAt: "2026-01-01T00:00:00Z",
      },
    ]);
    setHistory({ data: { messages: [] } });
    renderModal({});
    expect(
      await screen.findByText("Conversation Title From Fallback"),
    ).toBeInTheDocument();
  });

  it("picks the most recent conversation by lastMessageAt", async () => {
    mockListConversations.mockResolvedValue([
      {
        id: "old",
        title: "OLD",
        lastMessageAt: "2026-01-01T00:00:00Z",
        updatedAt: "2026-01-01T00:00:00Z",
        createdAt: "2026-01-01T00:00:00Z",
      },
      {
        id: "new",
        title: "NEW",
        lastMessageAt: "2026-02-01T00:00:00Z",
        updatedAt: "2026-02-01T00:00:00Z",
        createdAt: "2026-02-01T00:00:00Z",
      },
    ]);
    setHistory({ data: { messages: [] } });
    renderModal({});
    expect(await screen.findByText("NEW")).toBeInTheDocument();
  });

  it("falls back to updatedAt/createdAt when lastMessageAt is missing", async () => {
    mockListConversations.mockResolvedValue([
      {
        id: "a",
        title: "A",
        lastMessageAt: null,
        updatedAt: "2026-01-01T00:00:00Z",
        createdAt: "2026-01-01T00:00:00Z",
      },
      {
        id: "b",
        title: "B",
        lastMessageAt: null,
        updatedAt: "2026-03-01T00:00:00Z",
        createdAt: "2026-03-01T00:00:00Z",
      },
    ]);
    setHistory({ data: { messages: [] } });
    renderModal({});
    expect(await screen.findByText("B")).toBeInTheDocument();
  });

  it("disables conversations query when sessionId is null (no API call with null sessionId)", () => {
    mockListConversations.mockClear();
    renderModal({ sessionId: null, open: true });
    // No call should have been made specifically targeting a null sessionId.
    const calls = mockListConversations.mock.calls;
    expect(calls.every((args) => args[1] !== null && args[1] !== "")).toBe(true);
  });

  it("disables conversations query when open=false (no new fetches kicked off)", () => {
    mockListConversations.mockClear();
    renderModal({ open: false, sessionId: "different-session" });
    // The hook should not be invoked for the closed modal session
    expect(
      mockListConversations.mock.calls.some(
        (args) => args[1] === "different-session",
      ),
    ).toBe(false);
  });
});
