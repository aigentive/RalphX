/**
 * Phase 1 — no false-terminal writes on the question gate.
 *
 * The bug this pins: a submit that failed for TRANSPORT reasons rendered
 * "Agent session expired — question is no longer active" and cleared the banner, while the
 * host agent was still blocked on the answer. The client had learned nothing about the gate
 * — `RemoteTransportError` is raised by the wrapper itself, and a registered command that
 * ran on the host and returned `Err` rejects with that `Err`, never with this type — so the
 * clear was a terminal write authorized by no evidence at all. `dismissQuestion` had the same
 * shape with a worse blast radius: it recorded the requestId in the module-level answered-set
 * BEFORE the call, so a refusal suppressed every rehydration and reconcile of a live gate for
 * five minutes.
 *
 * These are ABSENCE assertions on the terminal write, not on the toast: a test that only
 * checked the copy would pass against a version that still deleted the banner silently.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { renderHook, act } from "@testing-library/react";
import { useAskUserQuestion } from "./useAskUserQuestion";
import { useUiStore } from "@/stores/uiStore";
import { LOCAL_ENVIRONMENT_ID } from "@/stores/environmentStore";
import { RemoteTransportError } from "@/lib/remote/transport-errors";
import type { AskUserQuestionPayload } from "@/types/ask-user-question";

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

vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn(), info: vi.fn() },
}));

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
import { toast } from "sonner";

const mockResolve = vi.mocked(api.askUserQuestion.resolveQuestion);
const mockGetPending = vi.mocked(api.askUserQuestion.getPendingQuestions);
const mockListGates = vi.mocked(api.askUserQuestion.listPendingQuestionGates);
const mockToastError = vi.mocked(toast.error);

const SESSION = "session-remote-1";
const REQUEST_ID = "req-remote-1";

/**
 * A distinct requestId per test. `answeredRequestIds` is module-level with a 5-minute TTL
 * and deliberately survives remounts, so reusing one id would let a suppression written by
 * an earlier test decide a later one.
 */
let requestSeq = 0;
function nextQuestion(): AskUserQuestionPayload {
  requestSeq += 1;
  return {
    requestId: `${REQUEST_ID}-${requestSeq}`,
    sessionId: SESSION,
    question: "Which auth method?",
    header: "Auth method",
    options: [{ label: "JWT", value: "jwt" }],
    multiSelect: false,
  };
}

function transportRefusal(): RemoteTransportError {
  return new RemoteTransportError({
    code: "REMOTE_COMMAND_UNAVAILABLE",
    message: "not registered on this host",
    environmentId: "env-remote",
    cmd: "resolve_user_question",
  });
}

function emit(payload: unknown): void {
  mockSubscribers.get("agent:ask_user_question")?.(payload);
}

function expiredToastFired(): boolean {
  return mockToastError.mock.calls.some((call) =>
    String(call[0]).includes("Agent session expired"),
  );
}

beforeEach(() => {
  vi.clearAllMocks();
  mockSubscribers.clear();
  mockResolve.mockResolvedValue({
    success: true,
    message: null,
    deliveredToWaitingAgent: true,
  });
  mockGetPending.mockResolvedValue([]);
  mockListGates.mockResolvedValue([]);
  useUiStore.setState({ activeQuestions: {}, answeredQuestions: {} });
});

afterEach(() => {
  mockSubscribers.clear();
});

