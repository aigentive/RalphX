/**
 * The IDLE half of the remote send (WP1 — conversation continuation).
 *
 * Before this change the remote surface was ONE-SHOT: once the agent finished its turn,
 * `send_remote_chat_message` answered `REMOTE_CHAT_SEND_NOT_STEERABLE` and the composer showed
 * a dead end. The send now branches on that refusal into a continuation intent the host
 * dispatcher delivers.
 *
 * What is pinned here, in the order it matters:
 *
 * 1. The BRANCH is driven by the HOST's refusal, never by client-side liveness inference — a
 *    client that guessed would either double a turn or strand one.
 * 2. The intent is POLLED to a terminal state before anything is reported as sent. Returning at
 *    persist time would show a sent message whose delivery is unproven (design doc §7).
 * 3. Every non-`dispatched` terminal state becomes a VISIBLE failure, not a ghost message.
 * 4. UX-5: the composer's model/effort selections TRAVEL instead of being silently dropped.
 *
 * Like the sibling remote tests, these exercise the REAL transport wrapper — mocking the API
 * module would prove nothing about the routing, which is the thing in question.
 */

import { beforeEach, describe, expect, it, vi } from "vitest";

vi.unmock("@tauri-apps/api/core");

const { primitiveInvoke } = vi.hoisted(() => ({
  primitiveInvoke: vi.fn<(cmd: string, args?: unknown) => Promise<unknown>>(),
}));

vi.mock("#tauri-core-primitive", async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>();
  return { ...actual, invoke: primitiveInvoke };
});

import { RemoteConversationMessageError, sendAgentMessage } from "@/api/chat";
import { RemoteTransportError } from "@/lib/remote/transport-errors";
import { LOCAL_ENVIRONMENT_ID, useEnvironmentStore } from "@/stores/environmentStore";

const REMOTE_ID = "env-remote";
const CONVERSATION_ID = "conversation-1";
const PROJECT_ID = "project-1";
const MESSAGE_REQUEST_ID = "message-req-1";
const NOW = "2026-08-01T13:00:00Z";

function useRemoteEnvironment(): void {
  useEnvironmentStore.setState({
    activeEnvironmentId: REMOTE_ID,
    environments: [
      { id: LOCAL_ENVIRONMENT_ID, name: "This Mac", kind: "local" },
      { id: REMOTE_ID, name: "Studio Mac", kind: "remote" },
    ],
    connectionPresentations: {
      [REMOTE_ID]: {
        presentation: "connected",
        blockedFailure: null,
        blockedMessage: null,
      },
    },
    effectiveScopes: { [REMOTE_ID]: ["ui:read", "ui:operate", "ui:agent"] },
  });
}

function useLocalEnvironment(): void {
  useEnvironmentStore.setState({
    activeEnvironmentId: LOCAL_ENVIRONMENT_ID,
    environments: [{ id: LOCAL_ENVIRONMENT_ID, name: "This Mac", kind: "local" }],
    effectiveScopes: {},
    connectionPresentations: {},
  });
}

/** The `{id, requestId, cmd, args}` envelope of the Nth `remote_invoke`. */
function wireInput(callIndex = 0): Record<string, unknown> {
  const args = primitiveInvoke.mock.calls[callIndex]?.[1] as {
    input: Record<string, unknown>;
  };
  return args.input;
}

function wireCmd(callIndex = 0): string {
  return wireInput(callIndex).cmd as string;
}

/**
 * Routes the remote transport by facade command. The live send always refuses as
 * NOT_STEERABLE (the idle case), and `status`/`errorCode` pick the terminal poll outcome.
 * `pendingPolls` inserts non-terminal reads first, so the poll loop itself is exercised.
 */
