/**
 * The remote half of the Agents transcript reads.
 *
 * Like `chat-remote-send.test.ts`, these tests exercise the REAL transport wrapper —
 * `@tauri-apps/api/core` is aliased to `lib/remote/invoke`, so a remote environment
 * routes through `networkInvoke` and out via the `remote_invoke` primitive. Mocking the
 * API module would prove nothing about the routing, which is the only thing in question.
 *
 * What is being pinned: the local `get_agent_conversation*` / `list_agent_conversations*`
 * commands are DELIBERATELY unregistered on the facade (they open by waking the
 * conversation's agent workspace, which reaches a live-agent steer sink — see
 * `remote_server/registry.rs::the_local_transcript_reads_stay_unregistered`). Routing any
 * of them remotely answers `REMOTE_COMMAND_UNAVAILABLE`, i.e. the Agents surface cannot
 * load a conversation at all. The spawn-free twins in `commands/remote_transcript_commands.rs`
 * are what a paired device must call instead.
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
  getConversation,
  getConversationMessagesPage,
  getConversationTimelinePage,
  listConversations,
  listConversationsPage,
} from "@/api/chat";
import { LOCAL_ENVIRONMENT_ID, useEnvironmentStore } from "@/stores/environmentStore";

const REMOTE_ID = "env-remote";
const CONVERSATION_ID = "conversation-1";
const PROJECT_ID = "project-1";

const RAW_CONVERSATION = {
  id: CONVERSATION_ID,
  context_type: "project",
  context_id: PROJECT_ID,
  claude_session_id: null,
  title: "Ship it",
  message_count: 0,
  last_message_at: null,
  created_at: "2026-07-28T12:00:00Z",
  updated_at: "2026-07-28T12:00:00Z",
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

/** The `{requestId, cmd, args}` envelope of the Nth `remote_invoke`. */
function wireInput(callIndex = 0): Record<string, unknown> {
  const args = primitiveInvoke.mock.calls[callIndex]?.[1] as {
    input: Record<string, unknown>;
  };
  return args.input;
}

/** Resolves the remote transport envelope with `result`. */
function remoteOk(result: unknown): void {
  primitiveInvoke.mockResolvedValue({ outcome: "ok", result });
}

beforeEach(() => {
  primitiveInvoke.mockReset();
  useLocalEnvironment();
});

describe("remote transcript read routing", () => {
  it("lists conversations via list_remote_agent_conversations, never the unregistered local one", async () => {
    useRemoteEnvironment();
    remoteOk([RAW_CONVERSATION]);

    const conversations = await listConversations("project", PROJECT_ID, true);

    expect(primitiveInvoke).toHaveBeenCalledTimes(1);
    expect(primitiveInvoke.mock.calls[0]?.[0]).toBe("remote_invoke");

    const input = wireInput();
    expect(input.id).toBe(REMOTE_ID);
    expect(input.cmd).toBe("list_remote_agent_conversations");
    expect(input.cmd).not.toBe("list_agent_conversations");
    expect(input.args).toEqual({
      contextType: "project",
      contextId: PROJECT_ID,
      includeArchived: true,
    });

    expect(conversations).toHaveLength(1);
    expect(conversations[0]?.id).toBe(CONVERSATION_ID);
  });

  it("pages the conversation list via list_remote_agent_conversations_page", async () => {
    useRemoteEnvironment();
    remoteOk({
      conversations: [RAW_CONVERSATION],
      limit: 6,
      offset: 12,
      total: 30,
      has_more: true,
    });

    const page = await listConversationsPage(
      "project",
      PROJECT_ID,
      6,
      12,
      true,
      "  ship  ",
      true,
    );

    const input = wireInput();
    expect(input.cmd).toBe("list_remote_agent_conversations_page");
    expect(input.args).toEqual({
      contextType: "project",
      contextId: PROJECT_ID,
      includeArchived: true,
      archivedOnly: true,
      limit: 6,
      offset: 12,
      search: "ship",
    });

    expect(page.offset).toBe(12);
    expect(page.hasMore).toBe(true);
    expect(page.total).toBe(30);
  });

  it("reads a conversation via get_remote_agent_conversation", async () => {
    useRemoteEnvironment();
    remoteOk({ conversation: RAW_CONVERSATION, messages: [] });

    const result = await getConversation(CONVERSATION_ID);

    const input = wireInput();
    expect(input.cmd).toBe("get_remote_agent_conversation");
    expect(input.cmd).not.toBe("get_agent_conversation");
    expect(input.args).toEqual({ conversationId: CONVERSATION_ID });
    expect(result.conversation.id).toBe(CONVERSATION_ID);
  });

  it("reads a messages page via get_remote_agent_conversation_messages_page", async () => {
    useRemoteEnvironment();
    remoteOk({
      conversation: RAW_CONVERSATION,
      messages: [],
      limit: 40,
      offset: 80,
      total_message_count: 120,
      has_older: true,
    });

    const page = await getConversationMessagesPage(CONVERSATION_ID, 40, 80);

    const input = wireInput();
    expect(input.cmd).toBe("get_remote_agent_conversation_messages_page");
    expect(input.args).toEqual({
      conversationId: CONVERSATION_ID,
      limit: 40,
      offset: 80,
    });
    expect(page.offset).toBe(80);
    expect(page.hasOlder).toBe(true);
  });

  it("reads a timeline page via get_remote_agent_conversation_timeline_page", async () => {
    useRemoteEnvironment();
    remoteOk({
      conversation: RAW_CONVERSATION,
      items: [],
      limit: 40,
      before_sequence: 512,
      total_item_count: 900,
      has_older: true,
      oldest_loaded_sequence: 480,
      newest_loaded_sequence: 511,
    });

    const page = await getConversationTimelinePage(CONVERSATION_ID, 40, 512);

    const input = wireInput();
    expect(input.cmd).toBe("get_remote_agent_conversation_timeline_page");
    expect(input.args).toEqual({
      conversationId: CONVERSATION_ID,
      limit: 40,
      beforeSequence: 512,
    });
    expect(page.beforeSequence).toBe(512);
    expect(page.oldestLoadedSequence).toBe(480);
  });

  it("sends the tail-first first page as beforeSequence null, not a missing key", async () => {
    useRemoteEnvironment();
    remoteOk({
      conversation: RAW_CONVERSATION,
      items: [],
      limit: 40,
      before_sequence: null,
      total_item_count: 0,
      has_older: false,
    });

    await getConversationTimelinePage(CONVERSATION_ID, 40);

    expect(wireInput().args).toEqual({
      conversationId: CONVERSATION_ID,
      limit: 40,
      beforeSequence: null,
    });
  });
});

