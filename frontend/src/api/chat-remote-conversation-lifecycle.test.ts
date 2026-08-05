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
  archiveConversation,
  forkAgentConversation,
  setAgentConversationMuted,
} from "@/api/chat";
import { switchConversationPersona } from "@/hooks/usePersonas";
import { LOCAL_ENVIRONMENT_ID, useEnvironmentStore } from "@/stores/environmentStore";

const REMOTE_ID = "env-remote";
const NOW = "2026-08-05T12:00:00Z";

function conversation(id: string, archivedAt: string | null = null) {
  return {
    id,
    context_type: "project",
    context_id: "project-1",
    claude_session_id: null,
    title: id,
    message_count: 0,
    last_message_at: null,
    created_at: NOW,
    updated_at: NOW,
    archived_at: archivedAt,
  };
}

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

beforeEach(() => {
  primitiveInvoke.mockReset();
  useLocalEnvironment();
});

describe("remote conversation lifecycle routing", () => {
  it("routes mute and persona writes through their direct remote twins", async () => {
    useRemoteEnvironment();
    primitiveInvoke.mockImplementation(async (command, payload) => {
      expect(command).toBe("remote_invoke");
      const cmd = (payload as { input: { cmd: string } }).input.cmd;
      if (cmd === "set_remote_agent_conversation_muted") {
        return { outcome: "ok", result: null };
      }
      if (cmd === "switch_remote_agent_conversation_persona") {
        return {
          outcome: "ok",
          result: {
            conversation: {
              id: "conversation-1",
              context_type: "project",
              context_id: "project-1",
              persona_id: "persona-1",
            },
          },
        };
      }
      throw new Error(`unexpected remote command ${cmd}`);
    });

    await setAgentConversationMuted("conversation-1", true);
    await switchConversationPersona({
      conversationId: "conversation-1",
      personaId: "persona-1",
    });

    const commands = primitiveInvoke.mock.calls.map(
      (call) => (call[1] as { input: { cmd: string } }).input.cmd,
    );
    expect(commands).toEqual([
      "set_remote_agent_conversation_muted",
      "switch_remote_agent_conversation_persona",
    ]);
    expect(commands).not.toContain("set_agent_conversation_muted");
    expect(commands).not.toContain("switch_agent_conversation_persona");
  });

  it("leaves both local command branches unchanged", async () => {
    primitiveInvoke.mockResolvedValue(null);

    await setAgentConversationMuted("conversation-1", false);
    await switchConversationPersona({ conversationId: "conversation-1", personaId: null });

    // The invoke seam passes an optional third schema argument; assert the command name and
    // payload per call rather than the whole tuple, which would pin that arity by accident.
    expect(
      primitiveInvoke.mock.calls.map(([command, args]) => [command, args])
    ).toEqual([
      [
        "set_agent_conversation_muted",
        { input: { conversationId: "conversation-1", muted: false } },
      ],
      [
        "switch_agent_conversation_persona",
        { input: { conversationId: "conversation-1", personaId: null } },
      ],
    ]);
  });

  it("surfaces the host running-agent rejection copy", async () => {
    useRemoteEnvironment();
    // The real host envelope: `commandError` carries the command's own error value
    // unwrapped (network-invoke.ts:189-192), and this command's error IS the string
    // PERSONA_SWITCH_AGENT_RUNNING_ERROR. A mock-convenient {outcome:"error"} shape would
    // never reach the client's rejection path at all.
    primitiveInvoke.mockResolvedValue({
      outcome: "commandError",
      error: "Cannot change persona while the agent is running",
    });

    await expect(
      switchConversationPersona({ conversationId: "conversation-1", personaId: null }),
    ).rejects.toThrow("Cannot change persona while the agent is running");
  });

  it("treats the real ALREADY_ARCHIVED command-error envelope as benign", async () => {
    useRemoteEnvironment();
    primitiveInvoke.mockImplementation(async (_command, payload) => {
      const cmd = (payload as { input: { cmd: string } }).input.cmd;
      if (cmd === "request_remote_conversation_archive") {
        return {
          outcome: "commandError",
          error: "REMOTE_CONVERSATION_LIFECYCLE_ALREADY_ARCHIVED",
        };
      }
      if (cmd === "get_remote_agent_conversation") {
        return {
          outcome: "ok",
          result: { conversation: conversation("conversation-1", NOW), messages: [] },
        };
      }
      throw new Error(`unexpected remote command ${cmd}`);
    });

    await expect(
      archiveConversation("conversation-1", { closePullRequest: false }),
    ).resolves.toMatchObject({ conversation: { id: "conversation-1" } });
  });

  it("resolves a benign CHILD_EXISTS completion with the preallocated child id", async () => {
    useRemoteEnvironment();
    primitiveInvoke.mockImplementation(async (_command, payload) => {
      const cmd = (payload as { input: { cmd: string } }).input.cmd;
      if (cmd === "request_remote_conversation_fork") {
        return {
          outcome: "ok",
          result: {
            requestId: "request-1",
            allocatedConversationId: "conversation-child",
            status: "pending",
            deduplicated: false,
            createdAt: NOW,
          },
        };
      }
      if (cmd === "get_remote_conversation_lifecycle_request") {
        return {
          outcome: "ok",
          result: {
            requestId: "request-1",
            kind: "fork",
            conversationId: "conversation-parent",
            allocatedConversationId: "conversation-child",
            status: "completed",
            errorCode: null,
            result: {
              conversationId: "conversation-child",
              code: "REMOTE_CONVERSATION_LIFECYCLE_CHILD_EXISTS",
              benign: true,
            },
            createdAt: NOW,
            updatedAt: NOW,
          },
        };
      }
      if (cmd === "get_remote_agent_conversation") {
        const conversationId = (payload as { input: { args: { conversationId: string } } })
          .input.args.conversationId;
        return {
          outcome: "ok",
          result: { conversation: conversation(conversationId), messages: [] },
        };
      }
      throw new Error(`unexpected remote command ${cmd}`);
    });

    await expect(forkAgentConversation("conversation-parent")).resolves.toMatchObject({
      conversation: { id: "conversation-child" },
    });
  });

  it("keeps local archive and fork on their original commands", async () => {
    primitiveInvoke.mockImplementation(async (command) => {
      if (command === "archive_agent_conversation") {
        return {
          conversation: conversation("conversation-1", NOW),
          cleanup: {
            runtime_shutdown_succeeded: true,
            cleanup_claim: "not_claimed",
            local_cleanup: "cleaned",
            message: null,
          },
        };
      }
      if (command === "fork_agent_conversation") {
        return {
          parent_conversation: conversation("conversation-parent"),
          conversation: conversation("conversation-child"),
          workspace: null,
          provider_session_forked: false,
          copied_message_count: 0,
          copied_timeline_item_count: 0,
        };
      }
      throw new Error(`unexpected local command ${command}`);
    });

    await archiveConversation("conversation-1", { closePullRequest: false });
    await forkAgentConversation("conversation-parent");
    expect(primitiveInvoke.mock.calls.map(([command]) => command)).toEqual([
      "archive_agent_conversation",
      "fork_agent_conversation",
    ]);
  });
});
