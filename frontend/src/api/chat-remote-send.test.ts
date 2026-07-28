/**
 * The remote half of `sendAgentMessage`.
 *
 * These tests exercise the real transport wrapper — `@tauri-apps/api/core` is aliased to
 * `lib/remote/invoke`, so a remote environment routes through `networkInvoke` and out via
 * the `remote_invoke` primitive. Mocking the API module would have proved nothing about
 * the routing, which is the only thing in question here.
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

import { sendAgentMessage } from "@/api/chat";
import { RemoteTransportError } from "@/lib/remote/transport-errors";
import { LOCAL_ENVIRONMENT_ID, useEnvironmentStore } from "@/stores/environmentStore";

const REMOTE_ID = "env-remote";
const CONVERSATION_ID = "conversation-1";

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

/** The `{input: {id, requestId, cmd, args}}` envelope of the Nth `remote_invoke`. */
function wireInput(callIndex = 0): Record<string, unknown> {
  const args = primitiveInvoke.mock.calls[callIndex]?.[1] as {
    input: Record<string, unknown>;
  };
  return args.input;
}

beforeEach(() => {
  primitiveInvoke.mockReset();
  useLocalEnvironment();
});

describe("remote send routing", () => {
  it("routes through NetworkInvoke as send_remote_chat_message, never send_agent_message", async () => {
    useRemoteEnvironment();
    primitiveInvoke.mockResolvedValue({
      outcome: "ok",
      result: {
        conversationId: CONVERSATION_ID,
        queuedMessageId: "queued-1",
        agentRunId: "run-1",
        createdAt: "2026-07-28T12:00:00Z",
      },
    });

    const result = await sendAgentMessage("project", "project-1", "ship it", undefined, {
      conversationId: CONVERSATION_ID,
    });

    expect(primitiveInvoke).toHaveBeenCalledTimes(1);
    expect(primitiveInvoke.mock.calls[0]?.[0]).toBe("remote_invoke");

    const input = wireInput();
    expect(input.id).toBe(REMOTE_ID);
    expect(input.cmd).toBe("send_remote_chat_message");
    // `send_agent_message` reaches a process-launch sink and is unregistered by ruling.
    // A regression that routed to it would answer REMOTE_COMMAND_UNAVAILABLE in the
    // field, which is exactly the bug this whole change removes.
    expect(input.cmd).not.toBe("send_agent_message");

    expect(input.args).toEqual({
      input: {
        conversationId: CONVERSATION_ID,
        content: "ship it",
        role: "user",
      },
    });
    expect(typeof input.requestId).toBe("string");
    expect((input.requestId as string).length).toBeGreaterThan(0);

    // The host queues for a live run rather than dispatching inline; the result says so.
    expect(result.wasQueued).toBe(true);
    expect(result.isNewConversation).toBe(false);
    expect(result.queuedMessageId).toBe("queued-1");
    expect(result.agentRunId).toBe("run-1");
  });

  it("mints a distinct requestId per send, so the dedup substrate cannot reject the second", async () => {
    useRemoteEnvironment();
    primitiveInvoke.mockResolvedValue({
      outcome: "ok",
      result: {
        conversationId: CONVERSATION_ID,
        queuedMessageId: "queued-1",
        agentRunId: "run-1",
        createdAt: "2026-07-28T12:00:00Z",
      },
    });

    await sendAgentMessage("project", "project-1", "one", undefined, {
      conversationId: CONVERSATION_ID,
    });
    await sendAgentMessage("project", "project-1", "two", undefined, {
      conversationId: CONVERSATION_ID,
    });

    expect(wireInput(0).requestId).not.toEqual(wireInput(1).requestId);
  });

  it("cannot forge a non-user role — the wire role is always user", async () => {
    useRemoteEnvironment();
    primitiveInvoke.mockResolvedValue({
      outcome: "ok",
      result: {
        conversationId: CONVERSATION_ID,
        queuedMessageId: "queued-1",
        agentRunId: "run-1",
        createdAt: "2026-07-28T12:00:00Z",
      },
    });

    await sendAgentMessage("project", "project-1", "hi", undefined, {
      conversationId: CONVERSATION_ID,
      // A caller reaching for a speaker label it should not control.
      suppressUserMessage: true,
    });

    const args = wireInput().args as { input: Record<string, unknown> };
    expect(args.input.role).toBe("user");
    // Belt and braces: the host pins this field anyway, so even a client that patched
    // the line above still cannot author an orchestrator turn.
    expect(Object.keys(args.input).sort()).toEqual([
      "content",
      "conversationId",
      "role",
    ]);
  });

  it("refuses to start a conversation remotely instead of silently creating one", async () => {
    useRemoteEnvironment();

    await expect(
      sendAgentMessage("project", "project-1", "hello", undefined, {}),
    ).rejects.toBeInstanceOf(RemoteTransportError);

    expect(primitiveInvoke).not.toHaveBeenCalled();
  });

  it("turns a host refusal code into copy a person can act on", async () => {
    useRemoteEnvironment();
    primitiveInvoke.mockResolvedValue({
      outcome: "commandError",
      error: "REMOTE_CHAT_SEND_NOT_STEERABLE",
    });

    await expect(
      sendAgentMessage("project", "project-1", "hi", undefined, {
        conversationId: CONVERSATION_ID,
      }),
    ).rejects.toThrow(/isn't running right now/);
  });

  it("leaves a RemoteTransportError intact so unknown-outcome reconcile still fires", async () => {
    useRemoteEnvironment();
    // A timeout is the unknown-outcome case: the composer must be able to tell it apart
    // from a plain failure, which it does by TYPE. Rewrapping it as an Error would make
    // a possibly-applied send look definitively failed.
    primitiveInvoke.mockRejectedValue(
      new RemoteTransportError({
        code: "REMOTE_TIMEOUT_UNKNOWN",
        message: "timed out",
        environmentId: REMOTE_ID,
        cmd: "send_remote_chat_message",
      }),
    );

    await expect(
      sendAgentMessage("project", "project-1", "hi", undefined, {
        conversationId: CONVERSATION_ID,
      }),
    ).rejects.toBeInstanceOf(RemoteTransportError);
  });

  it("passes an unrecognised error through rather than inventing prose for it", async () => {
    useRemoteEnvironment();
    primitiveInvoke.mockResolvedValue({
      outcome: "commandError",
      error: "something nobody mapped",
    });

    await expect(
      sendAgentMessage("project", "project-1", "hi", undefined, {
        conversationId: CONVERSATION_ID,
      }),
    ).rejects.toThrow(/something nobody mapped/);
  });

  it("local environment still uses send_agent_message", async () => {
    primitiveInvoke.mockResolvedValue({
      conversation_id: CONVERSATION_ID,
      agent_run_id: "run-1",
      is_new_conversation: false,
      was_queued: false,
      queued_as_pending: false,
    });

    await sendAgentMessage("project", "project-1", "ship it", undefined, {
      conversationId: CONVERSATION_ID,
    });

    expect(primitiveInvoke.mock.calls[0]?.[0]).toBe("send_agent_message");
  });
});
