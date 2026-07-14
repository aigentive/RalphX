import { describe, expect, it, beforeEach, afterEach, vi } from "vitest";

import {
  callTicketAttachmentTool,
  isTicketAttachmentToolName,
  safeTicketAttachmentResult,
} from "../ticket-attachment-tools.js";
import {
  getAllTools,
  getFilteredTools,
  setAgentType,
} from "../tools.js";
import {
  CODER,
  GENERAL_WORKER,
  MERGER,
  ORCHESTRATOR_IDEATION,
  REVIEWER,
  WORKER,
  WORKER_TEAM_LEAD,
} from "../agentNames.js";

const TOOL_NAMES = ["list_ticket_attachments", "fetch_ticket_attachment"];

describe("ticket attachment MCP tools", () => {
  beforeEach(() => {
    delete process.env.RALPHX_ALLOWED_MCP_TOOLS;
    delete process.env.RALPHX_AGENT_PROFILE;
  });

  afterEach(() => {
    delete process.env.RALPHX_ALLOWED_MCP_TOOLS;
    delete process.env.RALPHX_AGENT_PROFILE;
  });

  it("registers exactly the read-only attachment tool schemas", () => {
    const allTools = getAllTools();

    for (const name of TOOL_NAMES) {
      const tool = allTools.find((candidate) => candidate.name === name);
      expect(tool, `${name} registered`).toBeDefined();
      expect(Object.keys(tool!.inputSchema.properties ?? {})).not.toEqual(
        expect.arrayContaining(["url", "path", "token", "credential"])
      );
    }

    const listTool = allTools.find((candidate) => candidate.name === "list_ticket_attachments")!;
    expect(listTool.inputSchema.required).toEqual(["provider", "ticket_id"]);
    expect(listTool.inputSchema.properties?.provider).toMatchObject({
      enum: ["jira", "linear", "click_up"],
    });

    const fetchTool = allTools.find((candidate) => candidate.name === "fetch_ticket_attachment")!;
    expect(fetchTool.inputSchema.required).toEqual([
      "provider",
      "ticket_id",
      "content_pointer",
    ]);
  });

  it.each([WORKER, CODER, WORKER_TEAM_LEAD])(
    "exposes attachment tools to %s through canonical grants",
    (agent) => {
      setAgentType(agent);

      const toolNames = getFilteredTools().map((tool) => tool.name);

      expect(toolNames).toEqual(expect.arrayContaining(TOOL_NAMES));
    }
  );

  it("keeps attachment tools off Plan-mode ideation discovery", () => {
    setAgentType(ORCHESTRATOR_IDEATION);
    process.env.RALPHX_AGENT_PROFILE = "plan";

    const toolNames = getFilteredTools().map((tool) => tool.name);

    expect(toolNames).not.toContain("list_ticket_attachments");
    expect(toolNames).not.toContain("fetch_ticket_attachment");
  });

  it.each([ORCHESTRATOR_IDEATION, REVIEWER, MERGER, GENERAL_WORKER])(
    "keeps attachment tools off unrelated %s discovery",
    (agent) => {
      setAgentType(agent);

      const toolNames = getFilteredTools().map((tool) => tool.name);

      expect(toolNames).not.toContain("list_ticket_attachments");
      expect(toolNames).not.toContain("fetch_ticket_attachment");
    }
  );

  it("dispatches through the internal ticket attachment endpoints", async () => {
    const callTauri = vi.fn().mockResolvedValue({
      attachments: [],
    });

    await expect(
      callTicketAttachmentTool("list_ticket_attachments", callTauri, {
        provider: "jira",
        ticket_id: "JIRA-123",
      })
    ).resolves.toEqual({ attachments: [] });

    expect(callTauri).toHaveBeenCalledWith("ticket_attachments/list", {
      provider: "jira",
      ticket_id: "JIRA-123",
    });

    await callTicketAttachmentTool("fetch_ticket_attachment", callTauri, {
      provider: "jira",
      ticket_id: "JIRA-123",
      content_pointer: "ta_123",
    });

    expect(callTauri).toHaveBeenLastCalledWith("ticket_attachments/fetch", {
      provider: "jira",
      ticket_id: "JIRA-123",
      content_pointer: "ta_123",
    });
  });

  it("redacts unsafe backend fields from attachment results", () => {
    const shaped = safeTicketAttachmentResult({
      attachments: [
        {
          provider: "jira",
          id: "ta_123",
          fileName: "design.png",
          contentPointer: { id: "ta_123" },
          content_url: "https://example.test/download",
          token: "Bearer abcdefghijklmnopqrstuvwxyz",
          location: "/tmp/cache/file",
        },
      ],
    });

    expect(shaped).toEqual({
      attachments: [
        {
          provider: "jira",
          id: "ta_123",
          fileName: "design.png",
          contentPointer: { id: "ta_123" },
        },
      ],
    });
    expect(JSON.stringify(shaped)).not.toContain("https://");
    expect(JSON.stringify(shaped)).not.toContain("Bearer");
    expect(JSON.stringify(shaped)).not.toContain("/tmp/cache");
  });

  it("identifies only ticket attachment tool names", () => {
    expect(isTicketAttachmentToolName("list_ticket_attachments")).toBe(true);
    expect(isTicketAttachmentToolName("fetch_ticket_attachment")).toBe(true);
    expect(isTicketAttachmentToolName("get_task_context")).toBe(false);
  });
});
