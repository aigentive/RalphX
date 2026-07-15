import { describe, expect, it } from "vitest";
import {
  AUTOMATION_SETUP_TOOLS,
  callAutomationSetupTool,
  isAutomationSetupToolName,
} from "../automation-tools.js";
import type { TauriCallOptions } from "../tauri-client.js";

type CapturedCall = {
  path: string;
  body: Record<string, unknown>;
  options?: TauriCallOptions;
};

function capturePost() {
  const calls: CapturedCall[] = [];
  const callTauri = async (
    path: string,
    body: Record<string, unknown>,
    options?: TauriCallOptions
  ): Promise<unknown> => {
    calls.push({ path, body, options });
    return { ok: true };
  };

  return { callTauri, calls };
}

describe("automation setup MCP tools", () => {
  it("recognizes only automation setup tool names", () => {
    expect(isAutomationSetupToolName("get_automation")).toBe(true);
    expect(isAutomationSetupToolName("update_automation")).toBe(true);
    expect(isAutomationSetupToolName("finalize_automation")).toBe(true);
    expect(isAutomationSetupToolName("list_projects")).toBe(false);
  });

  it("forwards get_automation with the caller conversation header", async () => {
    const { callTauri, calls } = capturePost();

    await callAutomationSetupTool("get_automation", callTauri, {}, {
      conversationId: "conversation-1",
    });

    expect(calls).toEqual([
      {
        path: "get_automation",
        body: {},
        options: {
          headers: {
            "X-RalphX-Caller-Session-Id": "conversation-1",
          },
        },
      },
    ]);
  });

  it("strips caller-supplied identity from update_automation payloads", async () => {
    const { callTauri, calls } = capturePost();

    await callAutomationSetupTool(
      "update_automation",
      callTauri,
      {
        id: "automation-should-not-forward",
        conversation_id: "conversation-should-not-forward",
        name: "Spec automation",
        max_runs: 12,
        max_consecutive_failures: 2,
      },
      { conversationId: "conversation-1" }
    );

    expect(calls).toEqual([
      {
        path: "update_automation",
        body: {
          name: "Spec automation",
          max_runs: 12,
          max_consecutive_failures: 2,
        },
        options: {
          headers: {
            "X-RalphX-Caller-Session-Id": "conversation-1",
          },
        },
      },
    ]);
  });

  it("forwards plan gate settings in update_automation payloads", async () => {
    const { callTauri, calls } = capturePost();

    await callAutomationSetupTool(
      "update_automation",
      callTauri,
      {
        plan_approval_mode: "automatic",
        pr_merge_mode: "automatic",
        plan_deep_verification: true,
      },
      { conversationId: "conversation-1" }
    );

    expect(calls).toEqual([
      {
        path: "update_automation",
        body: {
          plan_approval_mode: "automatic",
          pr_merge_mode: "automatic",
          plan_deep_verification: true,
        },
        options: {
          headers: {
            "X-RalphX-Caller-Session-Id": "conversation-1",
          },
        },
      },
    ]);
  });

  it("exposes plan gate settings in the update_automation schema", () => {
    const updateTool = AUTOMATION_SETUP_TOOLS.find(
      (tool) => tool.name === "update_automation"
    );
    const properties = updateTool?.inputSchema.properties as Record<
      string,
      { enum?: string[]; type?: string }
    >;

    expect(properties.plan_approval_mode).toMatchObject({
      type: "string",
      enum: ["manual", "automatic"],
    });
    expect(properties.pr_merge_mode).toMatchObject({
      type: "string",
      enum: ["manual", "automatic"],
    });
    expect(properties.plan_deep_verification).toMatchObject({
      type: "boolean",
    });
  });

  it("forwards finalize_automation without a body payload", async () => {
    const { callTauri, calls } = capturePost();

    await callAutomationSetupTool("finalize_automation", callTauri, {}, {
      conversationId: "conversation-1",
    });

    expect(calls[0]).toEqual({
      path: "finalize_automation",
      body: {},
      options: {
        headers: {
          "X-RalphX-Caller-Session-Id": "conversation-1",
        },
      },
    });
  });

  it("fails closed when the runtime context lacks a conversation id", async () => {
    const { callTauri } = capturePost();

    await expect(
      callAutomationSetupTool("get_automation", callTauri, {}, {})
    ).rejects.toThrow("requires the current setup conversation id");
  });
});
