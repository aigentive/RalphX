/**
 * The typed capability boundary for remounted HTTP reads.
 *
 * A remote host answers routes it does not mount with a 404 whose body carries the
 * typed envelope `{code: "REMOTE_COMMAND_UNAVAILABLE", message}` (`remote_server/mod.rs`).
 * `fetchAgentWorkspaceJson` used to flatten that into a generic `AgentWorkspaceHttpError`,
 * which the hydration barrier read as host UNHEALTH — so a paired client whose mounted
 * queries included `workspace-review-context` or `conversation-issues` could NEVER leave
 * "Reconnecting…": every attempt opened a socket, failed the barrier on the 404, and tore
 * down (observed live 2026-08-01). These tests pin the conversion that breaks the loop.
 */

import { beforeEach, describe, expect, it, vi } from "vitest";

const { primitiveInvoke } = vi.hoisted(() => ({
  primitiveInvoke: vi.fn<(cmd: string, args?: unknown) => Promise<unknown>>(),
}));

vi.mock("#tauri-core-primitive", async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>();
  return { ...actual, invoke: primitiveInvoke };
});

import {
  AgentWorkspaceHttpError,
  getAgentWorkspaceReviewContext,
  listAgentConversationIssues,
} from "@/api/chat";
import {
  resetTransportEnvironmentId,
  setTransportEnvironmentId,
} from "@/lib/remote/active-environment";
import { RemoteTransportError } from "@/lib/remote/transport-errors";

const REMOTE_ID = "env-remote";

function remoteFetchAnswers(status: number, body: unknown): void {
  primitiveInvoke.mockImplementation(async (cmd: string) => {
    if (cmd !== "remote_fetch") {
      throw new Error(`unexpected primitive invoke: ${cmd}`);
    }
    return { status, headers: [], body: JSON.stringify(body) };
  });
}

beforeEach(() => {
  primitiveInvoke.mockReset();
  resetTransportEnvironmentId();
});

describe("remote route unavailability is a typed capability boundary", () => {
  it("lifts the host's REMOTE_COMMAND_UNAVAILABLE envelope into RemoteTransportError", async () => {
    setTransportEnvironmentId(REMOTE_ID);
    remoteFetchAnswers(404, {
      code: "REMOTE_COMMAND_UNAVAILABLE",
      message: "This remote route is not available.",
    });

    const failure = await getAgentWorkspaceReviewContext("conv-1").catch(
      (error: unknown) => error
    );

    expect(failure).toBeInstanceOf(RemoteTransportError);
    expect((failure as RemoteTransportError).code).toBe(
      "REMOTE_COMMAND_UNAVAILABLE"
    );
    expect((failure as RemoteTransportError).environmentId).toBe(REMOTE_ID);
  });

  it("lifts the envelope on POST-shaped reads too (conversation issues)", async () => {
    setTransportEnvironmentId(REMOTE_ID);
    remoteFetchAnswers(404, {
      code: "REMOTE_COMMAND_UNAVAILABLE",
      message: "This remote route is not available.",
    });

    const failure = await listAgentConversationIssues("conv-1").catch(
      (error: unknown) => error
    );

    expect(failure).toBeInstanceOf(RemoteTransportError);
    expect((failure as RemoteTransportError).code).toBe(
      "REMOTE_COMMAND_UNAVAILABLE"
    );
  });

  it("keeps a remote 404 WITHOUT the envelope as the plain HTTP error", async () => {
    setTransportEnvironmentId(REMOTE_ID);
    // A mounted route answering 404 about a specific entity is the endpoint's own
    // answer, not a capability statement — it must not be read as "unsupported host".
    remoteFetchAnswers(404, { error: "conversation not found" });

    const failure = await getAgentWorkspaceReviewContext("conv-1").catch(
      (error: unknown) => error
    );

    expect(failure).toBeInstanceOf(AgentWorkspaceHttpError);
  });

  it("keeps local-environment failures on the plain HTTP error path", async () => {
    const originalFetch = globalThis.fetch;
    globalThis.fetch = vi.fn(async () =>
      new Response(
        JSON.stringify({
          code: "REMOTE_COMMAND_UNAVAILABLE",
          message: "This remote route is not available.",
        }),
        { status: 404, statusText: "Not Found" }
      )
    ) as typeof fetch;
    try {
      const failure = await getAgentWorkspaceReviewContext("conv-1").catch(
        (error: unknown) => error
      );

      // The local backend never speaks the remote envelope; even if a body happens to
      // look like one, a local failure must stay an AgentWorkspaceHttpError.
      expect(failure).toBeInstanceOf(AgentWorkspaceHttpError);
      expect(primitiveInvoke).not.toHaveBeenCalled();
    } finally {
      globalThis.fetch = originalFetch;
    }
  });
});
