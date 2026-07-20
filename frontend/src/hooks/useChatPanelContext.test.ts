/**
 * useChatPanelContext hook tests
 *
 * Tests for context switching behavior and conversation selection logic,
 * ensuring no intermediate empty state during context transitions.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { renderHook, waitFor, act } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createElement } from "react";
import { useChatPanelContext } from "./useChatPanelContext";
import { useChatStore } from "@/stores/chatStore";

// Mock sonner toast
const mockToast = vi.fn();
vi.mock("sonner", () => ({
  toast: (message: string, options?: unknown) => mockToast(message, options),
}));

// Mock ideation store
const mockSetActiveSession = vi.fn();
vi.mock("@/stores/ideationStore", () => ({
  useIdeationStore: Object.assign(
    vi.fn(),
    { getState: () => ({ setActiveSession: mockSetActiveSession }) },
  ),
}));

interface MockState {
  activeConversationIds: Record<string, string | null>;
  setActiveConversation: ReturnType<typeof vi.fn>;
  clearMessages: ReturnType<typeof vi.fn>;
  setAgentRunning: ReturnType<typeof vi.fn>;
  setSending: ReturnType<typeof vi.fn>;
}

interface ChatContext {
  view: string;
  projectId: string;
  ideationSessionId?: string;
  selectedTaskId?: string;
}

// Mock chat store
vi.mock("@/stores/chatStore", () => ({
  useChatStore: vi.fn(),
  selectActiveConversationId: vi.fn((storeKey: string) => (state: MockState) => state.activeConversationIds[storeKey] ?? null),
  getContextKey: vi.fn((context: ChatContext) => {
    // Mirrors real implementation: ideation uses "session" prefix (from chat-context-registry storeKeyPrefix)
    if (context.view === "ideation") return `session:${context.ideationSessionId}`;
    if (context.view === "task_detail") return `task:${context.selectedTaskId}`;
    return `project:${context.projectId}`;
  }),
}));

// Mock chat API
vi.mock("@/api/chat", () => ({
  chatApi: {
    listConversations: vi.fn(),
    getConversation: vi.fn(),
  },
}));

// Mock useChat hook
vi.mock("./useChat", () => ({
  chatKeys: {
    conversation: (id: string) => ["conversation", id],
    conversationList: (type: string, id: string) => ["conversations", type, id],
    agentRun: (id: string) => ["agent-run", id],
  },
}));

interface ConversationData {
  id: string;
  lastMessageAt?: string | null;
  createdAt: string;
}

describe("useChatPanelContext", () => {
  let queryClient: QueryClient;
  let mockStore: MockState;

  beforeEach(() => {
    queryClient = new QueryClient({
      defaultOptions: {
        queries: { retry: false },
        mutations: { retry: false },
      },
    });

    // Setup mock store
    mockStore = {
      activeConversationIds: {},
      setActiveConversation: vi.fn(),
      clearMessages: vi.fn(),
      setAgentRunning: vi.fn(),
      setSending: vi.fn(),
    };

    (useChatStore as unknown as { mockImplementation: (fn: (selector: ((state: MockState) => unknown) | undefined) => unknown) => void }).mockImplementation((selector) => {
      if (typeof selector === "function") {
        return selector(mockStore);
      }
      return mockStore;
    });

    (useChatStore as unknown as { getState: () => MockState }).getState = vi.fn(() => mockStore);

  });

  afterEach(() => {
    vi.clearAllMocks();
    mockToast.mockClear();
    mockSetActiveSession.mockClear();
    queryClient.clear();
  });

  const wrapper = ({ children }: { children: React.ReactNode }) =>
    createElement(QueryClientProvider, { client: queryClient }, children);

  describe("unmount cleanup", () => {
    it("should clear isSending (but NOT agentStatus) for current storeContextKey on unmount", async () => {
      const { unmount } = renderHook(
        (props) => useChatPanelContext(props),
        {
          wrapper,
          initialProps: {
            projectId: "project-1",
            ideationSessionId: "session-1",
            selectedTaskId: undefined,
            isExecutionMode: false,
            isReviewMode: false,
            isMergeMode: false,
            isHistoryMode: false,
          },
        }
      );

      // Unmount the hook (simulates switching sessions with key={session.id})
      unmount();

      // agentStatus is owned by useGlobalAgentLifecycle — must NOT be cleared on unmount
      expect(mockStore.setAgentRunning).not.toHaveBeenCalledWith("session:session-1", false);
      // isSending is per-panel UI state — should still be cleared
      expect(mockStore.setSending).toHaveBeenCalledWith("session:session-1", false);
    });

    it("uses an explicit store key for externally owned project conversations", async () => {
      const { result } = renderHook(
        (props) => useChatPanelContext(props),
        {
          wrapper,
          initialProps: {
            projectId: "project-1",
            ideationSessionId: undefined,
            selectedTaskId: undefined,
            isExecutionMode: false,
            isReviewMode: false,
            isMergeMode: false,
            isHistoryMode: false,
            overrideConversationId: "conversation-1",
            storeContextKeyOverride: "project:conversation-1",
          },
        }
      );

      expect(result.current.storeContextKey).toBe("project:conversation-1");
      await waitFor(() => {
        expect(mockStore.setActiveConversation).toHaveBeenCalledWith(
          "project:conversation-1",
          "conversation-1"
        );
      });
    });

    it("routes a projectless panel through its standalone self-keyed context", () => {
      const { result } = renderHook(
        (props) => useChatPanelContext(props),
        {
          wrapper,
          initialProps: {
            projectId: null,
            contextTypeOverride: "standalone" as const,
            contextIdOverride: "standalone-1",
            ideationSessionId: undefined,
            selectedTaskId: undefined,
            isExecutionMode: false,
            isReviewMode: false,
            isMergeMode: false,
            isHistoryMode: false,
            overrideConversationId: "standalone-1",
            storeContextKeyOverride: "standalone:standalone-1",
          },
        },
      );

      expect(result.current.currentContextType).toBe("standalone");
      expect(result.current.currentContextId).toBe("standalone-1");
      expect(result.current.storeContextKey).toBe("standalone:standalone-1");
      expect(result.current.chatContext).toEqual(
        expect.objectContaining({
          contextTypeOverride: "standalone",
          contextIdOverride: "standalone-1",
        }),
      );
    });
  });

  describe("context switching", () => {
    it("should clear isSending (but NOT agentStatus) for OLD storeContextKey on context switch", async () => {
      const { rerender } = renderHook(
        (props) => useChatPanelContext(props),
        {
          wrapper,
          initialProps: {
            projectId: "project-1",
            ideationSessionId: "session-1",
            selectedTaskId: undefined,
            isExecutionMode: false,
            isReviewMode: false,
            isMergeMode: false,
            isHistoryMode: false,
          },
        }
      );

      // Clear calls from initial mount
      mockStore.setAgentRunning.mockClear();
      mockStore.setSending.mockClear();

      // Switch to a different session
      rerender({
        projectId: "project-1",
        ideationSessionId: "session-2",
        selectedTaskId: undefined,
        isExecutionMode: false,
        isReviewMode: false,
        isMergeMode: false,
        isHistoryMode: false,
      });

      // agentStatus is owned by useGlobalAgentLifecycle — must NOT be cleared on context switch
      // (mock getContextKey returns "session:<id>" for ideation view, mirroring registry storeKeyPrefix)
      await waitFor(() => {
        expect(mockStore.setSending).toHaveBeenCalledWith("session:session-1", false);
      });
      expect(mockStore.setAgentRunning).not.toHaveBeenCalledWith("session:session-1", false);

      // Should NOT have cleared the NEW session's key either
      expect(mockStore.setAgentRunning).not.toHaveBeenCalledWith("session:session-2", false);
    });

    it("should clear messages for old context during context change", async () => {
      const { rerender } = renderHook(
        (props) => useChatPanelContext(props),
        {
          wrapper,
          initialProps: {
            projectId: "project-1",
            ideationSessionId: "session-1",
            selectedTaskId: undefined,
            isExecutionMode: false,
            isReviewMode: false,
            isMergeMode: false,
            isHistoryMode: false,
          },
        }
      );

      // Switch to task context
      rerender({
        projectId: "project-1",
        ideationSessionId: undefined,
        selectedTaskId: "task-1",
        isExecutionMode: true,
        isReviewMode: false,
        isMergeMode: false,
        isHistoryMode: false,
      });

      // Verify cleanup was called with correct old context
      await waitFor(() => {
        expect(mockStore.clearMessages).toHaveBeenCalledWith("ideation:session-1");
      });
    });

    it("should NOT set activeConversationId to null during context switch", async () => {
      mockStore.activeConversationIds["session:session-1"] = "conv-1";

      const { rerender } = renderHook(
        (props) => useChatPanelContext(props),
        {
          wrapper,
          initialProps: {
            projectId: "project-1",
            ideationSessionId: "session-1",
            selectedTaskId: undefined,
            isExecutionMode: false,
            isReviewMode: false,
            isMergeMode: false,
            isHistoryMode: false,
          },
        }
      );

      // Verify initial conversation is set
      expect(mockStore.activeConversationIds["session:session-1"]).toBe("conv-1");

      // Switch context
      rerender({
        projectId: "project-1",
        ideationSessionId: undefined,
        selectedTaskId: "task-1",
        isExecutionMode: true,
        isReviewMode: false,
        isMergeMode: false,
        isHistoryMode: false,
      });

      // Verify setActiveConversation(storeKey, null) was NOT called during context switch
      // (it should only be called by autoSelectConversation if needed)
      const nullCalls = mockStore.setActiveConversation.mock.calls.filter(
        (call: [string, string | null]) => call[1] === null
      );
      expect(nullCalls.length).toBe(0);
    });
  });

  describe("autoSelectConversation", () => {
    it("should directly select new conversation when current is stale, without intermediate null", async () => {
      mockStore.activeConversationIds["task_execution:task-1"] = "conv-1";

      const { result } = renderHook(
        (props) => useChatPanelContext(props),
        {
          wrapper,
          initialProps: {
            projectId: "project-1",
            ideationSessionId: undefined,
            selectedTaskId: "task-1",
            isExecutionMode: true,
            isReviewMode: false,
            isMergeMode: false,
            isHistoryMode: false,
          },
        }
      );

      const mockConversations: ConversationData[] = [
        {
          id: "conv-2",
          lastMessageAt: "2026-02-11T12:00:00Z",
          createdAt: "2026-02-11T11:00:00Z",
        },
        {
          id: "conv-3",
          lastMessageAt: "2026-02-11T11:30:00Z",
          createdAt: "2026-02-11T11:00:00Z",
        },
      ];

      // Call autoSelectConversation with conversations that don't include conv-1
      act(() => {
        result.current.autoSelectConversation({
          data: mockConversations,
          isLoading: false,
        });
      });

      // Should have selected conv-2 (most recent) directly without setting null first
      const calls = mockStore.setActiveConversation.mock.calls;
      expect(calls.length).toBe(1);
      expect(calls[0][1]).toBe("conv-2"); // second arg is the conv ID (first is storeKey)

      // Verify no null was set
      const nullCalls = calls.filter((call: [string, string | null]) => call[1] === null);
      expect(nullCalls.length).toBe(0);
    });

    it("should NOT clear conversation when new context has no conversations (early return)", async () => {
      mockStore.activeConversationIds["task_execution:task-1"] = "conv-1";

      const { result } = renderHook(
        (props) => useChatPanelContext(props),
        {
          wrapper,
          initialProps: {
            projectId: "project-1",
            ideationSessionId: undefined,
            selectedTaskId: "task-1",
            isExecutionMode: true,
            isReviewMode: false,
            isMergeMode: false,
            isHistoryMode: false,
          },
        }
      );

      // Call autoSelectConversation with empty conversation list
      act(() => {
        result.current.autoSelectConversation({
          data: [],
          isLoading: false,
        });
      });

      // Should NOT set null — the stale ID is safe because
      // isConversationInCurrentContext guards against wrong-context messages,
      // and auto-select will correct when the list populates
      const calls = mockStore.setActiveConversation.mock.calls;
      expect(calls.length).toBe(0);
    });

    it("should select most recent conversation by lastMessageAt", async () => {
      mockStore.activeConversationIds["task:task-1"] = "conv-old";

      const { result } = renderHook(
        (props) => useChatPanelContext(props),
        {
          wrapper,
          initialProps: {
            projectId: "project-1",
            ideationSessionId: undefined,
            selectedTaskId: "task-1",
            isExecutionMode: false, // Non-agent context: sorts by lastMessageAt not createdAt
            isReviewMode: false,
            isMergeMode: false,
            isHistoryMode: false,
          },
        }
      );

      const mockConversations: ConversationData[] = [
        {
          id: "conv-1",
          lastMessageAt: "2026-02-11T10:00:00Z",
          createdAt: "2026-02-11T09:00:00Z",
        },
        {
          id: "conv-2",
          lastMessageAt: "2026-02-11T12:00:00Z", // Most recent
          createdAt: "2026-02-11T09:30:00Z",
        },
        {
          id: "conv-3",
          lastMessageAt: "2026-02-11T11:00:00Z",
          createdAt: "2026-02-11T10:00:00Z",
        },
      ];

      act(() => {
        result.current.autoSelectConversation({
          data: mockConversations,
          isLoading: false,
        });
      });

      // Should select conv-2 (most recent lastMessageAt)
      expect(mockStore.setActiveConversation).toHaveBeenCalledWith("task:task-1", "conv-2");
    });

    it("should have stable callback reference across re-renders (activeConversationId not in deps)", async () => {
      mockStore.activeConversationIds = {};

      const { result, rerender } = renderHook(
        (props) => useChatPanelContext(props),
        {
          wrapper,
          initialProps: {
            projectId: "project-1",
            ideationSessionId: undefined,
            selectedTaskId: "task-1",
            isExecutionMode: true,
            isReviewMode: false,
            isMergeMode: false,
            isHistoryMode: false,
          },
        }
      );

      const firstRef = result.current.autoSelectConversation;

      // Simulate activeConversationId changing (e.g., after autoSelect runs)
      mockStore.activeConversationIds["task_execution:task-1"] = "conv-1";

      // Re-render with same props — only activeConversationId changed in store
      rerender({
        projectId: "project-1",
        ideationSessionId: undefined,
        selectedTaskId: "task-1",
        isExecutionMode: true,
        isReviewMode: false,
        isMergeMode: false,
        isHistoryMode: false,
      });

      const secondRef = result.current.autoSelectConversation;

      // Callback should be the SAME reference — activeConversationId is not a dep
      expect(secondRef).toBe(firstRef);
    });

    it("should read activeConversationId from store snapshot inside callback", async () => {
      // Start with no active conversation
      mockStore.activeConversationIds = {};

      const { result } = renderHook(
        (props) => useChatPanelContext(props),
        {
          wrapper,
          initialProps: {
            projectId: "project-1",
            ideationSessionId: undefined,
            selectedTaskId: "task-1",
            isExecutionMode: true,
            isReviewMode: false,
            isMergeMode: false,
            isHistoryMode: false,
          },
        }
      );

      // Now update the store directly (simulating a previous selection)
      mockStore.activeConversationIds["task_execution:task-1"] = "conv-existing";

      const mockConversations: ConversationData[] = [
        {
          id: "conv-existing",
          lastMessageAt: "2026-02-11T12:00:00Z",
          createdAt: "2026-02-11T11:00:00Z",
        },
      ];

      // Call autoSelectConversation — it should read the CURRENT store value
      // ("conv-existing"), not the stale closure value (null)
      act(() => {
        result.current.autoSelectConversation({
          data: mockConversations,
          isLoading: false,
        });
      });

      // conv-existing belongs to context and is already active — no call needed
      expect(mockStore.setActiveConversation).not.toHaveBeenCalled();
    });

    it("should not auto-select in history mode with explicit override", async () => {
      const { result } = renderHook(
        (props) => useChatPanelContext(props),
        {
          wrapper,
          initialProps: {
            projectId: "project-1",
            ideationSessionId: undefined,
            selectedTaskId: "task-1",
            isExecutionMode: false,
            isReviewMode: true,
            isMergeMode: false,
            isHistoryMode: true,
            overrideConversationId: "conv-history",
          },
        }
      );

      // Wait for override effect to run
      await waitFor(() => {
        expect(mockStore.setActiveConversation).toHaveBeenCalledWith("review:task-1", "conv-history");
      });

      // Clear the mock calls
      mockStore.setActiveConversation.mockClear();

      const mockConversations: ConversationData[] = [
        {
          id: "conv-1",
          lastMessageAt: "2026-02-11T12:00:00Z",
          createdAt: "2026-02-11T11:00:00Z",
        },
      ];

      act(() => {
        result.current.autoSelectConversation({
          data: mockConversations,
          isLoading: false,
        });
      });

      // Should not have called setActiveConversation again because we're in history mode
      // with an explicit override
      expect(mockStore.setActiveConversation).not.toHaveBeenCalled();
    });

    it("treats null history override as an explicit no-transcript selection", async () => {
      mockStore.activeConversationIds["review:task-1"] = "stale-review-conv";

      const { result } = renderHook(
        (props) => useChatPanelContext(props),
        {
          wrapper,
          initialProps: {
            projectId: "project-1",
            ideationSessionId: undefined,
            selectedTaskId: "task-1",
            isExecutionMode: false,
            isReviewMode: true,
            isMergeMode: false,
            isHistoryMode: true,
            overrideConversationId: null,
          },
        }
      );

      expect(result.current.activeConversationId).toBeNull();
      await waitFor(() => {
        expect(mockStore.setActiveConversation).toHaveBeenCalledWith(
          "review:task-1",
          null
        );
      });

      mockStore.setActiveConversation.mockClear();

      act(() => {
        result.current.autoSelectConversation({
          data: [
            {
              id: "new-review-conv",
              lastMessageAt: "2026-02-11T12:00:00Z",
              createdAt: "2026-02-11T11:00:00Z",
            },
          ],
          isLoading: false,
        });
      });

      expect(mockStore.setActiveConversation).not.toHaveBeenCalled();
    });

    it("should not auto-select over an explicit conversation override outside history mode", async () => {
      const { result } = renderHook(
        (props) => useChatPanelContext(props),
        {
          wrapper,
          initialProps: {
            projectId: "project-1",
            ideationSessionId: undefined,
            selectedTaskId: undefined,
            isExecutionMode: false,
            isReviewMode: false,
            isMergeMode: false,
            isHistoryMode: false,
            overrideConversationId: "conv-archived",
          },
        }
      );

      await waitFor(() => {
        expect(mockStore.setActiveConversation).toHaveBeenCalledWith(
          "project:project-1",
          "conv-archived"
        );
      });

      mockStore.activeConversationIds["project:project-1"] = "conv-archived";
      mockStore.setActiveConversation.mockClear();

      const mockConversations: ConversationData[] = [
        {
          id: "conv-active",
          lastMessageAt: "2026-02-11T12:00:00Z",
          createdAt: "2026-02-11T11:00:00Z",
        },
      ];

      act(() => {
        result.current.autoSelectConversation({
          data: mockConversations,
          isLoading: false,
        });
      });

      expect(mockStore.setActiveConversation).not.toHaveBeenCalled();
    });
  });

  describe("isVisible re-trigger", () => {
    it("should reset hasAutoSelectedRef when panel transitions from hidden to visible", async () => {
      mockStore.activeConversationIds = {};

      const { result, rerender } = renderHook(
        (props) => useChatPanelContext(props),
        {
          wrapper,
          initialProps: {
            projectId: "project-1",
            ideationSessionId: "session-1",
            selectedTaskId: undefined,
            isExecutionMode: false,
            isReviewMode: false,
            isMergeMode: false,
            isHistoryMode: false,
            isVisible: true,
          },
        }
      );

      // First auto-select runs
      const mockConversations: ConversationData[] = [
        { id: "conv-parent", lastMessageAt: "2026-03-17T10:00:00Z", createdAt: "2026-03-17T09:00:00Z" },
      ];
      act(() => {
        result.current.autoSelectConversation({ data: mockConversations, isLoading: false });
      });
      expect(mockStore.setActiveConversation).toHaveBeenCalledWith("session:session-1", "conv-parent");
      mockStore.activeConversationIds["session:session-1"] = "conv-parent";
      mockStore.setActiveConversation.mockClear();

      // Panel becomes hidden (verification tab shown)
      rerender({
        projectId: "project-1",
        ideationSessionId: "session-1",
        selectedTaskId: undefined,
        isExecutionMode: false,
        isReviewMode: false,
        isMergeMode: false,
        isHistoryMode: false,
        isVisible: false,
      });

      // While hidden, activeConversationId for this context gets stomped
      mockStore.activeConversationIds["session:session-1"] = "conv-child";

      // Panel becomes visible again (Plan tab clicked)
      rerender({
        projectId: "project-1",
        ideationSessionId: "session-1",
        selectedTaskId: undefined,
        isExecutionMode: false,
        isReviewMode: false,
        isMergeMode: false,
        isHistoryMode: false,
        isVisible: true,
      });

      // autoSelectConversation should now re-fire (hasAutoSelectedRef was reset)
      // and re-select conv-parent because conv-child doesn't belong to this context
      act(() => {
        result.current.autoSelectConversation({ data: mockConversations, isLoading: false });
      });

      expect(mockStore.setActiveConversation).toHaveBeenCalledWith("session:session-1", "conv-parent");
    });

    it("should invalidate conversation list atomically with hasAutoSelectedRef reset on hidden→visible transition", async () => {
      const invalidateSpy = vi.spyOn(queryClient, "invalidateQueries");

      const { rerender } = renderHook(
        (props) => useChatPanelContext(props),
        {
          wrapper,
          initialProps: {
            projectId: "project-1",
            ideationSessionId: "session-1",
            selectedTaskId: undefined,
            isExecutionMode: false,
            isReviewMode: false,
            isMergeMode: false,
            isHistoryMode: false,
            isVisible: true,
          },
        }
      );

      // Wait for initial mount effect (prevIsVisibleRef starts false → isVisible true fires)
      await waitFor(() => {
        expect(invalidateSpy).toHaveBeenCalledWith({
          queryKey: ["conversations", "ideation", "session-1"],
        });
      });
      invalidateSpy.mockClear();

      // Panel becomes hidden
      rerender({
        projectId: "project-1",
        ideationSessionId: "session-1",
        selectedTaskId: undefined,
        isExecutionMode: false,
        isReviewMode: false,
        isMergeMode: false,
        isHistoryMode: false,
        isVisible: false,
      });

      // No invalidation on hidden transition
      expect(invalidateSpy).not.toHaveBeenCalled();

      // Panel becomes visible again
      rerender({
        projectId: "project-1",
        ideationSessionId: "session-1",
        selectedTaskId: undefined,
        isExecutionMode: false,
        isReviewMode: false,
        isMergeMode: false,
        isHistoryMode: false,
        isVisible: true,
      });

      // Should invalidate with the correct contextType and contextId
      await waitFor(() => {
        expect(invalidateSpy).toHaveBeenCalledWith({
          queryKey: ["conversations", "ideation", "session-1"],
        });
      });
    });

    it("should use updated contextType/contextId in invalidation after context change", async () => {
      const invalidateSpy = vi.spyOn(queryClient, "invalidateQueries");

      const { rerender } = renderHook(
        (props) => useChatPanelContext(props),
        {
          wrapper,
          initialProps: {
            projectId: "project-1",
            ideationSessionId: "session-1",
            selectedTaskId: undefined,
            isExecutionMode: false,
            isReviewMode: false,
            isMergeMode: false,
            isHistoryMode: false,
            isVisible: false,
          },
        }
      );

      // Context changes to a task while hidden
      rerender({
        projectId: "project-1",
        ideationSessionId: undefined,
        selectedTaskId: "task-42",
        isExecutionMode: true,
        isReviewMode: false,
        isMergeMode: false,
        isHistoryMode: false,
        isVisible: false,
      });

      invalidateSpy.mockClear();

      // Panel becomes visible — invalidation should use the NEW context (task_execution:task-42)
      rerender({
        projectId: "project-1",
        ideationSessionId: undefined,
        selectedTaskId: "task-42",
        isExecutionMode: true,
        isReviewMode: false,
        isMergeMode: false,
        isHistoryMode: false,
        isVisible: true,
      });

      await waitFor(() => {
        expect(invalidateSpy).toHaveBeenCalledWith({
          queryKey: ["conversations", "task_execution", "task-42"],
        });
      });

      // Must NOT have used the stale ideation context
      const staleCall = invalidateSpy.mock.calls.find(
        (call) => JSON.stringify(call[0]).includes("session-1")
      );
      expect(staleCall).toBeUndefined();
    });

    it("should NOT reset hasAutoSelectedRef when panel stays visible across renders", async () => {
      mockStore.activeConversationIds = {};

      const { result, rerender } = renderHook(
        (props) => useChatPanelContext(props),
        {
          wrapper,
          initialProps: {
            projectId: "project-1",
            ideationSessionId: "session-1",
            selectedTaskId: undefined,
            isExecutionMode: false,
            isReviewMode: false,
            isMergeMode: false,
            isHistoryMode: false,
            isVisible: true,
          },
        }
      );

      // First auto-select runs
      const mockConversations: ConversationData[] = [
        { id: "conv-1", lastMessageAt: "2026-03-17T10:00:00Z", createdAt: "2026-03-17T09:00:00Z" },
      ];
      act(() => {
        result.current.autoSelectConversation({ data: mockConversations, isLoading: false });
      });
      expect(mockStore.setActiveConversation).toHaveBeenCalledWith("session:session-1", "conv-1");
      mockStore.activeConversationIds["session:session-1"] = "conv-1";
      mockStore.setActiveConversation.mockClear();

      // Re-render with isVisible still true (no transition)
      rerender({
        projectId: "project-1",
        ideationSessionId: "session-1",
        selectedTaskId: undefined,
        isExecutionMode: false,
        isReviewMode: false,
        isMergeMode: false,
        isHistoryMode: false,
        isVisible: true,
      });

      // autoSelectConversation should NOT re-select (hasAutoSelectedRef still true)
      act(() => {
        result.current.autoSelectConversation({ data: mockConversations, isLoading: false });
      });

      // conv-1 already belongs to context, so no redundant setActiveConversation call
      expect(mockStore.setActiveConversation).not.toHaveBeenCalled();
    });
  });

});
