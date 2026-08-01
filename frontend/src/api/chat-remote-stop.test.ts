/**
 * The remote half of `stopAgent` (spawn-free agent stop, WP2).
 *
 * Like `chat-remote-start.test.ts`, these tests exercise the REAL transport wrapper —
 * `@tauri-apps/api/core` is aliased to `lib/remote/invoke`, so a remote environment routes
 * through `networkInvoke` and out via the `remote_invoke` primitive. Mocking the API module
 * would prove nothing about the routing, which is the only thing in question here.
 *
 * What is pinned: a paired device NEVER calls `stop_agent` (it reaches
 * `Command::new(resolve_pkill_cli_path())` on the host and stays unregistered by the absolute
 * process floor). It persists a stop-intent through `request_remote_agent_stop`, polls
 * `get_remote_agent_stop_request` to a terminal state, treats `noLiveRun` as a BENIGN success,
 * and throws a typed error for every failure terminal so the caller can surface it.
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

import { RemoteAgentStopError, stopAgent } from "@/api/chat";
import { LOCAL_ENVIRONMENT_ID, useEnvironmentStore } from "@/stores/environmentStore";

const REMOTE_ID = "env-remote";
const CONVERSATION_ID = "conversation-1";
const STOP_REQUEST_ID = "stop-req-1";
const NOW = "2026-08-01T12:00:00Z";

function useRemoteEnvironment(scopes: string[] = ["ui:read", "ui:operate"]): void {
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
    effectiveScopes: { [REMOTE_ID]: scopes },
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

function wireInput(callIndex = 0): Record<string, unknown> {
  const args = primitiveInvoke.mock.calls[callIndex]?.[1] as {
    input: Record<string, unknown>;
  };
  return args.input;
}

function wireCmd(callIndex = 0): string {
  return wireInput(callIndex).cmd as string;
}

/** Scripts the request → poll sequence, with the poll's terminal state chosen per test. */
function routeRemote(
  terminalStatus: string,
  errorCode: string | null = null,
  initialStatus = "pending",
): void {
  primitiveInvoke.mockImplementation(async (_command, payload) => {
    const cmd = (payload as { input: { cmd: string } }).input.cmd;
    switch (cmd) {
      case "request_remote_agent_stop":
        return {
          outcome: "ok",
          result: {
            stopRequestId: STOP_REQUEST_ID,
            conversationId: CONVERSATION_ID,
            status: initialStatus,
            deduplicated: false,
            createdAt: NOW,
          },
        };
      case "get_remote_agent_stop_request":
        return {
          outcome: "ok",
          result: {
            id: STOP_REQUEST_ID,
            conversationId: CONVERSATION_ID,
            status: terminalStatus,
            errorCode,
            createdAt: NOW,
            updatedAt: NOW,
          },
        };
      default:
        throw new Error(`unexpected remote command ${cmd}`);
    }
  });
}

beforeEach(() => {
  primitiveInvoke.mockReset();
  useLocalEnvironment();
});

describe("remote agent-stop routing", () => {
  it("persists a stop-intent and polls it instead of naming stop_agent", async () => {
    useRemoteEnvironment();
    routeRemote("stopped");

    const stopped = await stopAgent("project", CONVERSATION_ID);

    expect(
      primitiveInvoke.mock.calls.every((call) => call[0] === "remote_invoke"),
    ).toBe(true);
    expect(wireCmd(0)).toBe("request_remote_agent_stop");
    // The process-terminating command is never named on a paired device.
    expect(
      primitiveInvoke.mock.calls.some(
        (_call, index) => wireCmd(index) === "stop_agent",
      ),
    ).toBe(false);
    expect(wireCmd(1)).toBe("get_remote_agent_stop_request");

    // The intent names a CONVERSATION and nothing else — no contextType, run id, or pid.
    expect(wireInput(0).args).toEqual({
      input: { conversationId: CONVERSATION_ID },
    });
    expect(wireInput(1).args).toEqual({ stopRequestId: STOP_REQUEST_ID });
    expect(stopped).toBe(true);
  });

  it("routes from the DEFAULT pairing — the brake needs no ui:agent grant", async () => {
    useRemoteEnvironment(["ui:read", "ui:operate"]);
    routeRemote("stopped");

    await expect(stopAgent("project", CONVERSATION_ID)).resolves.toBe(true);
    expect(wireCmd(0)).toBe("request_remote_agent_stop");
  });

  it("treats noLiveRun as a benign success, not a failure", async () => {
    useRemoteEnvironment();
    routeRemote("noLiveRun");

    // Same shape the local command returns for "nothing was running".
    await expect(stopAgent("project", CONVERSATION_ID)).resolves.toBe(false);
  });

  it("throws a typed error when the host could not stop the run", async () => {
    useRemoteEnvironment();
    routeRemote("failed", "REMOTE_AGENT_STOP_HOST_STOP_FAILED");

    const error = await stopAgent("project", CONVERSATION_ID).catch(
      (thrown: unknown) => thrown,
    );
    expect(error).toBeInstanceOf(RemoteAgentStopError);
    expect((error as RemoteAgentStopError).status).toBe("failed");
    expect((error as RemoteAgentStopError).errorCode).toBe(
      "REMOTE_AGENT_STOP_HOST_STOP_FAILED",
    );
  });

  it("surfaces a stale-lease terminal rather than resolving silently", async () => {
    useRemoteEnvironment();
    routeRemote("failedStale");

    await expect(stopAgent("project", CONVERSATION_ID)).rejects.toBeInstanceOf(
      RemoteAgentStopError,
    );
  });

  it("returns immediately when the host already settled the intent synchronously", async () => {
    useRemoteEnvironment();
    routeRemote("stopped", null, "stopped");

    await expect(stopAgent("project", CONVERSATION_ID)).resolves.toBe(true);
    // No poll needed: the request itself came back terminal.
    expect(primitiveInvoke.mock.calls).toHaveLength(1);
  });

  it("keeps the local command on a local environment", async () => {
    useLocalEnvironment();
    primitiveInvoke.mockResolvedValue(true);

    await expect(stopAgent("project", CONVERSATION_ID)).resolves.toBe(true);
    expect(primitiveInvoke).toHaveBeenCalledWith(
      "stop_agent",
      { contextType: "project", contextId: CONVERSATION_ID },
      undefined,
    );
  });

  it("does not re-aim a non-project context at the conversation-scoped intent", async () => {
    useRemoteEnvironment();
    primitiveInvoke.mockImplementation(async (_command, payload) => {
      const cmd = (payload as { input: { cmd: string } }).input.cmd;
      if (cmd === "stop_agent") return { outcome: "ok", result: true };
      throw new Error(`unexpected remote command ${cmd}`);
    });

    // The intent carries no contextType, so a task/ideation stop must keep the local name and
    // fail honestly rather than silently stopping a conversation with a colliding id.
    await stopAgent("task_execution", "task-9");
    expect(wireCmd(0)).toBe("stop_agent");
  });
});
