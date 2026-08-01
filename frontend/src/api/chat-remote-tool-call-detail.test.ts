/**
 * WP3 Gap 4 — expanding a truncated tool call works against a remote host.
 *
 * `get_agent_message_tool_call_detail` / `get_agent_timeline_item_tool_call_detail` were
 * unregistered on the facade (a `FailOpenUntilFixed` audit refusal), so the transcript twins
 * showed a truncated delegate tool block a remote user could never expand.
 *
 * The client half needs nothing gated: neither command is in `LOCAL_ONLY_COMMANDS`, so
 * `typedInvoke` routes them to the host like any other read, and `useLazyToolCallDetail`
 * already renders a rejection as `detailError` rather than collapsing silently
 * (`ToolCallIndicator.test.tsx` — "shows detail load errors for previewed tool results").
 * These tests pin the routing and the error shape the hook depends on.
 */

import { beforeEach, describe, expect, it, vi } from "vitest";

const { primitiveInvoke } = vi.hoisted(() => ({
  primitiveInvoke: vi.fn<(cmd: string, args?: unknown) => Promise<unknown>>(),
}));

vi.mock("#tauri-core-primitive", async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>();
  return { ...actual, invoke: primitiveInvoke };
});

// `src/test/setup.ts` stubs `@tauri-apps/api/core` wholesale, which would bypass the transport
// seam this file exists to exercise. Restore the project-owned wrapper (the same module
// `vitest.config.ts` aliases the specifier to) so the local/remote routing decision is real.
vi.mock("@tauri-apps/api/core", async () =>
  vi.importActual<Record<string, unknown>>("@/lib/remote/invoke")
);

import {
  getAgentMessageToolCallDetail,
  getAgentTimelineItemToolCallDetail,
} from "@/api/chat";
import {
  resetTransportEnvironmentId,
  setTransportEnvironmentId,
} from "@/lib/remote/active-environment";
import { LOCAL_ONLY_COMMANDS } from "@/lib/remote/local-only-commands";

const REMOTE_ID = "env-remote";

const TOOL_CALL_DETAIL_COMMANDS = [
  "get_agent_message_tool_call_detail",
  "get_agent_timeline_item_tool_call_detail",
] as const;

const rawDetail = () => ({
  tool_call: {
    id: "call-1",
    name: "mcp__ralphx__delegate_start",
    arguments: { prompt: "inspect" },
    result: "the full, un-truncated delegated result",
  },
});

interface RecordedInvoke {
  cmd: string;
  args?: Record<string, unknown>;
}

function hostAnswers(envelope: unknown): RecordedInvoke[] {
  const calls: RecordedInvoke[] = [];
  primitiveInvoke.mockImplementation(async (cmd: string, args?: unknown) => {
    const input = (args as { input?: RecordedInvoke } | undefined)?.input;
    calls.push({ cmd, args: input as unknown as Record<string, unknown> });
    if (cmd !== "remote_invoke") {
      throw new Error(`the detail read must travel to the host, not run locally (${cmd})`);
    }
    return envelope;
  });
  return calls;
}

beforeEach(() => {
  primitiveInvoke.mockReset();
  resetTransportEnvironmentId();
});

describe("tool-call detail over the remote transport", () => {
  it("routes both detail reads to the host rather than pinning them local-only", () => {
    for (const command of TOOL_CALL_DETAIL_COMMANDS) {
      expect(
        LOCAL_ONLY_COMMANDS.some((entry) => entry.command === command)
      ).toBe(false);
    }
  });

  it("expands a message tool call against the host", async () => {
    setTransportEnvironmentId(REMOTE_ID);
    const calls = hostAnswers({ outcome: "ok", result: rawDetail() });

    const detail = await getAgentMessageToolCallDetail({
      conversationId: "conv-1",
      messageId: "msg-1",
      toolCallId: "call-1",
    });

    expect(calls[0]?.cmd).toBe("remote_invoke");
    expect(calls[0]?.args?.cmd).toBe("get_agent_message_tool_call_detail");
    expect(detail?.toolCall.result).toBe(
      "the full, un-truncated delegated result"
    );
  });

  it("expands a timeline tool call against the host", async () => {
    setTransportEnvironmentId(REMOTE_ID);
    const calls = hostAnswers({ outcome: "ok", result: rawDetail() });

    const detail = await getAgentTimelineItemToolCallDetail("conv-1", "item-1");

    expect(calls[0]?.args?.cmd).toBe("get_agent_timeline_item_tool_call_detail");
    expect(detail?.toolCall.id).toBe("call-1");
  });

  it("surfaces a host-side failure as a rejection, never as an empty expansion", async () => {
    setTransportEnvironmentId(REMOTE_ID);
    // The WP3 backend fix makes this the shape a repository outage takes: the command's own
    // `Err`, unwrapped by the transport. A silent `null` here would render as
    // "Full result is unavailable." — indistinguishable from a genuinely absent payload.
    hostAnswers({
      outcome: "commandError",
      error: "Database error: simulated delegated-session outage",
    });

    await expect(
      getAgentMessageToolCallDetail({
        conversationId: "conv-1",
        messageId: "msg-1",
        toolCallId: "call-1",
      })
    ).rejects.toBe("Database error: simulated delegated-session outage");
  });
});