function routeIdleHost(
  status: string,
  errorCode: string | null = null,
  pendingPolls = 0,
): void {
  let polls = 0;
  primitiveInvoke.mockImplementation(async (_command, payload) => {
    const cmd = (payload as { input: { cmd: string } }).input.cmd;
    switch (cmd) {
      case "send_remote_chat_message":
        return { outcome: "commandError", error: "REMOTE_CHAT_SEND_NOT_STEERABLE" };
      case "request_remote_agent_conversation_message":
        return {
          outcome: "ok",
          result: {
            messageRequestId: MESSAGE_REQUEST_ID,
            conversationId: CONVERSATION_ID,
            status: "pending",
            createdAt: NOW,
          },
        };
      case "get_remote_conversation_message_request": {
        const settled = polls >= pendingPolls;
        polls += 1;
        return {
          outcome: "ok",
          result: {
            id: MESSAGE_REQUEST_ID,
            conversationId: CONVERSATION_ID,
            status: settled ? status : "dispatching",
            errorCode: settled ? errorCode : null,
            agentRunId: settled && status === "dispatched" ? "run-7" : null,
            createdAt: NOW,
            updatedAt: NOW,
          },
        };
      }
      default:
        throw new Error(`unexpected remote command ${cmd}`);
    }
  });
}

beforeEach(() => {
  primitiveInvoke.mockReset();
  vi.useRealTimers();
  useLocalEnvironment();
});

