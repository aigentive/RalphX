/**
 * Remote conversation MODE SWITCH routing (WP5a).
 *
 * `switch_agent_conversation_mode` prepares the workspace (`ensure_git_worktree` — a git
 * spawn) and is host-denied by the absolute process floor. Before start-mode parity, a paired
 * device could reach chat and nothing else. The switch now
 * persists an intent (`request_remote_agent_conversation_mode_switch`) and polls it to a
 * terminal state — the host owns the worktree.
 *
 * Pinned here:
 * 1. A remote environment NEVER invokes the denied local command; the intent + poll route runs.
 * 2. `alreadyInMode` is a SUCCESS terminal (a re-fired picker is a no-op, not an error toast).
 * 3. Every failure terminal becomes a VISIBLE error, not a silent revert.
 * 4. `base`/`runtimeOverride` must not cross the wire — host-forced under the spawn-free
 *    contract; their presence is a loud programming error.
 * 5. A local environment invokes `switch_agent_conversation_mode` exactly as before.
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

import {
  RemoteConversationModeSwitchError,
  switchAgentConversationMode,
} from "@/api/chat";
import { LOCAL_ENVIRONMENT_ID, useEnvironmentStore } from "@/stores/environmentStore";

const REMOTE_ID = "env-remote";
const CONVERSATION_ID = "conversation-1";
const PROJECT_ID = "project-1";
const MODE_SWITCH_REQUEST_ID = "mode-switch-req-1";
const NOW = "2026-08-01T13:00:00Z";

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

function wireCmd(callIndex = 0): string {
  return wireInput(callIndex).cmd as string;
}

/**
 * Routes the remote transport by facade command. `status`/`errorCode` pick the terminal poll
 * outcome; `pendingPolls` inserts non-terminal reads first so the poll loop is exercised.
 */
function routeRemote(
  status: string,
  errorCode: string | null = null,
  pendingPolls = 0,
): void {
  let polls = 0;
  primitiveInvoke.mockImplementation(async (_command, payload) => {
    const cmd = (payload as { input: { cmd: string } }).input.cmd;
    switch (cmd) {
      case "get_remote_agent_conversation":
        return { outcome: "ok", result: { conversation: RAW_CONVERSATION, messages: [] } };
      case "request_remote_agent_conversation_mode_switch":
        return {
          outcome: "ok",
          result: {
            modeSwitchRequestId: MODE_SWITCH_REQUEST_ID,
            conversationId: CONVERSATION_ID,
            status: "pending",
            deduplicated: false,
            createdAt: NOW,
          },
        };
      case "get_remote_conversation_mode_switch_request": {
        const settled = polls >= pendingPolls;
        polls += 1;
        return {
          outcome: "ok",
          result: {
            id: MODE_SWITCH_REQUEST_ID,
            conversationId: CONVERSATION_ID,
            targetMode: "edit",
            status: settled ? status : "switching",
            errorCode: settled ? errorCode : null,
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

describe("remote conversation mode-switch routing", () => {
  it("persists a mode-switch intent, polls it, and re-reads the conversation", async () => {
    useRemoteEnvironment();
    routeRemote("switched", null, 1);

    const result = await switchAgentConversationMode({
      conversationId: CONVERSATION_ID,
      mode: "edit",
    });

    const cmds = primitiveInvoke.mock.calls.map((_call, i) => wireCmd(i));
    expect(cmds).not.toContain("switch_agent_conversation_mode");
    expect(cmds[0]).toBe("get_remote_agent_conversation");
    expect(cmds[1]).toBe("request_remote_agent_conversation_mode_switch");
    expect(cmds).toContain("get_remote_conversation_mode_switch_request");

    expect(wireInput(1).args).toEqual({
      input: {
        conversationId: CONVERSATION_ID,
        projectId: PROJECT_ID,
        mode: "edit",
      },
    });

    expect(result.conversation.id).toBe(CONVERSATION_ID);
    expect(result.workspace).toBeNull();
  });

  it("treats alreadyInMode as success, not an error", async () => {
    useRemoteEnvironment();
    routeRemote("alreadyInMode");

    const result = await switchAgentConversationMode({
      conversationId: CONVERSATION_ID,
      mode: "edit",
    });
    expect(result.conversation.id).toBe(CONVERSATION_ID);
  });

  it("surfaces a failed terminal as a typed visible error", async () => {
    useRemoteEnvironment();
    routeRemote("failed", "REMOTE_CONV_MODE_SWITCH_HOST_FAILED");

    await expect(
      switchAgentConversationMode({ conversationId: CONVERSATION_ID, mode: "edit" }),
    ).rejects.toMatchObject({
      name: "RemoteConversationModeSwitchError",
      status: "failed",
      errorCode: "REMOTE_CONV_MODE_SWITCH_HOST_FAILED",
    });
  });

  it("refuses to forward base/runtimeOverride on a remote environment", async () => {
    useRemoteEnvironment();
    routeRemote("switched");

    await expect(
      switchAgentConversationMode({
        conversationId: CONVERSATION_ID,
        mode: "edit",
        base: {
          kind: "branch",
          ref: "main",
          displayName: "main",
        } as never,
      }),
    ).rejects.toThrow(/host owns workspace preparation/);
    expect(primitiveInvoke).not.toHaveBeenCalled();
  });

  it("keeps the local path on the local command", async () => {
    useLocalEnvironment();
    primitiveInvoke.mockImplementation(async (cmd) => {
      if (cmd === "switch_agent_conversation_mode") {
        return {
          conversation: RAW_CONVERSATION,
          workspace: null,
        };
      }
      throw new Error(`unexpected local command ${String(cmd)}`);
    });

    const result = await switchAgentConversationMode({
      conversationId: CONVERSATION_ID,
      mode: "edit",
    });
    expect(primitiveInvoke.mock.calls[0]?.[0]).toBe("switch_agent_conversation_mode");
    expect(result.conversation.id).toBe(CONVERSATION_ID);
  });
});

// The error class is exported for call sites that want to branch on status; keep the
// contract pinned so a rename breaks loudly.
it("exports the typed error with status and errorCode", () => {
  const error = new RemoteConversationModeSwitchError("failed", "X");
  expect(error.message).toBe("X");
  expect(error.errorCode).toBe("X");
});
