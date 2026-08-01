/**
 * The remote half of `startAgentConversation` (spawn-free conversation start, contract §3.1).
 *
 * Like `chat-remote-send.test.ts`, these tests exercise the REAL transport wrapper —
 * `@tauri-apps/api/core` is aliased to `lib/remote/invoke`, so a remote environment routes
 * through `networkInvoke` and out via the `remote_invoke` primitive. Mocking the API module
 * would prove nothing about the routing, which is the only thing in question here.
 *
 * What is pinned: a paired device NEVER calls `start_agent_conversation` (it fires the
 * process-spawn detectors and stays unregistered by ruling). It persists a start-intent
 * through `request_remote_agent_conversation_start`, polls
 * `get_remote_conversation_start_request` to a terminal state, and — on `started` — navigates
 * into the host-seeded conversation through the spawn-free `get_remote_agent_conversation`
 * read. No local seeded-create runs. `refreshRuntime`, base/branch, persona, team intent, and
 * first-turn role never cross the wire.
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

import {
  RemoteConversationStartError,
  startAgentConversation,
  type StartAgentConversationInput,
} from "@/api/chat";
import { LOCAL_ENVIRONMENT_ID, useEnvironmentStore } from "@/stores/environmentStore";

const REMOTE_ID = "env-remote";
const CONVERSATION_ID = "conversation-1";
const PROJECT_ID = "project-1";
const START_REQUEST_ID = "start-req-1";
const NOW = "2026-08-01T12:00:00Z";

const RAW_CONVERSATION = {
  id: CONVERSATION_ID,
  context_type: "project",
  context_id: PROJECT_ID,
  claude_session_id: null,
  title: "Ship it",
  message_count: 1,
  last_message_at: NOW,
  created_at: NOW,
  updated_at: NOW,
} as const;

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

/** The facade command name of the Nth `remote_invoke`. */
function wireCmd(callIndex = 0): string {
  return wireInput(callIndex).cmd as string;
}

/**
 * Routes the remote transport by facade command, so the start → poll → navigate sequence
 * can be scripted end to end. `status` picks the terminal poll outcome.
 */
function routeRemote(
  status: string,
  errorCode: string | null = null,
): void {
  primitiveInvoke.mockImplementation(async (_command, payload) => {
    const cmd = (payload as { input: { cmd: string } }).input.cmd;
    switch (cmd) {
      case "request_remote_agent_conversation_start":
        return {
          outcome: "ok",
          result: {
            startRequestId: START_REQUEST_ID,
            conversationId: CONVERSATION_ID,
            status: "pending",
            createdAt: NOW,
          },
        };
      case "get_remote_conversation_start_request":
        return {
          outcome: "ok",
          result: {
            id: START_REQUEST_ID,
            conversationId: CONVERSATION_ID,
            status,
            errorCode,
            agentRunId: status === "started" ? "run-1" : null,
            createdAt: NOW,
            updatedAt: NOW,
          },
        };
      case "get_remote_agent_conversation":
        return { outcome: "ok", result: { conversation: RAW_CONVERSATION, messages: [] } };
      default:
        throw new Error(`unexpected remote command ${cmd}`);
    }
  });
}

beforeEach(() => {
  primitiveInvoke.mockReset();
  useLocalEnvironment();
});

