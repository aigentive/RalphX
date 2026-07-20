import { describe, expect, it } from "vitest";
import {
  PERSONA_BUILDER_TOOLS,
  callPersonaTool,
  isPersonaToolName,
} from "../persona-tools.js";
import type { TauriCallOptions } from "../tauri-client.js";

type CapturedCall = {
  path: string;
  body?: Record<string, unknown>;
  options?: TauriCallOptions;
};

function captureCalls() {
  const postCalls: CapturedCall[] = [];
  const getCalls: CapturedCall[] = [];
  const callTauri = async (
    path: string,
    body: Record<string, unknown>,
    options?: TauriCallOptions
  ): Promise<unknown> => {
    postCalls.push({ path, body, options });
    return { ok: true };
  };
  const callTauriGet = async (
    path: string,
    options?: TauriCallOptions
  ): Promise<unknown> => {
    getCalls.push({ path, options });
    return { ok: true };
  };

  return { callTauri, callTauriGet, getCalls, postCalls };
}

describe("persona builder MCP tools", () => {
  it("defines and recognizes exactly the persona draft tools", () => {
    expect(PERSONA_BUILDER_TOOLS.map((tool) => tool.name)).toEqual([
      "save_persona_draft",
      "get_persona_draft",
    ]);
    expect(isPersonaToolName("save_persona_draft")).toBe(true);
    expect(isPersonaToolName("get_persona_draft")).toBe(true);
    expect(isPersonaToolName("update_automation")).toBe(false);
    expect(PERSONA_BUILDER_TOOLS[0]?.description).toContain(
      "A prose or Markdown-only response does not create a Persona"
    );
  });

  it("posts the Rust request shape and caller conversation header when saving", async () => {
    const { callTauri, callTauriGet, postCalls } = captureCalls();

    await callPersonaTool(
      "save_persona_draft",
      callTauri,
      callTauriGet,
      {
        draftId: "draft-1",
        slug: "researcher",
        content: "kind: persona\nrole: research",
        sourceSessionId: "source-session-1",
        caller_session_id: "must-not-forward",
      },
      { conversationId: "conversation-1" }
    );

    expect(postCalls).toEqual([
      {
        path: "save_persona_draft",
        body: {
          draftId: "draft-1",
          slug: "researcher",
          content: "kind: persona\nrole: research",
          sourceSessionId: "source-session-1",
        },
        options: {
          headers: {
            "X-RalphX-Caller-Session-Id": "conversation-1",
          },
        },
      },
    ]);
  });

  it("gets a persona draft through the GET route with caller context", async () => {
    const { callTauri, callTauriGet, getCalls } = captureCalls();

    await callPersonaTool(
      "get_persona_draft",
      callTauri,
      callTauriGet,
      { draft_id: "draft-1" },
      { conversationId: "conversation-1" }
    );

    expect(getCalls).toEqual([
      {
        path: "get_persona_draft/draft-1",
        options: {
          headers: {
            "X-RalphX-Caller-Session-Id": "conversation-1",
          },
        },
      },
    ]);
  });

  it("fails closed without the caller conversation context", async () => {
    const { callTauri, callTauriGet } = captureCalls();

    await expect(
      callPersonaTool(
        "save_persona_draft",
        callTauri,
        callTauriGet,
        { slug: "researcher", content: "kind: persona" },
        {}
      )
    ).rejects.toThrow("requires the current PersonaBuilder conversation id");
  });

  it("rejects unsupported tools and malformed draft reads before making backend calls", async () => {
    const { callTauri, callTauriGet, getCalls, postCalls } = captureCalls();

    await expect(
      callPersonaTool("unsupported_persona_tool", callTauri, callTauriGet, {}, { conversationId: "conversation-1" })
    ).rejects.toThrow("Unsupported persona tool");
    await expect(
      callPersonaTool("get_persona_draft", callTauri, callTauriGet, { draft_id: "   " }, { conversationId: "conversation-1" })
    ).rejects.toThrow("requires a non-empty draft_id");

    expect(postCalls).toEqual([]);
    expect(getCalls).toEqual([]);
  });

  it("filters non-object save arguments while preserving trimmed caller context", async () => {
    const { callTauri, callTauriGet, postCalls } = captureCalls();

    await callPersonaTool("save_persona_draft", callTauri, callTauriGet, null, {
      conversationId: "  conversation-1  ",
    });

    expect(postCalls).toEqual([
      {
        path: "save_persona_draft",
        body: {},
        options: { headers: { "X-RalphX-Caller-Session-Id": "conversation-1" } },
      },
    ]);
  });
});
