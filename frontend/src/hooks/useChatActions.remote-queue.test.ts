/**
 * Phase 1 — the queued-message brakes fail CLOSED under a remote environment.
 *
 * Two bugs, one shape. Delete mutated the local store first and swallowed the host failure,
 * so a turn the user watched vanish was still queued on the host and still delivered. Edit
 * did the same and then sent the rewritten content UNCONDITIONALLY, so the agent received
 * both the original queued turn and the edit.
 *
 * The assertions here are ABSENCE assertions on the effects — `deleteQueuedMessage` not
 * called, `sendAgentMessage` not called — because a test that only checked for a toast would
 * pass against a version that still dropped the chip and still double-sent. The local column
 * is asserted byte-identical: the optimistic order is correct over local IPC and this phase
 * does not change it.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { renderHook, act } from "@testing-library/react";
import { useChatActions } from "./useChatActions";
import {
  resetTransportEnvironmentId,
  setTransportEnvironmentId,
} from "@/lib/remote/active-environment";
import { RemoteTransportError } from "@/lib/remote/transport-errors";

const mockToastError = vi.fn();
const mockToastWarning = vi.fn();
vi.mock("sonner", () => ({
  toast: {
    error: (...args: unknown[]) => mockToastError(...args),
    warning: (...args: unknown[]) => mockToastWarning(...args),
  },
}));

vi.mock("@tanstack/react-query", () => ({
  useQueryClient: () => ({ invalidateQueries: vi.fn(), setQueryData: vi.fn() }),
}));

const mockActions = {
  queueMessage: vi.fn(),
  deleteQueuedMessage: vi.fn(),
  startEditingQueuedMessage: vi.fn(),
  setActiveConversation: vi.fn(),
  setAgentRunning: vi.fn(),
  setSending: vi.fn(),
};
vi.mock("@/stores/chatStore", () => ({
  useChatStore: (selector: (state: typeof mockActions) => unknown) => selector(mockActions),
}));

const mockSendAgentMessage = vi.fn();
const mockDeleteQueuedAgentMessage = vi.fn();
const mockSendQueuedAgentMessageNow = vi.fn();
vi.mock("@/api/chat", () => ({
  chatApi: {
    sendAgentMessage: (...args: unknown[]) => mockSendAgentMessage(...args),
    deleteQueuedAgentMessage: (...args: unknown[]) =>
      mockDeleteQueuedAgentMessage(...args),
    sendQueuedAgentMessageNow: (...args: unknown[]) =>
      mockSendQueuedAgentMessageNow(...args),
  },
  stopAgent: vi.fn(),
}));

vi.mock("@/api/recovery", () => ({ recoverTaskExecution: vi.fn() }));
vi.mock("@/api/ideation", () => ({
  ideationApi: { sessions: { spawnSessionNamer: vi.fn() } },
}));
vi.mock("@/hooks/useChat", () => ({
  chatKeys: {
    all: ["chat"] as const,
    conversations: () => ["chat", "conversations"] as const,
    conversation: (id: string) => ["chat", "conversations", id] as const,
    conversationHistory: (id: string) =>
      ["chat", "conversations", id, "history"] as const,
    conversationList: (ct: string, ci: string) =>
      ["chat", "conversations", ct, ci] as const,
  },
  invalidateConversationDataQueries: vi.fn(),
  addOptimisticUserMessageToConversationCache: vi.fn(() => ({ id: "opt-1" })),
  removeOptimisticMessageFromConversationCache: vi.fn(),
}));

const REMOTE_ID = "env-remote";
const MESSAGE_ID = "queued-1";
const STORE_KEY = "task:task-1";

function setup() {
  const { result } = renderHook(() =>
    useChatActions({
      contextType: "task",
      contextId: "task-1",
      storeContextKey: STORE_KEY,
      sendMessage: { isPending: false, mutateAsync: vi.fn() },
      messageCount: 5,
    }),
  );
  return result;
}

function hostRefusal(): RemoteTransportError {
  return new RemoteTransportError({
    code: "REMOTE_COMMAND_UNAVAILABLE",
    message: "not registered on this host",
    environmentId: REMOTE_ID,
    cmd: "delete_queued_agent_message",
  });
}

beforeEach(() => {
  vi.clearAllMocks();
  mockDeleteQueuedAgentMessage.mockResolvedValue(undefined);
  mockSendAgentMessage.mockResolvedValue({
    conversationId: "conv-1",
    agentRunId: "run-1",
    isNewConversation: false,
    wasQueued: false,
    queuedAsPending: false,
  });
  resetTransportEnvironmentId();
});

afterEach(() => {
  resetTransportEnvironmentId();
});

describe("remote delete — host first, local state follows", () => {
  it("keeps the chip when the host refuses the delete", async () => {
    setTransportEnvironmentId(REMOTE_ID);
    mockDeleteQueuedAgentMessage.mockRejectedValueOnce(hostRefusal());
    const result = setup();

    await act(async () => {
      await result.current.handleDeleteQueuedMessage(MESSAGE_ID);
    });

    // THE assertion: the turn is still queued on the host, so it is still on screen.
    expect(mockActions.deleteQueuedMessage).not.toHaveBeenCalled();
    expect(mockToastError).toHaveBeenCalled();
  });

  it("calls the host BEFORE touching local state", async () => {
    setTransportEnvironmentId(REMOTE_ID);
    const order: string[] = [];
    mockDeleteQueuedAgentMessage.mockImplementationOnce(async () => {
      order.push("host");
    });
    mockActions.deleteQueuedMessage.mockImplementationOnce(() => {
      order.push("local");
    });
    const result = setup();

    await act(async () => {
      await result.current.handleDeleteQueuedMessage(MESSAGE_ID);
    });

    expect(order).toEqual(["host", "local"]);
  });

  it("surfaces the gate's own copy for a typed transport code", async () => {
    setTransportEnvironmentId(REMOTE_ID);
    mockDeleteQueuedAgentMessage.mockRejectedValueOnce(hostRefusal());
    const result = setup();

    await act(async () => {
      await result.current.handleDeleteQueuedMessage(MESSAGE_ID);
    });

    expect(mockToastError).toHaveBeenCalledWith(
      "Unavailable on this host",
      expect.objectContaining({
        description: "This action runs only on the host — it is not available remotely.",
      }),
    );
  });

  it("drops the chip once the host confirms", async () => {
    setTransportEnvironmentId(REMOTE_ID);
    const result = setup();

    await act(async () => {
      await result.current.handleDeleteQueuedMessage(MESSAGE_ID);
    });

    expect(mockActions.deleteQueuedMessage).toHaveBeenCalledWith(STORE_KEY, MESSAGE_ID);
    expect(mockToastError).not.toHaveBeenCalled();
  });
});

describe("remote edit — a failed delete must not become a second turn", () => {
  it("does not send the rewrite when the host kept the original queued", async () => {
    setTransportEnvironmentId(REMOTE_ID);
    mockDeleteQueuedAgentMessage.mockRejectedValueOnce(hostRefusal());
    const result = setup();

    await act(async () => {
      await result.current.handleEditQueuedMessage(MESSAGE_ID, "rewritten");
    });

    // The double-turn assertion: the original is still queued on the host, so sending the
    // rewrite would put BOTH in front of the agent.
    expect(mockSendAgentMessage).not.toHaveBeenCalled();
    expect(mockActions.deleteQueuedMessage).not.toHaveBeenCalled();
    expect(mockToastError).toHaveBeenCalled();
  });

  it("leaves the composer's sending flag untouched on an aborted edit", async () => {
    setTransportEnvironmentId(REMOTE_ID);
    mockDeleteQueuedAgentMessage.mockRejectedValueOnce(hostRefusal());
    const result = setup();

    await act(async () => {
      await result.current.handleEditQueuedMessage(MESSAGE_ID, "rewritten");
    });

    // The edit never started, so nothing should have flipped the panel into "sending".
    expect(mockActions.setSending).not.toHaveBeenCalled();
  });

  it("sends exactly once when the host accepted the delete", async () => {
    setTransportEnvironmentId(REMOTE_ID);
    const result = setup();

    await act(async () => {
      await result.current.handleEditQueuedMessage(MESSAGE_ID, "rewritten");
    });

    expect(mockDeleteQueuedAgentMessage).toHaveBeenCalledTimes(1);
    expect(mockActions.deleteQueuedMessage).toHaveBeenCalledWith(STORE_KEY, MESSAGE_ID);
    expect(mockSendAgentMessage).toHaveBeenCalledTimes(1);
  });
});

describe("local environment is byte-identical", () => {
  it("still deletes optimistically and still swallows the host failure", async () => {
    mockDeleteQueuedAgentMessage.mockRejectedValueOnce(new Error("boom"));
    const result = setup();

    await act(async () => {
      await result.current.handleDeleteQueuedMessage(MESSAGE_ID);
    });

    expect(mockActions.deleteQueuedMessage).toHaveBeenCalledWith(STORE_KEY, MESSAGE_ID);
    expect(mockToastError).not.toHaveBeenCalled();
  });

  it("still touches local state before the host on delete", async () => {
    const order: string[] = [];
    mockDeleteQueuedAgentMessage.mockImplementationOnce(async () => {
      order.push("host");
    });
    mockActions.deleteQueuedMessage.mockImplementationOnce(() => {
      order.push("local");
    });
    const result = setup();

    await act(async () => {
      await result.current.handleDeleteQueuedMessage(MESSAGE_ID);
    });

    expect(order).toEqual(["local", "host"]);
  });

  it("still sends the edit after a failed delete", async () => {
    mockDeleteQueuedAgentMessage.mockRejectedValueOnce(new Error("boom"));
    const result = setup();

    await act(async () => {
      await result.current.handleEditQueuedMessage(MESSAGE_ID, "rewritten");
    });

    expect(mockActions.deleteQueuedMessage).toHaveBeenCalledWith(STORE_KEY, MESSAGE_ID);
    expect(mockSendAgentMessage).toHaveBeenCalledTimes(1);
  });
});