describe("remote conversation-start routing", () => {
  it("persists a start-intent, polls it, and navigates into the seeded conversation", async () => {
    useRemoteEnvironment();
    routeRemote("started");

    const result = await startAgentConversation({
      projectId: PROJECT_ID,
      content: "ship it",
      providerHarness: "codex",
      modelId: "gpt-5.6-sol",
      logicalEffort: "high",
    });

    // Three remote calls, in order: start-intent → status poll → transcript navigate.
    expect(primitiveInvoke.mock.calls.every((call) => call[0] === "remote_invoke")).toBe(
      true,
    );
    expect(wireCmd(0)).toBe("request_remote_agent_conversation_start");
    // The process-spawn command is never named on a paired device.
    expect(wireCmd(0)).not.toBe("start_agent_conversation");
    expect(wireCmd(1)).toBe("get_remote_conversation_start_request");
    expect(wireCmd(2)).toBe("get_remote_agent_conversation");

    // Only client-settable fields cross; `mode` is pinned `chat`.
    expect(wireInput(0).args).toEqual({
      input: {
        projectId: PROJECT_ID,
        content: "ship it",
        provider: "codex",
        modelOverride: "gpt-5.6-sol",
        logicalEffort: "high",
        mode: "chat",
      },
    });
    expect(wireInput(1).args).toEqual({ startRequestId: START_REQUEST_ID });

    expect(result.conversation.id).toBe(CONVERSATION_ID);
    expect(result.sendResult.agentRunId).toBe("run-1");
    expect(result.sendResult.wasQueued).toBe(false);
  });

  it("never forwards refreshRuntime, base/branch, persona, or team intent", async () => {
    useRemoteEnvironment();
    routeRemote("started");

    await startAgentConversation({
      projectId: PROJECT_ID,
      content: "hello",
      providerHarness: "claude",
      modelId: "sonnet",
      logicalEffort: "medium",
      // Fields the host forces or forbids — none may appear on the wire.
      personaId: "persona-9",
      teamIntent: {
        coordinationMode: "team",
      } as StartAgentConversationInput["teamIntent"],
      base: {
        kind: "branch",
        ref: "main",
        displayName: "main",
      } as StartAgentConversationInput["base"],
      mode: "edit",
    });

    const sent = (wireInput(0).args as { input: Record<string, unknown> }).input;
    expect(Object.keys(sent).sort()).toEqual([
      "content",
      "logicalEffort",
      "mode",
      "modelOverride",
      "projectId",
      "provider",
    ]);
    // The client cannot smuggle a non-chat mode past the pin, even by naming one.
    expect(sent.mode).toBe("chat");
  });

  it("surfaces a non-started terminal status as a typed error with the host error code", async () => {
    useRemoteEnvironment();
    routeRemote("failed", "REMOTE_CONV_START_PROVIDER_NOT_ENABLED");

    const error = await startAgentConversation({
      projectId: PROJECT_ID,
      content: "go",
    }).catch((err: unknown) => err);

    expect(error).toBeInstanceOf(RemoteConversationStartError);
    expect((error as RemoteConversationStartError).status).toBe("failed");
    expect((error as RemoteConversationStartError).errorCode).toBe(
      "REMOTE_CONV_START_PROVIDER_NOT_ENABLED",
    );
    // A failed start never navigates into a conversation.
    expect(
      primitiveInvoke.mock.calls.some(
        (_call, index) => wireCmd(index) === "get_remote_agent_conversation",
      ),
    ).toBe(false);
  });

  it("classifies a stale host claim (failedStale) as an error the user can retry", async () => {
    useRemoteEnvironment();
    routeRemote("failedStale", "REMOTE_CONV_START_STALE");

    const error = await startAgentConversation({
      projectId: PROJECT_ID,
      content: "go",
    }).catch((err: unknown) => err);

    expect(error).toBeInstanceOf(RemoteConversationStartError);
    expect((error as RemoteConversationStartError).status).toBe("failedStale");
  });

  it("refuses to start remotely without a project rather than sending a dangling scope", async () => {
    useRemoteEnvironment();
    routeRemote("started");

    await expect(
      startAgentConversation({ projectId: null, content: "go" }),
    ).rejects.toThrow(/project is required/i);
    expect(primitiveInvoke).not.toHaveBeenCalled();
  });

  it("local environment still starts inline through start_agent_conversation", async () => {
    // A rejection stops before the response transform; we only assert the routed command.
    primitiveInvoke.mockRejectedValue(new Error("stop after routing"));

    await expect(
      startAgentConversation({ projectId: PROJECT_ID, content: "ship it" }),
    ).rejects.toThrow();

    expect(primitiveInvoke.mock.calls[0]?.[0]).toBe("start_agent_conversation");
  });
});
