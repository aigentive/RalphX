/**
 * Agent workspace MCP tool definitions.
 *
 * These are intentionally separate from task-pipeline workflow tools.
 */

import { Tool } from "@modelcontextprotocol/sdk/types.js";

export const AGENT_WORKSPACE_TOOLS: Tool[] = [
  {
    name: "get_agent_workspace_publish_status",
    description:
      "Read the current publish status for an agent workspace conversation. " +
      "Use this before publishing when the user asks about PR/publication state.",
    inputSchema: {
      type: "object",
      properties: {
        conversation_id: {
          type: "string",
          description: "The agent workspace conversation ID",
        },
      },
      required: ["conversation_id"],
    },
  },
  {
    name: "check_agent_workspace_publish_readiness",
    description:
      "Check whether an agent workspace is ready to publish, including base freshness and local change state. " +
      "Call this only when the user asks to check publish readiness or base freshness; a base update may be recommended without blocking publish.",
    inputSchema: {
      type: "object",
      properties: {
        conversation_id: {
          type: "string",
          description: "The agent workspace conversation ID",
        },
      },
      required: ["conversation_id"],
    },
  },
  {
    name: "update_agent_workspace_from_base",
    description:
      "Update the current agent workspace branch from its configured base through RalphX. " +
      "If conflicts require repair, RalphX will route the workspace to the repair agent and will not publish automatically.",
    inputSchema: {
      type: "object",
      properties: {
        conversation_id: {
          type: "string",
          description: "The agent workspace conversation ID",
        },
        base_ref_kind: {
          type: "string",
          enum: ["project_default", "current_branch", "local_branch", "pull_request"],
          description: "Optional base selection kind. Omit to use the workspace's configured base.",
        },
        base_ref: {
          type: "string",
          description: "Optional explicit branch/ref when base_ref_kind requires it.",
        },
        base_display_name: {
          type: "string",
          description: "Optional user-facing label for the selected base.",
        },
      },
      required: ["conversation_id"],
    },
  },
  {
    name: "publish_agent_workspace",
    description:
      "Publish the current agent workspace through RalphX's Commit & Publish pipeline. " +
      "Call this only when the user explicitly asks to commit, publish, or open a PR for this workspace.",
    inputSchema: {
      type: "object",
      properties: {
        conversation_id: {
          type: "string",
          description: "The agent workspace conversation ID",
        },
      },
      required: ["conversation_id"],
    },
  },
  {
    name: "complete_agent_workspace_repair",
    description:
      "Signal that an agent workspace publish/update repair has been committed, then let RalphX verify the repair and continue the original workflow. " +
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
];
