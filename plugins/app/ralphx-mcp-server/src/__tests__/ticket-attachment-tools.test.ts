import { describe, expect, it, beforeEach, afterEach, vi } from "vitest";

import {
  callTicketAttachmentTool,
  isTicketAttachmentToolName,
  safeTicketAttachmentResult,
} from "../ticket-attachment-tools.js";
import {
  getAllTools,
  getFilteredTools,
  getToolsByAgent,
  setAgentType,
} from "../tools.js";
import {
  CODER,
  GENERAL_WORKER,
  MERGER,
  ORCHESTRATOR_IDEATION,
  REVIEWER,
  WORKER,
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
    // "path" is deliberately excluded here: fetch_ticket_attachment's
    // description now documents the contentPath delivery shape. Argument
    // shape stays covered below via inputSchema.properties.
    const forbiddenSchemaTerms = [
      "url",
      "token",
      "credential",
      "authorization",
      "download",
      "cache",
      "source",
      "handle",
    ];

    for (const name of TOOL_NAMES) {
      const tool = allTools.find((candidate) => candidate.name === name);
      expect(tool, `${name} registered`).toBeDefined();
      const properties = Object.keys(tool!.inputSchema.properties ?? {});
      expect(properties).not.toEqual(
        expect.arrayContaining([
          "url",
          "path",
          "token",
          "credential",
          "authorization",
          "download_url",
          "content_url",
          "cache_path",
        ])
      );
      // Arguments never accept a path/location/handle-shaped input, even
      // though the response may now carry a materialized contentPath.
      const inputSchemaText = JSON.stringify(tool!.inputSchema).toLowerCase();
      for (const term of [...forbiddenSchemaTerms, "path"]) {
        expect(
          inputSchemaText,
          `${name} input schema must not expose ${term}`
        ).not.toContain(term);
      }
      const schemaText = JSON.stringify(tool).toLowerCase();
      for (const term of forbiddenSchemaTerms) {
        expect(schemaText, `${name} schema must not expose ${term}`).not.toContain(
          term
        );
      }
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

  it.each([WORKER, CODER])(
    "exposes attachment tools to %s through canonical grants",
    (agent) => {
      setAgentType(agent);

      const toolNames = getFilteredTools().map((tool) => tool.name);
      const configuredTools = getToolsByAgent()[agent];

      expect(toolNames).toEqual(expect.arrayContaining(TOOL_NAMES));
      expect(configuredTools).toEqual(expect.arrayContaining(TOOL_NAMES));
    }
  );

  it("keeps live discovery bounded by the injected runtime allowlist", () => {
    setAgentType(WORKER);
    process.env.RALPHX_ALLOWED_MCP_TOOLS = "list_ticket_attachments";

    const toolNames = getFilteredTools().map((tool) => tool.name);

    expect(toolNames).toContain("list_ticket_attachments");
    expect(toolNames).not.toContain("fetch_ticket_attachment");
  });

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
          nested: {
            downloadUrl: "https://example.test/direct",
            authorization: "Bearer nested-secret",
            path: "/tmp/cache/nested",
          },
        },
      ],
      content: {
        kind: "ticket_attachment_content",
        id: "ta_456",
        trust: "untrusted_external_content",
        available: true,
        directDownloadUrl: "https://example.test/direct",
        bearerToken: "Bearer secret",
        cachePath: "/tmp/cache/content",
      },
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
      content: {
        kind: "ticket_attachment_content",
        id: "ta_456",
        trust: "untrusted_external_content",
        available: true,
      },
    });
    expect(JSON.stringify(shaped)).not.toContain("https://");
    expect(JSON.stringify(shaped)).not.toContain("Bearer");
    expect(JSON.stringify(shaped)).not.toContain("/tmp/cache");
    expect(JSON.stringify(shaped)).not.toContain("authorization");
  });

  it("redacts unsafe scalar values even under otherwise allowed result keys", () => {
    const shaped = safeTicketAttachmentResult({
      attachments: [
        {
          provider: "jira",
          id: "https://example.test/raw-id",
          fileName: "Bearer direct-secret",
          contentPointer: {
            id: "ta_123",
            available: true,
          },
        },
        {
          provider: "linear",
          id: "ta_456",
          fileName: "evidence.txt",
          contentPointer: {
            id: "https://example.test/direct-download",
          },
        },
        {
          provider: "click_up",
          id: "ta_789",
          fileName: "/tmp/cache/content",
          contentPointer: {
            id: "ta_789",
          },
        },
      ],
    });

    expect(shaped).toEqual({
      attachments: [
        {
          provider: "jira",
          contentPointer: {
            id: "ta_123",
            available: true,
          },
        },
        {
          provider: "linear",
          id: "ta_456",
          fileName: "evidence.txt",
          contentPointer: {},
        },
        {
          provider: "click_up",
          id: "ta_789",
          contentPointer: {
            id: "ta_789",
          },
        },
      ],
    });
    expect(JSON.stringify(shaped)).not.toContain("https://");
    expect(JSON.stringify(shaped)).not.toContain("Bearer");
    expect(JSON.stringify(shaped)).not.toContain("/tmp/cache");
  });

  it("lets a materialized contentPath survive redaction even under /Users/", () => {
    const shaped = safeTicketAttachmentResult({
      content: {
        kind: "ticket_attachment_content",
        id: "ta_456",
        trust: "untrusted_external_content",
        available: true,
        contentPath:
          "/Users/agent/Library/Application Support/com.ralphx.app/ticket_attachments/ta/content.txt",
      },
    });

    expect(shaped).toEqual({
      content: {
        kind: "ticket_attachment_content",
        id: "ta_456",
        trust: "untrusted_external_content",
        available: true,
        contentPath:
          "/Users/agent/Library/Application Support/com.ralphx.app/ticket_attachments/ta/content.txt",
      },
    });
  });

  it("lets small inline contentText survive redaction", () => {
    const shaped = safeTicketAttachmentResult({
      content: {
        kind: "ticket_attachment_content",
        id: "ta_456",
        trust: "untrusted_external_content",
        available: true,
        contentText: "Steps to reproduce:\n1. Open the app\n2. Click save",
      },
    });

    expect(shaped).toEqual({
      content: {
        kind: "ticket_attachment_content",
        id: "ta_456",
        trust: "untrusted_external_content",
        available: true,
        contentText: "Steps to reproduce:\n1. Open the app\n2. Click save",
      },
    });
  });

  it("still redacts a provider URL carried on any key other than contentPath", () => {
    const shaped = safeTicketAttachmentResult({
      content: {
        kind: "ticket_attachment_content",
        id: "ta_456",
        trust: "untrusted_external_content",
        available: true,
        contentText: "See https://example.test/download?token=secret for the original",
      },
    });

    expect(shaped).toEqual({
      content: {
        kind: "ticket_attachment_content",
        id: "ta_456",
        trust: "untrusted_external_content",
        available: true,
      },
    });
  });

  it("still redacts /Users/ under an unrelated allowed key", () => {
    const shaped = safeTicketAttachmentResult({
      attachments: [
        {
          provider: "jira",
          id: "ta_123",
          fileName: "/Users/agent/Library/Application Support/leak.txt",
        },
      ],
    });

    expect(shaped).toEqual({
      attachments: [
        {
          provider: "jira",
          id: "ta_123",
        },
      ],
    });
  });

  it("identifies only ticket attachment tool names", () => {
    expect(isTicketAttachmentToolName("list_ticket_attachments")).toBe(true);
    expect(isTicketAttachmentToolName("fetch_ticket_attachment")).toBe(true);
    expect(isTicketAttachmentToolName("get_task_context")).toBe(false);
  });
});