describe("remote page-limit clamping", () => {
  // The host clamps to `1..=200` (`remote_transcript_commands.rs` MAX_PAGE_LIMIT) rather
  // than erroring, so an over-wide request would come back NARROWER than asked without
  // saying so. Clamping client-side keeps the request we send the request the host runs.
  it("never asks the host for a wider messages page than it will serve", async () => {
    useRemoteEnvironment();
    remoteOk({
      conversation: RAW_CONVERSATION,
      messages: [],
      limit: 200,
      offset: 0,
      total_message_count: 0,
      has_older: false,
    });

    await getConversationMessagesPage(CONVERSATION_ID, 5000);

    expect((wireInput().args as { limit: number }).limit).toBe(200);
  });

  it("never asks the host for a zero-width timeline page", async () => {
    useRemoteEnvironment();
    remoteOk({
      conversation: RAW_CONVERSATION,
      items: [],
      limit: 1,
      before_sequence: null,
      total_item_count: 0,
      has_older: false,
    });

    await getConversationTimelinePage(CONVERSATION_ID, 0);

    expect((wireInput().args as { limit: number }).limit).toBe(1);
  });

  it("clamps the conversation-list page too", async () => {
    useRemoteEnvironment();
    remoteOk({
      conversations: [],
      limit: 200,
      offset: 0,
      total: 0,
      has_more: false,
    });

    await listConversationsPage("project", PROJECT_ID, 5000);

    expect((wireInput().args as { limit: number }).limit).toBe(200);
  });

  it("leaves the local transport's limits alone — the clamp is a wire concern only", async () => {
    primitiveInvoke.mockResolvedValue({
      conversations: [],
      limit: 5000,
      offset: 0,
      total: 0,
      has_more: false,
    });

    await listConversationsPage("project", PROJECT_ID, 5000);

    const args = primitiveInvoke.mock.calls[0]?.[1] as { limit: number };
    expect(args.limit).toBe(5000);
  });
});

describe("local transcript reads are unchanged", () => {
  it("keeps calling the local commands on the local environment", async () => {
    primitiveInvoke.mockImplementation(async (cmd: string) => {
      switch (cmd) {
        case "list_agent_conversations":
          return [RAW_CONVERSATION];
        case "list_agent_conversations_page":
          return {
            conversations: [RAW_CONVERSATION],
            limit: 6,
            offset: 0,
            total: 1,
            has_more: false,
          };
        case "get_agent_conversation":
          return { conversation: RAW_CONVERSATION, messages: [] };
        case "get_agent_conversation_messages_page":
          return {
            conversation: RAW_CONVERSATION,
            messages: [],
            limit: 40,
            offset: 0,
            total_message_count: 0,
            has_older: false,
          };
        case "get_agent_conversation_timeline_page":
          return {
            conversation: RAW_CONVERSATION,
            items: [],
            limit: 40,
            before_sequence: null,
            total_item_count: 0,
            has_older: false,
          };
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    await listConversations("project", PROJECT_ID);
    await listConversationsPage("project", PROJECT_ID, 6);
    await getConversation(CONVERSATION_ID);
    await getConversationMessagesPage(CONVERSATION_ID, 40);
    await getConversationTimelinePage(CONVERSATION_ID, 40);

    expect(primitiveInvoke.mock.calls.map((call) => call[0])).toEqual([
      "list_agent_conversations",
      "list_agent_conversations_page",
      "get_agent_conversation",
      "get_agent_conversation_messages_page",
      "get_agent_conversation_timeline_page",
    ]);
    // Nothing left the machine.
    expect(
      primitiveInvoke.mock.calls.some((call) => call[0] === "remote_invoke"),
    ).toBe(false);
  });
});