describe("remote conversation-continuation routing", () => {
  it("falls back to the continuation intent when the host says no run is live", async () => {
    useRemoteEnvironment();
    routeIdleHost("dispatched");

    const result = await sendAgentMessage(
      "project",
      PROJECT_ID,
      "keep going",
      undefined,
      { conversationId: CONVERSATION_ID },
    );

    // Three remote calls, in order: live attempt → intent → status poll.
    expect(
      primitiveInvoke.mock.calls.every((call) => call[0] === "remote_invoke"),
    ).toBe(true);
    expect(wireCmd(0)).toBe("send_remote_chat_message");
    expect(wireCmd(1)).toBe("request_remote_agent_conversation_message");
    expect(wireCmd(2)).toBe("get_remote_conversation_message_request");
    // The process-spawn commands are never named on a paired device.
    expect(primitiveInvoke.mock.calls.map((_, i) => wireCmd(i))).not.toContain(
      "send_agent_message",
    );

    expect(wireInput(2).args).toEqual({ messageRequestId: MESSAGE_REQUEST_ID });

    // Delivery is proven, not assumed: the run id comes off the terminal poll.
    expect(result.agentRunId).toBe("run-7");
    expect(result.conversationId).toBe(CONVERSATION_ID);
    expect(result.isNewConversation).toBe(false);
    expect(result.wasQueued).toBe(false);
  });

  it("carries the composer's model and effort instead of dropping them (UX-5)", async () => {
    useRemoteEnvironment();
    routeIdleHost("dispatched");

    await sendAgentMessage("project", PROJECT_ID, "keep going", undefined, {
      conversationId: CONVERSATION_ID,
      modelId: "sonnet",
      logicalEffort: "high",
    });

    expect(wireInput(1).args).toEqual({
      input: {
        conversationId: CONVERSATION_ID,
        projectId: PROJECT_ID,
        content: "keep going",
        modelOverride: "sonnet",
        logicalEffort: "high",
      },
    });
  });

  it("sends no role field at all — authorship is host-forced by absence", async () => {
    useRemoteEnvironment();
    routeIdleHost("dispatched");

    await sendAgentMessage("project", PROJECT_ID, "hi", undefined, {
      conversationId: CONVERSATION_ID,
      // A caller reaching for a speaker label it should not control.
      suppressUserMessage: true,
    });

    const args = wireInput(1).args as { input: Record<string, unknown> };
    expect(Object.keys(args.input).sort()).toEqual([
      "content",
      "conversationId",
      "projectId",
    ]);
    expect(args.input).not.toHaveProperty("role");
    expect(args.input).not.toHaveProperty("suppressUserMessage");
  });

  it("keeps polling while the intent is non-terminal", async () => {
    useRemoteEnvironment();
    routeIdleHost("dispatched", null, 2);

    const result = await sendAgentMessage(
      "project",
      PROJECT_ID,
      "hi",
      undefined,
      { conversationId: CONVERSATION_ID },
    );

    // live attempt + intent + two `dispatching` reads + the terminal read.
    expect(primitiveInvoke).toHaveBeenCalledTimes(5);
    expect(result.agentRunId).toBe("run-7");
  });

  // The §7 hazard, one case per terminal failure state: a message that no agent ever saw must
  // become a visible failure, never a ghost "sent" bubble.
  it.each([
    ["failed", "REMOTE_CONV_MESSAGE_HOST_SEND_FAILED", /never delivered/],
    ["failed", "REMOTE_CONV_MESSAGE_RUN_WENT_LIVE", /Send it again/],
    ["failedStale", null, /never sent/],
    ["cancelled", null, /never delivered/],
  ])(
    "surfaces terminal %s (%s) as a visible failure",
    async (status, errorCode, copy) => {
      useRemoteEnvironment();
      routeIdleHost(status, errorCode as string | null);

      const failure = await sendAgentMessage(
        "project",
        PROJECT_ID,
        "hi",
        undefined,
        { conversationId: CONVERSATION_ID },
      ).then(
        () => null,
        (error: unknown) => error,
      );

      expect(failure).toBeInstanceOf(RemoteConversationMessageError);
      const typed = failure as RemoteConversationMessageError;
      expect(typed.status).toBe(status);
      expect(typed.errorCode).toBe(errorCode);
      expect(typed.message).toMatch(copy as RegExp);
    },
  );

  it("turns a synchronous continuation refusal into copy a person can act on", async () => {
    useRemoteEnvironment();
    primitiveInvoke.mockImplementation(async (_command, payload) => {
      const cmd = (payload as { input: { cmd: string } }).input.cmd;
      if (cmd === "send_remote_chat_message") {
        return { outcome: "commandError", error: "REMOTE_CHAT_SEND_NOT_STEERABLE" };
      }
      return {
        outcome: "commandError",
        error: "REMOTE_CONV_MESSAGE_MODEL_NOT_ENABLED",
      };
    });

    await expect(
      sendAgentMessage("project", PROJECT_ID, "hi", undefined, {
        conversationId: CONVERSATION_ID,
      }),
    ).rejects.toThrow(/not enabled on the host/);
  });

  /**
   * The one branch that must NOT fall through. An unknown outcome may already have queued the
   * turn into the live run; continuing to the idle path would risk delivering it twice.
   */
  it("does not fall through to the continuation path on an unknown outcome", async () => {
    useRemoteEnvironment();
    primitiveInvoke.mockRejectedValue(
      new RemoteTransportError({
        code: "REMOTE_TIMEOUT_UNKNOWN",
        message: "timed out",
        environmentId: REMOTE_ID,
        cmd: "send_remote_chat_message",
      }),
    );

    await expect(
      sendAgentMessage("project", PROJECT_ID, "hi", undefined, {
        conversationId: CONVERSATION_ID,
      }),
    ).rejects.toBeInstanceOf(RemoteTransportError);

    // Exactly one call: the live attempt. No continuation intent was persisted.
    expect(primitiveInvoke).toHaveBeenCalledTimes(1);
  });

  it("still takes the live path when a run IS live, without touching the intent surface", async () => {
    useRemoteEnvironment();
    primitiveInvoke.mockResolvedValue({
      outcome: "ok",
      result: {
        conversationId: CONVERSATION_ID,
        queuedMessageId: "queued-1",
        agentRunId: "run-1",
        createdAt: NOW,
      },
    });

    const result = await sendAgentMessage(
      "project",
      PROJECT_ID,
      "hi",
      undefined,
      { conversationId: CONVERSATION_ID },
    );

    expect(primitiveInvoke).toHaveBeenCalledTimes(1);
    expect(wireCmd(0)).toBe("send_remote_chat_message");
    expect(result.wasQueued).toBe(true);
  });
});
