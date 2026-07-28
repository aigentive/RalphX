/**
 * Tests for useAskUserQuestion hook
 *
 * Tests event listening for agent:ask_user_question events,
 * storing per-session question payloads in uiStore, and submitting/dismissing answers.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { renderHook, act } from "@testing-library/react";
import { useAskUserQuestion } from "./useAskUserQuestion";
import { useUiStore } from "@/stores/uiStore";
import { LOCAL_ENVIRONMENT_ID } from "@/stores/environmentStore";
import { requestPendingGateReconcile } from "@/lib/remote/pending-gate-reconcile";
import { RemoteTransportError } from "@/lib/remote/transport-errors";
import type { AskUserQuestionPayload, AskUserQuestionResponse } from "@/types/ask-user-question";

// Mock EventBus
const mockSubscribers = new Map<string, (payload: unknown) => void>();

vi.mock("@/providers/EventProvider", () => ({
  useEventBus: () => ({
    subscribe: (event: string, handler: (payload: unknown) => void) => {
      mockSubscribers.set(event, handler);
      return () => {
        mockSubscribers.delete(event);
      };
    },
  }),
}));

// Mock sonner toast
vi.mock("sonner", () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
    info: vi.fn(),
  },
}));

// Mock Tauri invoke for answering questions
const { mockInvoke } = vi.hoisted(() => ({ mockInvoke: vi.fn().mockResolvedValue(null) }));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => mockInvoke(...args),
}));

// Mock the api module
vi.mock("@/lib/tauri", () => ({
  api: {
    askUserQuestion: {
      resolveQuestion: vi.fn(),
      answerQuestion: vi.fn(),
      getPendingQuestions: vi.fn().mockResolvedValue([]),
      listPendingQuestionGates: vi.fn().mockResolvedValue([]),
    },
  },
}));

import { api } from "@/lib/tauri";
const mockResolve = vi.mocked(api.askUserQuestion.resolveQuestion);
const mockAnswer = vi.mocked(api.askUserQuestion.answerQuestion);
const mockGetPending = vi.mocked(api.askUserQuestion.getPendingQuestions);
const mockListGates = vi.mocked(api.askUserQuestion.listPendingQuestionGates);

// Helper to emit events
function emitEvent(eventName: string, payload: unknown) {
  const handler = mockSubscribers.get(eventName);
  if (handler) {
    handler(payload);
  }
}

const TEST_SESSION = "session-abc";

// Valid test payload
const validPayload: AskUserQuestionPayload = {
  requestId: "req-test-123",
  taskId: "task-123",
  sessionId: TEST_SESSION,
  question: "Which authentication method should we use?",
  header: "Auth method",
  options: [
    { label: "JWT tokens", description: "Recommended for stateless APIs" },
    { label: "Session cookies", description: "Traditional server-side sessions" },
  ],
  multiSelect: false,
};

describe("useAskUserQuestion", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.clearAllTimers();
    mockSubscribers.clear();
    mockResolve.mockResolvedValue({
      success: true,
      message: null,
      deliveredToWaitingAgent: true,
    });
    mockAnswer.mockResolvedValue(undefined);
    mockGetPending.mockResolvedValue([]);
    mockListGates.mockResolvedValue([]);
    // Reset store state
    useUiStore.setState({
      activeQuestions: {},
      answeredQuestions: {},
    });
    // Use fake timers for testing timeouts
    vi.useFakeTimers();
  });

  afterEach(() => {
    mockSubscribers.clear();
    vi.useRealTimers();
  });

  describe("listener registration", () => {
    it("should register agent:ask_user_question listener on mount", () => {
      renderHook(() => useAskUserQuestion(TEST_SESSION));
      expect(mockSubscribers.has("agent:ask_user_question")).toBe(true);
    });

    it("should unregister listener on unmount", () => {
      const { unmount } = renderHook(() => useAskUserQuestion(TEST_SESSION));
      expect(mockSubscribers.has("agent:ask_user_question")).toBe(true);

      unmount();
      expect(mockSubscribers.has("agent:ask_user_question")).toBe(false);
    });
  });

  describe("session scoping", () => {
    it("should store question payload keyed by sessionId", () => {
      renderHook(() => useAskUserQuestion(TEST_SESSION));

      act(() => {
        emitEvent("agent:ask_user_question", validPayload);
      });

      const state = useUiStore.getState();
      expect(state.activeQuestions[TEST_SESSION]).toEqual(validPayload);
    });

    it("should only return question for current session", () => {
      const { result } = renderHook(() => useAskUserQuestion(TEST_SESSION));

      // Store question for a DIFFERENT session
      act(() => {
        emitEvent("agent:ask_user_question", {
          ...validPayload,
          sessionId: "other-session",
        });
      });

      expect(result.current.activeQuestion).toBeNull();
    });

    it("should return question when sessionId matches", () => {
      const { result } = renderHook(() => useAskUserQuestion(TEST_SESSION));

      act(() => {
        emitEvent("agent:ask_user_question", validPayload);
      });

      expect(result.current.activeQuestion).toEqual(validPayload);
    });

    it("should return null when currentSessionId is undefined", () => {
      const { result } = renderHook(() => useAskUserQuestion(undefined));

      // Store a question
      act(() => {
        emitEvent("agent:ask_user_question", validPayload);
      });

      expect(result.current.activeQuestion).toBeNull();
    });

    it("should ignore events without sessionId", () => {
      const consoleSpy = vi.spyOn(console, "warn").mockImplementation(() => {});
      renderHook(() => useAskUserQuestion(TEST_SESSION));

      act(() => {
        emitEvent("agent:ask_user_question", {
          ...validPayload,
          sessionId: undefined,
        });
      });

      const state = useUiStore.getState();
      expect(Object.keys(state.activeQuestions)).toHaveLength(0);
      consoleSpy.mockRestore();
    });
  });

  describe("event handling", () => {
    it("should ignore invalid events with missing fields", () => {
      const consoleSpy = vi.spyOn(console, "warn").mockImplementation(() => {});
      renderHook(() => useAskUserQuestion(TEST_SESSION));

      const invalidPayload = { taskId: "task-123" }; // Missing required fields

      act(() => {
        emitEvent("agent:ask_user_question", invalidPayload);
      });

      const state = useUiStore.getState();
      expect(Object.keys(state.activeQuestions)).toHaveLength(0);
      consoleSpy.mockRestore();
    });
  });

  describe("return values", () => {
    it("should return activeQuestion from store for current session", async () => {
      act(() => {
        useUiStore.getState().setActiveQuestion(TEST_SESSION, validPayload);
      });
      mockGetPending.mockResolvedValueOnce([validPayload]);
      const { result } = renderHook(() => useAskUserQuestion(TEST_SESSION));

      await act(async () => {
        await Promise.resolve();
      });

      expect(result.current.activeQuestion).toEqual(validPayload);
    });

    it("should return null activeQuestion when no question is active", () => {
      const { result } = renderHook(() => useAskUserQuestion(TEST_SESSION));
      expect(result.current.activeQuestion).toBeNull();
    });

    it("should return submitAnswer function", () => {
      const { result } = renderHook(() => useAskUserQuestion(TEST_SESSION));
      expect(typeof result.current.submitAnswer).toBe("function");
    });

    it("should return dismissQuestion function", () => {
      const { result } = renderHook(() => useAskUserQuestion(TEST_SESSION));
      expect(typeof result.current.dismissQuestion).toBe("function");
    });

    it("should return isLoading state", () => {
      const { result } = renderHook(() => useAskUserQuestion(TEST_SESSION));
      expect(result.current.isLoading).toBe(false);
    });
  });

  describe("submitAnswer", () => {
    it("should call resolveQuestion when requestId is present (MCP flow)", async () => {
      useUiStore.getState().setActiveQuestion(TEST_SESSION, validPayload);
      const { result } = renderHook(() => useAskUserQuestion(TEST_SESSION));

      const response: AskUserQuestionResponse = {
        requestId: "req-test-123",
        taskId: "task-123",
        selectedOptions: ["JWT tokens"],
      };

      await act(async () => {
        await result.current.submitAnswer(response);
      });

      expect(mockResolve).toHaveBeenCalledWith({
        requestId: "req-test-123",
        selectedOptions: ["JWT tokens"],
      });
    });

    it("should call answerQuestion when no requestId (legacy flow)", async () => {
      useUiStore.getState().setActiveQuestion(TEST_SESSION, validPayload);
      const { result } = renderHook(() => useAskUserQuestion(TEST_SESSION));

      const response: AskUserQuestionResponse = {
        taskId: "task-123",
        selectedOptions: ["JWT tokens"],
      };

      await act(async () => {
        await result.current.submitAnswer(response);
      });

      expect(mockAnswer).toHaveBeenCalledWith(response);
    });

    it("should clear active question and set answered after successful submission", async () => {
      useUiStore.getState().setActiveQuestion(TEST_SESSION, validPayload);
      const { result } = renderHook(() => useAskUserQuestion(TEST_SESSION));

      const response: AskUserQuestionResponse = {
        requestId: "req-test-123",
        selectedOptions: ["JWT tokens"],
      };

      await act(async () => {
        await result.current.submitAnswer(response);
      });

      const state = useUiStore.getState();
      expect(state.activeQuestions[TEST_SESSION]).toBeUndefined();
      expect(state.answeredQuestions[TEST_SESSION]).toBe("JWT tokens");
    });

    it("should submit skipped answers and summarize them", async () => {
      useUiStore.getState().setActiveQuestion(TEST_SESSION, validPayload);
      const { result } = renderHook(() => useAskUserQuestion(TEST_SESSION));

      const response: AskUserQuestionResponse = {
        requestId: "req-test-123",
        selectedOptions: [],
        skipped: true,
      };

      await act(async () => {
        await result.current.submitAnswer(response);
      });

      expect(mockResolve).toHaveBeenCalledWith({
        requestId: "req-test-123",
        selectedOptions: [],
        skipped: true,
      });
      expect(useUiStore.getState().answeredQuestions[TEST_SESSION]).toBe("Skipped");
    });

    it("should not call api if no active question", async () => {
      const { result } = renderHook(() => useAskUserQuestion(TEST_SESSION));

      const response: AskUserQuestionResponse = {
        taskId: "task-123",
        selectedOptions: ["JWT tokens"],
      };

      await act(async () => {
        await result.current.submitAnswer(response);
      });

      expect(mockResolve).not.toHaveBeenCalled();
      expect(mockAnswer).not.toHaveBeenCalled();
    });
  });

  describe("dismissQuestion", () => {
    it("should clear both question and answered state for session", async () => {
      useUiStore.getState().setActiveQuestion(TEST_SESSION, validPayload);
      useUiStore.getState().setAnsweredQuestion(TEST_SESSION, "prev answer");

      const { result } = renderHook(() => useAskUserQuestion(TEST_SESSION));

      await act(async () => {
        await result.current.dismissQuestion();
      });

      const state = useUiStore.getState();
      expect(state.activeQuestions[TEST_SESSION]).toBeUndefined();
      expect(state.answeredQuestions[TEST_SESSION]).toBeUndefined();
    });

    it("should send dismiss to backend when question has requestId", async () => {
      useUiStore.getState().setActiveQuestion(TEST_SESSION, validPayload);
      const { result } = renderHook(() => useAskUserQuestion(TEST_SESSION));

      await act(async () => {
        await result.current.dismissQuestion();
      });

      expect(mockResolve).toHaveBeenCalledWith({
        requestId: "req-test-123",
        selectedOptions: [],
        customResponse: "[dismissed]",
      });
    });

    it("should not send to backend when no active question", async () => {
      const { result } = renderHook(() => useAskUserQuestion(TEST_SESSION));

      await act(async () => {
        await result.current.dismissQuestion();
      });

      expect(mockResolve).not.toHaveBeenCalled();
    });
  });

  describe("clearAnswered", () => {
    it("should clear answered summary for session", () => {
      useUiStore.getState().setAnsweredQuestion(TEST_SESSION, "some answer");
      const { result } = renderHook(() => useAskUserQuestion(TEST_SESSION));

      expect(result.current.answeredQuestion).toBe("some answer");

      act(() => {
        result.current.clearAnswered();
      });

      expect(result.current.answeredQuestion).toBeUndefined();
    });
  });

  describe("answeredQuestion", () => {
    it("should return answered summary for current session", () => {
      useUiStore.getState().setAnsweredQuestion(TEST_SESSION, "JWT tokens");
      const { result } = renderHook(() => useAskUserQuestion(TEST_SESSION));
      expect(result.current.answeredQuestion).toBe("JWT tokens");
    });

    it("should not return answered summary from other sessions", () => {
      useUiStore.getState().setAnsweredQuestion("other-session", "other answer");
      const { result } = renderHook(() => useAskUserQuestion(TEST_SESSION));
      expect(result.current.answeredQuestion).toBeUndefined();
    });
  });

  describe("hydration from pending backend state", () => {
    it("should display question missed while panel was unmounted", async () => {
      const missedQuestion: AskUserQuestionPayload = {
        requestId: "req-missed-event",
        sessionId: TEST_SESSION,
        question: "Which approach should we use?",
        options: [{ label: "Alpha" }, { label: "Beta" }],
        multiSelect: false,
      };
      mockGetPending.mockResolvedValueOnce([missedQuestion]);

      const { result } = renderHook(() => useAskUserQuestion(TEST_SESSION));

      await act(async () => {
        await Promise.resolve(); // flush microtasks
      });

      expect(result.current.activeQuestion).toEqual(missedQuestion);
    });

    it("should not hydrate when pending question belongs to different session", async () => {
      const otherSessionQuestion: AskUserQuestionPayload = {
        requestId: "req-other",
        sessionId: "other-session",
        question: "Other session question",
        options: [],
        multiSelect: false,
      };
      mockGetPending.mockResolvedValueOnce([otherSessionQuestion]);

      const { result } = renderHook(() => useAskUserQuestion(TEST_SESSION));

      await act(async () => {
        await Promise.resolve();
      });

      expect(result.current.activeQuestion).toBeNull();
    });

    it("should not hydrate when currentSessionId is undefined", async () => {
      const pendingQuestion: AskUserQuestionPayload = {
        requestId: "req-abc",
        sessionId: TEST_SESSION,
        question: "Some question",
        options: [],
        multiSelect: false,
      };
      mockGetPending.mockResolvedValueOnce([pendingQuestion]);

      renderHook(() => useAskUserQuestion(undefined));

      await act(async () => {
        await Promise.resolve();
      });

      expect(mockGetPending).not.toHaveBeenCalled();
    });

    it("should call getPendingQuestions once on mount", async () => {
      renderHook(() => useAskUserQuestion(TEST_SESSION));

      await act(async () => {
        await Promise.resolve();
      });

      expect(mockGetPending).toHaveBeenCalledTimes(1);
    });

    it("should not override event-delivered question with empty backend response", async () => {
      const { result } = renderHook(() => useAskUserQuestion(TEST_SESSION));

      // Event arrives first
      act(() => {
        emitEvent("agent:ask_user_question", validPayload);
      });

      // Backend returns empty (question already answered/removed)
      mockGetPending.mockResolvedValueOnce([]);

      await act(async () => {
        await Promise.resolve();
      });

      // The event-delivered question should still be active
      expect(result.current.activeQuestion).toEqual(validPayload);
    });
  });

  describe("multiple sessions", () => {
    it("should store questions for different sessions independently", () => {
      renderHook(() => useAskUserQuestion(TEST_SESSION));

      const payload1 = { ...validPayload, sessionId: "session-1", requestId: "req-1" };
      const payload2 = { ...validPayload, sessionId: "session-2", requestId: "req-2" };

      act(() => {
        emitEvent("agent:ask_user_question", payload1);
        emitEvent("agent:ask_user_question", payload2);
      });

      const state = useUiStore.getState();
      expect(state.activeQuestions["session-1"]?.requestId).toBe("req-1");
      expect(state.activeQuestions["session-2"]?.requestId).toBe("req-2");
    });
  });

  describe("question lifecycle events", () => {
    it("keeps the active question when a matching question_expired event arrives", async () => {
      const activeQuestion = { ...validPayload, requestId: "req-expire-active" };
      useUiStore.getState().setActiveQuestion(TEST_SESSION, activeQuestion);
      mockGetPending.mockResolvedValueOnce([activeQuestion]);
      renderHook(() => useAskUserQuestion(TEST_SESSION));

      await act(async () => {
        await Promise.resolve();
      });

      act(() => {
        emitEvent("agent:question_expired", {
          sessionId: TEST_SESSION,
          requestId: activeQuestion.requestId,
        });
      });

      expect(useUiStore.getState().activeQuestions[TEST_SESSION]).toEqual(activeQuestion);
    });

    it("does not clear the active question when question_expired is for another request", async () => {
      const activeQuestion = { ...validPayload, requestId: "req-expire-other" };
      useUiStore.getState().setActiveQuestion(TEST_SESSION, activeQuestion);
      mockGetPending.mockResolvedValueOnce([activeQuestion]);
      renderHook(() => useAskUserQuestion(TEST_SESSION));

      await act(async () => {
        await Promise.resolve();
      });

      act(() => {
        emitEvent("agent:question_expired", {
          sessionId: TEST_SESSION,
          requestId: "req-other",
        });
      });

      expect(useUiStore.getState().activeQuestions[TEST_SESSION]).toEqual(activeQuestion);
    });

    it("rehydrates a question after its agent wait has expired", async () => {
      const expiredQuestion: AskUserQuestionPayload = {
        requestId: "req-expired-1",
        sessionId: TEST_SESSION,
        question: "Expired question",
        options: [],
        multiSelect: false,
      };

      const { unmount } = renderHook(() => useAskUserQuestion(TEST_SESSION));

      await act(async () => {
        await Promise.resolve();
      });

      act(() => {
        emitEvent("agent:question_expired", {
          sessionId: TEST_SESSION,
          requestId: expiredQuestion.requestId,
        });
      });

      unmount();
      useUiStore.setState({
        activeQuestions: {},
        answeredQuestions: {},
      });
      mockGetPending.mockResolvedValueOnce([expiredQuestion]);

      const { result } = renderHook(() => useAskUserQuestion(TEST_SESSION));

      await act(async () => {
        await Promise.resolve();
      });

      expect(result.current.activeQuestion).toEqual(expiredQuestion);
    });
  });

  describe("auto-dismiss timer", () => {
    it("should automatically clear answered question after 3500ms delay", async () => {
      useUiStore.getState().setActiveQuestion(TEST_SESSION, validPayload);
      const { result } = renderHook(() => useAskUserQuestion(TEST_SESSION));

      const response: AskUserQuestionResponse = {
        requestId: "req-test-123",
        selectedOptions: ["JWT tokens"],
      };

      await act(async () => {
        await result.current.submitAnswer(response);
      });

      // Answered question should be set
      expect(useUiStore.getState().answeredQuestions[TEST_SESSION]).toBe("JWT tokens");

      // Advance time by 3500ms
      act(() => {
        vi.advanceTimersByTime(3500);
      });

      // Answered question should be cleared
      expect(useUiStore.getState().answeredQuestions[TEST_SESSION]).toBeUndefined();
    });

    it("should cancel auto-dismiss timer and clear answered state when new question arrives for same session", async () => {
      useUiStore.getState().setActiveQuestion(TEST_SESSION, validPayload);
      const { result } = renderHook(() => useAskUserQuestion(TEST_SESSION));

      const response: AskUserQuestionResponse = {
        requestId: "req-test-123",
        selectedOptions: ["JWT tokens"],
      };

      await act(async () => {
        await result.current.submitAnswer(response);
      });

      // Answered question should be set
      expect(useUiStore.getState().answeredQuestions[TEST_SESSION]).toBe("JWT tokens");

      // Advance time by 1000ms (before auto-dismiss triggers)
      act(() => {
        vi.advanceTimersByTime(1000);
      });

      // New question arrives for same session — should clear stale answered state
      act(() => {
        emitEvent("agent:ask_user_question", {
          ...validPayload,
          requestId: "req-test-456",
        });
      });

      // Answered state should be cleared immediately (not hidden behind old banner)
      expect(useUiStore.getState().answeredQuestions[TEST_SESSION]).toBeUndefined();

      // New question should be active
      expect(result.current.activeQuestion?.requestId).toBe("req-test-456");

      // Advance to what would have been the original timeout
      act(() => {
        vi.advanceTimersByTime(2500);
      });

      // Answered should still be cleared (timer was cancelled, won't fire)
      expect(useUiStore.getState().answeredQuestions[TEST_SESSION]).toBeUndefined();
    });

    it("should cancel auto-dismiss timer when user manually dismisses", async () => {
      useUiStore.getState().setActiveQuestion(TEST_SESSION, validPayload);
      useUiStore.getState().setAnsweredQuestion(TEST_SESSION, "some answer");
      const { result } = renderHook(() => useAskUserQuestion(TEST_SESSION));

      // Dismiss the question
      await act(async () => {
        await result.current.dismissQuestion();
      });

      // Answered question should be cleared by dismissQuestion
      expect(useUiStore.getState().answeredQuestions[TEST_SESSION]).toBeUndefined();

      // Advance timer to verify it won't try to clear again
      act(() => {
        vi.advanceTimersByTime(3500);
      });

      // Should still be undefined (no errors or double-clear)
      expect(useUiStore.getState().answeredQuestions[TEST_SESSION]).toBeUndefined();
    });

    it("should cancel auto-dismiss timer on component unmount", async () => {
      useUiStore.getState().setActiveQuestion(TEST_SESSION, validPayload);
      const { result, unmount } = renderHook(() => useAskUserQuestion(TEST_SESSION));

      const response: AskUserQuestionResponse = {
        requestId: "req-test-123",
        selectedOptions: ["JWT tokens"],
      };

      await act(async () => {
        await result.current.submitAnswer(response);
      });

      // Answered question should be set
      expect(useUiStore.getState().answeredQuestions[TEST_SESSION]).toBe("JWT tokens");

      // Unmount component (cleanup runs)
      unmount();

      // Advance past the timeout that should have been cancelled
      act(() => {
        vi.advanceTimersByTime(3500);
      });

      // Answered question should still be set (timer was cleaned up)
      expect(useUiStore.getState().answeredQuestions[TEST_SESSION]).toBe("JWT tokens");
    });

    it("should not start timer if submitAnswer fails", async () => {
      useUiStore.getState().setActiveQuestion(TEST_SESSION, validPayload);
      const { result } = renderHook(() => useAskUserQuestion(TEST_SESSION));

      mockResolve.mockRejectedValueOnce(new Error("API error"));

      const response: AskUserQuestionResponse = {
        requestId: "req-test-123",
        selectedOptions: ["JWT tokens"],
      };

      await act(async () => {
        await result.current.submitAnswer(response);
      });

      // Should not have set answered question due to error
      expect(useUiStore.getState().answeredQuestions[TEST_SESSION]).toBeUndefined();

      // Advance time
      act(() => {
        vi.advanceTimersByTime(3500);
      });

      // Should still be undefined
      expect(useUiStore.getState().answeredQuestions[TEST_SESSION]).toBeUndefined();
    });
  });

  describe("submitAnswer return value and error handling", () => {
    it("returns delivery metadata on successful submission", async () => {
      useUiStore.getState().setActiveQuestion(TEST_SESSION, validPayload);
      const { result } = renderHook(() => useAskUserQuestion(TEST_SESSION));

      const response: AskUserQuestionResponse = {
        requestId: "req-test-123",
        selectedOptions: ["JWT tokens"],
      };

      let returnValue: Awaited<ReturnType<typeof result.current.submitAnswer>> | undefined;
      await act(async () => {
        returnValue = await result.current.submitAnswer(response);
      });

      expect(returnValue).toEqual({
        success: true,
        deliveredToWaitingAgent: true,
        planModeProposalHandled: false,
      });
    });

    it("returns unsuccessful delivery metadata when no active question", async () => {
      // Don't set an active question
      const { result } = renderHook(() => useAskUserQuestion(TEST_SESSION));

      const response: AskUserQuestionResponse = {
        requestId: "req-test-123",
        selectedOptions: ["JWT tokens"],
      };

      let returnValue: Awaited<ReturnType<typeof result.current.submitAnswer>> | undefined;
      await act(async () => {
        returnValue = await result.current.submitAnswer(response);
      });

      expect(returnValue).toEqual({
        success: false,
        deliveredToWaitingAgent: false,
      });
    });

    it("returns unsuccessful delivery metadata when API call fails", async () => {
      useUiStore.getState().setActiveQuestion(TEST_SESSION, validPayload);
      const { result } = renderHook(() => useAskUserQuestion(TEST_SESSION));

      mockResolve.mockRejectedValueOnce(new Error("Session expired"));

      const response: AskUserQuestionResponse = {
        requestId: "req-test-123",
        selectedOptions: ["JWT tokens"],
      };

      let returnValue: Awaited<ReturnType<typeof result.current.submitAnswer>> | undefined;
      await act(async () => {
        returnValue = await result.current.submitAnswer(response);
      });

      expect(returnValue).toEqual({
        success: false,
        deliveredToWaitingAgent: false,
      });
    });

    it("clears stale active question on API failure", async () => {
      useUiStore.getState().setActiveQuestion(TEST_SESSION, validPayload);
      const { result } = renderHook(() => useAskUserQuestion(TEST_SESSION));

      mockResolve.mockRejectedValueOnce(new Error("Session expired"));

      const response: AskUserQuestionResponse = {
        requestId: "req-test-123",
        selectedOptions: ["JWT tokens"],
      };

      await act(async () => {
        await result.current.submitAnswer(response);
      });

      // Stale question should be cleaned up on error
      expect(useUiStore.getState().activeQuestions[TEST_SESSION]).toBeUndefined();
    });

    it("does not set answered state on API failure", async () => {
      useUiStore.getState().setActiveQuestion(TEST_SESSION, validPayload);
      const { result } = renderHook(() => useAskUserQuestion(TEST_SESSION));

      mockResolve.mockRejectedValueOnce(new Error("Session expired"));

      const response: AskUserQuestionResponse = {
        requestId: "req-test-123",
        selectedOptions: ["JWT tokens"],
      };

      await act(async () => {
        await result.current.submitAnswer(response);
      });

      expect(useUiStore.getState().answeredQuestions[TEST_SESSION]).toBeUndefined();
    });

    it("resets isLoading after API failure", async () => {
      useUiStore.getState().setActiveQuestion(TEST_SESSION, validPayload);
      const { result } = renderHook(() => useAskUserQuestion(TEST_SESSION));

      mockResolve.mockRejectedValueOnce(new Error("Session expired"));

      const response: AskUserQuestionResponse = {
        requestId: "req-test-123",
        selectedOptions: ["JWT tokens"],
      };

      await act(async () => {
        await result.current.submitAnswer(response);
      });

      expect(result.current.isLoading).toBe(false);
    });

    it("does not wipe a new question when old submit completes", async () => {
      // Set up original question
      useUiStore.getState().setActiveQuestion(TEST_SESSION, validPayload);
      const { result } = renderHook(() => useAskUserQuestion(TEST_SESSION));

      // Make resolveQuestion slow — simulate in-flight API call
      let resolveApi!: () => void;
      mockResolve.mockImplementationOnce(() => new Promise<void>((r) => { resolveApi = r; }));

      const response: AskUserQuestionResponse = {
        requestId: "req-test-123",
        selectedOptions: ["JWT tokens"],
      };

      // Start the submit (won't complete yet)
      let submitPromise: Promise<boolean>;
      act(() => {
        submitPromise = result.current.submitAnswer(response);
      });

      // While submit is in-flight, a NEW question arrives for the same session
      const newQuestion: AskUserQuestionPayload = {
        requestId: "req-NEW-456",
        taskId: "task-123",
        sessionId: TEST_SESSION,
        question: "Pick a strategy",
        header: "Strategy",
        options: [{ label: "A", description: "Option A" }],
        multiSelect: false,
      };
      act(() => {
        useUiStore.getState().setActiveQuestion(TEST_SESSION, newQuestion);
      });

      // Now the old API call completes successfully
      await act(async () => {
        resolveApi();
        await submitPromise!;
      });

      // The NEW question must still be in the store — old submit must NOT wipe it
      const currentQuestion = useUiStore.getState().activeQuestions[TEST_SESSION];
      expect(currentQuestion).toBeDefined();
      expect(currentQuestion?.requestId).toBe("req-NEW-456");
    });

    it("does not show error toast when question already cleared by agent death", async () => {
      const { toast: toastMock } = await import("sonner");

      act(() => {
        useUiStore.getState().setActiveQuestion(TEST_SESSION, validPayload);
      });
      mockGetPending.mockResolvedValueOnce([validPayload]);
      const { result } = renderHook(() => useAskUserQuestion(TEST_SESSION));

      mockResolve.mockRejectedValueOnce(new Error("Session expired"));

      // Agent death clears the question before submit's catch runs
      act(() => {
        useUiStore.getState().clearActiveQuestion(TEST_SESSION);
      });

      const response: AskUserQuestionResponse = {
        requestId: "req-test-123",
        selectedOptions: ["JWT tokens"],
      };

      await act(async () => {
        await result.current.submitAnswer(response);
      });

      // Toast should NOT fire — question was already cleaned up by agent death
      expect(toastMock.error).not.toHaveBeenCalled();
    });
  });

  describe("stale question cleanup on mount and focus", () => {
    it("clears stale question on mount when backend has no pending question", async () => {
      // Pre-populate store with a question the backend no longer knows about
      useUiStore.getState().setActiveQuestion(TEST_SESSION, validPayload);
      mockGetPending.mockResolvedValueOnce([]);

      renderHook(() => useAskUserQuestion(TEST_SESSION));

      await act(async () => {
        await Promise.resolve(); // flush microtasks
      });

      expect(useUiStore.getState().activeQuestions[TEST_SESSION]).toBeUndefined();
    });

    it("preserves question on mount when backend confirms it is still pending", async () => {
      useUiStore.getState().setActiveQuestion(TEST_SESSION, validPayload);
      mockGetPending.mockResolvedValueOnce([validPayload]);

      renderHook(() => useAskUserQuestion(TEST_SESSION));

      await act(async () => {
        await Promise.resolve();
      });

      expect(useUiStore.getState().activeQuestions[TEST_SESSION]).toEqual(validPayload);
    });

    it("does not clear event-delivered question when mount hydration resolves after event", async () => {
      // Pre-populate with old question
      const oldQuestion = { ...validPayload, requestId: "old-req" };
      useUiStore.getState().setActiveQuestion(TEST_SESSION, oldQuestion);

      // Deferred promise for getPendingQuestions — we control when it resolves
      let resolveGetPending!: (value: AskUserQuestionPayload[]) => void;
      mockGetPending.mockImplementationOnce(
        () => new Promise<AskUserQuestionPayload[]>((r) => { resolveGetPending = r; })
      );

      const { result } = renderHook(() => useAskUserQuestion(TEST_SESSION));

      // While API is in-flight, a NEW question arrives via event
      const newQuestion = { ...validPayload, requestId: "new-req" };
      act(() => {
        emitEvent("agent:ask_user_question", newQuestion);
      });

      expect(result.current.activeQuestion?.requestId).toBe("new-req");

      // Now the old API call resolves with empty (backend has nothing pending)
      await act(async () => {
        resolveGetPending([]);
        await Promise.resolve();
      });

      // The new event-delivered question must survive — requestId changed, so cleanup skips
      expect(useUiStore.getState().activeQuestions[TEST_SESSION]).toBeDefined();
      expect(useUiStore.getState().activeQuestions[TEST_SESSION]?.requestId).toBe("new-req");
    });

    it("clears stale question on visibilitychange when backend has no pending question", async () => {
      const { result } = renderHook(() => useAskUserQuestion(TEST_SESSION));

      // Wait for mount hydration to complete
      await act(async () => {
        await Promise.resolve();
      });

      // Now set an active question (simulating one that arrived while app was foregrounded)
      act(() => {
        useUiStore.getState().setActiveQuestion(TEST_SESSION, validPayload);
      });
      expect(result.current.activeQuestion).toEqual(validPayload);

      // Backend will report no pending questions
      mockGetPending.mockResolvedValueOnce([]);

      // Simulate returning to the app
      Object.defineProperty(document, "visibilityState", { value: "visible", writable: true, configurable: true });
      act(() => {
        document.dispatchEvent(new Event("visibilitychange"));
      });

      // Advance past debounce
      act(() => {
        vi.advanceTimersByTime(500);
      });

      // Flush the promise from getPendingQuestions
      await act(async () => {
        await Promise.resolve();
        await Promise.resolve(); // extra flush for chained .then()
      });

      expect(useUiStore.getState().activeQuestions[TEST_SESSION]).toBeUndefined();
    });

    it("does not check backend on visibilitychange when no active question", async () => {
      renderHook(() => useAskUserQuestion(TEST_SESSION));

      // Wait for mount hydration
      await act(async () => {
        await Promise.resolve();
      });

      // Reset call count after mount
      mockGetPending.mockClear();

      // Dispatch visibilitychange with no active question
      Object.defineProperty(document, "visibilityState", { value: "visible", writable: true, configurable: true });
      act(() => {
        document.dispatchEvent(new Event("visibilitychange"));
      });

      act(() => {
        vi.advanceTimersByTime(500);
      });

      await act(async () => {
        await Promise.resolve();
      });

      // Should NOT have called getPendingQuestions — no question to check
      expect(mockGetPending).not.toHaveBeenCalled();
    });

    it("debounces rapid visibilitychange events", async () => {
      renderHook(() => useAskUserQuestion(TEST_SESSION));

      // Wait for mount hydration
      await act(async () => {
        await Promise.resolve();
      });

      // Set active question
      act(() => {
        useUiStore.getState().setActiveQuestion(TEST_SESSION, validPayload);
      });

      // Reset after mount
      mockGetPending.mockClear();
      mockGetPending.mockResolvedValue([validPayload]); // keep question alive

      Object.defineProperty(document, "visibilityState", { value: "visible", writable: true, configurable: true });

      // Dispatch 3 rapid visibilitychange events
      act(() => {
        document.dispatchEvent(new Event("visibilitychange"));
      });
      act(() => {
        document.dispatchEvent(new Event("visibilitychange"));
      });
      act(() => {
        document.dispatchEvent(new Event("visibilitychange"));
      });

      // Advance past debounce
      act(() => {
        vi.advanceTimersByTime(500);
      });

      await act(async () => {
        await Promise.resolve();
      });

      // Only one call — debounce collapsed the 3 events
      expect(mockGetPending).toHaveBeenCalledTimes(1);
    });

    it("does not clear question when document becomes hidden", async () => {
      renderHook(() => useAskUserQuestion(TEST_SESSION));

      // Wait for mount hydration
      await act(async () => {
        await Promise.resolve();
      });

      // Set active question
      act(() => {
        useUiStore.getState().setActiveQuestion(TEST_SESSION, validPayload);
      });

      mockGetPending.mockClear();

      // Document becomes hidden
      Object.defineProperty(document, "visibilityState", { value: "hidden", writable: true, configurable: true });
      act(() => {
        document.dispatchEvent(new Event("visibilitychange"));
      });

      act(() => {
        vi.advanceTimersByTime(500);
      });

      await act(async () => {
        await Promise.resolve();
      });

      // Question should still be present — no cleanup on hidden
      expect(useUiStore.getState().activeQuestions[TEST_SESSION]).toEqual(validPayload);
      // getPendingQuestions should not have been called
      expect(mockGetPending).not.toHaveBeenCalled();
    });
  });

  // ==========================================================================
  // P-21: authoritative gate reconciliation on (re)connect (PR 2.7-c)
  // ==========================================================================

  describe("pending-gate reconciliation", () => {
    async function reconcile(environmentId = LOCAL_ENVIRONMENT_ID) {
      await act(async () => {
        requestPendingGateReconcile(environmentId);
        await Promise.resolve();
        await Promise.resolve();
      });
    }

    it("adopts a question raised while this client was disconnected", async () => {
      // A fresh requestId: the module-level answered-guard is deliberately long-lived,
      // and reusing an id another test already answered would be filtered by design.
      const raisedOffline = { ...validPayload, requestId: "req-raised-offline" };
      renderHook(() => useAskUserQuestion(TEST_SESSION));
      expect(useUiStore.getState().activeQuestions[TEST_SESSION]).toBeUndefined();

      mockListGates.mockResolvedValue([raisedOffline]);
      await reconcile();

      expect(useUiStore.getState().activeQuestions[TEST_SESSION]).toEqual(
        raisedOffline
      );
    });

    it("drops a question the host no longer lists", async () => {
      renderHook(() => useAskUserQuestion(TEST_SESSION));
      act(() => {
        useUiStore.getState().setActiveQuestion(TEST_SESSION, validPayload);
      });

      mockListGates.mockResolvedValue([]);
      await reconcile();

      expect(useUiStore.getState().activeQuestions[TEST_SESSION]).toBeUndefined();
    });

    it("FAILS CLOSED on a read error: the banner stays up", async () => {
      renderHook(() => useAskUserQuestion(TEST_SESSION));
      act(() => {
        useUiStore.getState().setActiveQuestion(TEST_SESSION, validPayload);
      });

      mockListGates.mockRejectedValue(new Error("question state unreadable"));
      await reconcile();

      // Clearing a live gate on unreadable state leaves an agent waiting forever on an
      // answer nobody can see it needs.
      expect(useUiStore.getState().activeQuestions[TEST_SESSION]).toEqual(
        validPayload
      );
    });

    it("stays silent when the host does not expose the gate list yet", async () => {
      const { toast } = await import("sonner");
      renderHook(() => useAskUserQuestion(TEST_SESSION));
      act(() => {
        useUiStore.getState().setActiveQuestion(TEST_SESSION, validPayload);
      });

      mockListGates.mockRejectedValue(
        new RemoteTransportError({
          code: "REMOTE_COMMAND_UNAVAILABLE",
          message: "not registered on this host",
          environmentId: LOCAL_ENVIRONMENT_ID,
        })
      );
      await reconcile();

      expect(useUiStore.getState().activeQuestions[TEST_SESSION]).toEqual(
        validPayload
      );
      expect(vi.mocked(toast.error)).not.toHaveBeenCalled();
    });

    it("ignores a BACKGROUND environment's connect (SCOPE negative)", async () => {
      renderHook(() => useAskUserQuestion(TEST_SESSION));
      act(() => {
        useUiStore.getState().setActiveQuestion(TEST_SESSION, validPayload);
      });

      mockListGates.mockResolvedValue([]);
      await reconcile("some-other-environment");

      expect(mockListGates).not.toHaveBeenCalled();
      expect(useUiStore.getState().activeQuestions[TEST_SESSION]).toEqual(
        validPayload
      );
    });
  });
});