describe("submitAnswer — transport refusal is not authority over the gate", () => {
  it("keeps the banner up and never claims the session expired", async () => {
    const question = nextQuestion();
    const { result } = renderHook(() => useAskUserQuestion(SESSION));
    act(() => emit(question));

    mockResolve.mockRejectedValueOnce(transportRefusal());

    let outcome: { success: boolean } | undefined;
    await act(async () => {
      outcome = await result.current.submitAnswer({
        requestId: question.requestId,
        selectedOptions: ["jwt"],
      });
    });

    expect(outcome?.success).toBe(false);
    // THE assertion: the still-blocked agent's gate is still on screen.
    expect(useUiStore.getState().activeQuestions[SESSION]).toEqual(question);
    // And no answered summary was written over it.
    expect(useUiStore.getState().answeredQuestions[SESSION]).toBeUndefined();
    expect(expiredToastFired()).toBe(false);
    // The failure is still surfaced — silence would be its own bug.
    expect(mockToastError).toHaveBeenCalled();
  });

  it("uses the gate's own unavailable copy, not a per-surface phrasing", async () => {
    const question = nextQuestion();
    const { result } = renderHook(() => useAskUserQuestion(SESSION));
    act(() => emit(question));

    mockResolve.mockRejectedValueOnce(transportRefusal());
    await act(async () => {
      await result.current.submitAnswer({
        requestId: question.requestId,
        selectedOptions: ["jwt"],
      });
    });

    expect(mockToastError).toHaveBeenCalledWith(
      "Unavailable on this host",
      expect.objectContaining({
        description: "This action runs only on the host — it is not available remotely.",
      }),
    );
  });

  it("leaves retry possible — the requestId is not recorded as answered", async () => {
    const question = nextQuestion();
    const { result } = renderHook(() => useAskUserQuestion(SESSION));
    act(() => emit(question));

    mockResolve.mockRejectedValueOnce(transportRefusal());
    await act(async () => {
      await result.current.submitAnswer({
        requestId: question.requestId,
        selectedOptions: ["jwt"],
      });
    });

    // A second attempt still reaches the API rather than being short-circuited...
    mockResolve.mockResolvedValueOnce({
      success: true,
      message: null,
      deliveredToWaitingAgent: true,
    });
    await act(async () => {
      await result.current.submitAnswer({
        requestId: question.requestId,
        selectedOptions: ["jwt"],
      });
    });
    expect(mockResolve).toHaveBeenCalledTimes(2);
    // ...and the retry is the write that finally clears it.
    expect(useUiStore.getState().activeQuestions[SESSION]).toBeUndefined();
  });

  it("re-hydration can still restore the gate after a failed submit", async () => {
    const question = nextQuestion();
    const { result, unmount } = renderHook(() => useAskUserQuestion(SESSION));
    act(() => emit(question));

    mockResolve.mockRejectedValueOnce(transportRefusal());
    await act(async () => {
      await result.current.submitAnswer({
        requestId: question.requestId,
        selectedOptions: ["jwt"],
      });
    });
    unmount();
    useUiStore.setState({ activeQuestions: {}, answeredQuestions: {} });

    // The host still lists it: the answered-set guard must not suppress the rehydrate.
    mockGetPending.mockResolvedValue([question]);
    await act(async () => {
      renderHook(() => useAskUserQuestion(SESSION));
      await Promise.resolve();
    });

    expect(useUiStore.getState().activeQuestions[SESSION]).toEqual(question);
  });

  it("still treats a HOST-produced Err as authoritative and clears", async () => {
    const question = nextQuestion();
    const { result } = renderHook(() => useAskUserQuestion(SESSION));
    act(() => emit(question));

    // The host read its own question state to answer this — that IS evidence about the gate.
    mockResolve.mockRejectedValueOnce(
      new Error("Question request 'req' not found"),
    );
    await act(async () => {
      await result.current.submitAnswer({
        requestId: question.requestId,
        selectedOptions: ["jwt"],
      });
    });

    expect(useUiStore.getState().activeQuestions[SESSION]).toBeUndefined();
    expect(expiredToastFired()).toBe(true);
  });

  it("does not rewrite a banner a newer question already replaced", async () => {
    const question = nextQuestion();
    const replacement = nextQuestion();
    const { result } = renderHook(() => useAskUserQuestion(SESSION));
    act(() => emit(question));

    mockResolve.mockImplementationOnce(async () => {
      act(() => emit(replacement));
      throw transportRefusal();
    });
    await act(async () => {
      await result.current.submitAnswer({
        requestId: question.requestId,
        selectedOptions: ["jwt"],
      });
    });

    expect(useUiStore.getState().activeQuestions[SESSION]).toEqual(replacement);
    expect(mockToastError).not.toHaveBeenCalled();
  });
});

