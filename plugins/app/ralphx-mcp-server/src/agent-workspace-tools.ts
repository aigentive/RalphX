/**
 * Agent workspace MCP tool definitions.
 *
 * These are intentionally separate from task-pipeline workflow tools.
 */

import { Tool } from "@modelcontextprotocol/sdk/types.js";

export const AGENT_WORKSPACE_TOOLS: Tool[] = [
  {
    name: "complete_agent_workspace_repair",
    description:
      "Signal that an agent workspace publish/update repair has been committed, then let RalphX verify the repair and automatically retry publishing. " +
      "Call this only after the workspace branch contains the current base, the repair is committed, and the worktree is clean.",
    inputSchema: {
      type: "object",
      properties: {
        conversation_id: {
          type: "string",
          description: "The agent workspace conversation ID from the repair prompt",
        },
        repair_commit_sha: {
          type: "string",
          description: "Full 40-character SHA of the current workspace HEAD (from `git rev-parse HEAD`)",
        },
        resolved_base_ref: {
          type: "string",
          description: "The base ref that was resolved into the workspace branch",
        },
        resolved_base_commit: {
          type: "string",
          description: "Full 40-character SHA of the resolved base ref",
        },
        summary: {
          type: "string",
          description: "Brief summary of the repair performed",
        },
      },
      required: [
        "conversation_id",
        "repair_commit_sha",
        "resolved_base_ref",
        "resolved_base_commit",
        "summary",
      ],
    },
  },
  {
    name: "submit_agent_workspace_pr_description",
    description:
      "Submit the completed pull request title/body for an agent workspace publish. " +
      "Call this exactly once after writing a reviewer-focused body that follows the supplied pull request template.",
    inputSchema: {
      type: "object",
      properties: {
        conversation_id: {
          type: "string",
          description: "The agent workspace conversation ID from the PR description prompt",
        },
        title: {
          type: "string",
          description: "Optional pull request title. Omit unless the prompt context supports a better title.",
        },
        body_markdown: {
          type: "string",
          description: "Complete Markdown pull request body following the supplied template",
        },
      },
      required: ["conversation_id", "body_markdown"],
    },
  },
];
