/**
 * MCP tool definitions for worker artifact context
 * Used by worker agent to fetch task context, implementation plans, and related artifacts
 */

import { Tool } from "@modelcontextprotocol/sdk/types.js";

/**
 * Worker context tools for worker agent
 * All tools are proxies that forward to Tauri backend via HTTP
 */
export const WORKER_CONTEXT_TOOLS: Tool[] = [
  {
    name: "get_task_context",
    description:
      "Fetch authoritative task context including source proposal, exact plan overview and implementation blueprint snapshots, blockers, scope drift, and related artifacts. Use when injected bootstrap context is absent, stale, incomplete, or blocked; fetch both artifact bodies before implementing or reviewing when blueprint_artifact is present.",
    inputSchema: {
      type: "object",
      properties: {
        task_id: {
          type: "string",
          description: "The ID of the task to get context for",
        },
      },
      required: ["task_id"],
    },
  },
  {
    name: "get_artifact",
    description:
      "Retrieve the full content of any artifact by ID. Use when task context or composer-selected artifact references provide a plan/artifact id and full content is needed.",
    inputSchema: {
      type: "object",
      properties: {
        artifact_id: {
          type: "string",
          description: "The artifact ID to fetch",
        },
      },
      required: ["artifact_id"],
    },
  },
  {
    name: "get_artifact_version",
    description:
      "Fetch a specific historical version of an artifact. Use when you need to access the plan as it existed when the task was created (referenced by plan_version_at_creation).",
    inputSchema: {
      type: "object",
      properties: {
        artifact_id: {
          type: "string",
          description: "The artifact ID",
        },
        version: {
          type: "number",
          description:
            "The version number to fetch (e.g., from plan_version_at_creation)",
        },
      },
      required: ["artifact_id", "version"],
    },
  },
  {
    name: "get_related_artifacts",
    description:
      "Get artifacts related to a specific artifact (e.g., research documents related to a plan, design documents referenced by the plan). Use to gather additional context for complex tasks.",
    inputSchema: {
      type: "object",
      properties: {
        artifact_id: {
          type: "string",
          description: "The artifact ID to find relations for",
        },
        relation_types: {
          type: "array",
          items: { type: "string" },
          description:
            "Optional: Filter by relation types: 'derived_from', 'references', 'supersedes'",
        },
      },
      required: ["artifact_id"],
    },
  },
  {
    name: "search_project_artifacts",
    description:
      "Search for artifacts in the project by query and optional type filter. Use when you need to find relevant context like research documents, design docs, or related implementations that aren't directly linked to the task.",
    inputSchema: {
      type: "object",
      properties: {
        project_id: {
          type: "string",
          description: "The project ID to search within (provided in context)",
        },
        query: {
          type: "string",
          description:
            "Search query (matches title and content). Be specific to find relevant artifacts.",
        },
        artifact_types: {
          type: "array",
          items: { type: "string" },
          description:
            "Optional: Filter by artifact types - 'specification', 'research', 'design_doc', 'decision', 'test_plan'",
        },
      },
      required: ["project_id", "query"],
    },
  },
];