describe("dismissQuestion — a refused dismiss must not suppress the live gate", () => {
  it("puts the banner back and surfaces the failure", async () => {
    const question = nextQuestion();
    const { result } = renderHook(() => useAskUserQuestion(SESSION));
    act(() => emit(question));

    mockResolve.mockRejectedValueOnce(transportRefusal());
    await act(async () => {
      await result.current.dismissQuestion();
    });

    expect(useUiStore.getState().activeQuestions[SESSION]).toEqual(question);
    expect(mockToastError).toHaveBeenCalled();
  });

  it("does not poison the answered-set, so hydration can restore it", async () => {
    const question = nextQuestion();
    const { result, unmount } = renderHook(() => useAskUserQuestion(SESSION));
    act(() => emit(question));

    mockResolve.mockRejectedValueOnce(transportRefusal());
    await act(async () => {
      await result.current.dismissQuestion();
    });
    unmount();
    useUiStore.setState({ activeQuestions: {}, answeredQuestions: {} });

    mockGetPending.mockResolvedValue([question]);
    await act(async () => {
      renderHook(() => useAskUserQuestion(SESSION));
      await Promise.resolve();
    });

    expect(useUiStore.getState().activeQuestions[SESSION]).toEqual(question);
  });

  it("keeps the dismiss when the HOST accepted it", async () => {
    const question = nextQuestion();
    const { result, unmount } = renderHook(() => useAskUserQuestion(SESSION));
    act(() => emit(question));

    await act(async () => {
      await result.current.dismissQuestion();
    });
    expect(useUiStore.getState().activeQuestions[SESSION]).toBeUndefined();
    unmount();

    // A stale host listing must not resurrect a dismissal the host confirmed.
    mockGetPending.mockResolvedValue([question]);
    await act(async () => {
      renderHook(() => useAskUserQuestion(SESSION));
      await Promise.resolve();
    });
    expect(useUiStore.getState().activeQuestions[SESSION]).toBeUndefined();
  });

  it("keeps the dismiss when the HOST rejected it as already gone", async () => {
    const question = nextQuestion();
    const { result } = renderHook(() => useAskUserQuestion(SESSION));
    act(() => emit(question));

    mockResolve.mockRejectedValueOnce(
      new Error("Question request 'req' is already resolved"),
    );
    await act(async () => {
      await result.current.dismissQuestion();
    });

    expect(useUiStore.getState().activeQuestions[SESSION]).toBeUndefined();
  });
});

describe("the questionAnswer gate names an op the host actually serves", () => {
  it("resolves unavailable remotely — the answer path has no registered twin", async () => {
    const { AGENT_GATED_AFFORDANCES, REMOTE_FACADE_OPS, resolveAffordanceGate } =
      await import("@/lib/remote/agent-gate");

    // `answer_user_question` IS registered, and pointing the row at it was the bug: it takes a
    // non-optional `taskId` and performs a Blocked→Ready TASK transition, never signalling the
    // MCP long-poll keyed by `requestId` — and the question event
    // (`http_server/handlers/questions.rs`) carries no taskId to call it with. A gate pointed at
    // it renders ENABLED over a submit that cannot land.
    expect(AGENT_GATED_AFFORDANCES.questionAnswer).toBe("resolve_user_question");
    expect(REMOTE_FACADE_OPS["resolve_user_question"]).toBeUndefined();

    // Unavailable at every scope, including a fully granted one — absence, not scope.
    for (const scopes of [
      ["ui:read", "ui:operate"],
      ["ui:read", "ui:operate", "ui:agent"],
    ]) {
      expect(resolveAffordanceGate("questionAnswer", true, scopes).status).toBe(
        "unavailable",
      );
    }
    // Local is untouched.
    expect(
      resolveAffordanceGate("questionAnswer", false, null).status,
    ).toBe("enabled");
    expect(LOCAL_ENVIRONMENT_ID).toBeDefined();
  });
});
