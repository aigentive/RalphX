import { Tool } from "@modelcontextprotocol/sdk/types.js";

import {
  buildProjectSkillPipelineTransportHeaders,
  buildRuntimeIdentityTransportHeaders,
  buildRuntimeTransportHeaders,
} from "./runtime-context.js";
import type { RuntimeContext } from "./runtime-context.js";
import type { TauriCallOptions } from "./tauri-client.js";

const PIPELINE_VALUES = [
  "planning",
  "verification",
  "review",
  "execution",
  "merge",
];

const PROJECT_ID = {
  type: "string",
  description: "The active project ID from RALPHX_PROJECT_ID.",
};

const CONTENT_PROPERTIES = {
  project_id: PROJECT_ID,
  title: {
    type: "string",
    maxLength: 120,
    description: "A concise procedural skill title.",
  },
  bucket: {
    type: "string",
    enum: PIPELINE_VALUES,
    description: "The pipeline bucket where this guidance applies.",
  },
  stage: {
    type: "string",
    enum: PIPELINE_VALUES,
    description: "The pipeline stage where this guidance applies.",
  },
  scope_paths: {
    type: "array",
    items: { type: "string" },
    description: "Project-relative path or glob scopes; use an empty array for project-wide guidance.",
  },
  compact_guidance: {
    type: "string",
    maxLength: 400,
    description: "Compact guidance suitable for runtime injection.",
  },
  body_markdown: {
    type: "string",
    maxLength: 32000,
    description: "Complete reusable procedural guidance in Markdown.",
  },
  predicted_effect: {
    type: "string",
    maxLength: 600,
    description: "The concrete behavior or outcome this skill should improve.",
  },
};

const CONTENT_REQUIRED = [
  "project_id",
  "title",
  "bucket",
  "stage",
  "scope_paths",
  "compact_guidance",
  "body_markdown",
  "predicted_effect",
];

export const LEARNED_SKILL_TOOLS: Tool[] = [
  {
    name: "list_project_skills",
    description:
      "List approved or staged learned project skills for the active project. " +
      "Read-only; use to inspect repository-backed learned procedural guidance before applying it.",
    inputSchema: {
      type: "object",
      properties: {
        project_id: PROJECT_ID,
        status: {
          type: "string",
          enum: ["staged", "approved", "rejected", "archived", "retired"],
          description: "Optional lifecycle status filter. Use approved for normal runtime guidance.",
        },
        include_archived: {
          type: "boolean",
          description: "Include archived or retired skills. Defaults to false.",
        },
        stage: {
          type: "string",
          description: "Optional pipeline stage filter.",
        },
        bucket: {
          type: "string",
          description: "Optional pipeline bucket filter.",
        },
        scope_path: {
          type: "string",
          description: "Optional active path for backend scope filtering.",
        },
      },
      required: ["project_id"],
      additionalProperties: false,
    },
  },
  {
    name: "get_project_skill",
    description:
      "Fetch one learned project skill by stable project_skill_id within the active project. Read-only; returns full guidance and provenance.",
    inputSchema: {
      type: "object",
      properties: {
        project_id: PROJECT_ID,
        project_skill_id: {
          type: "string",
          description: "Stable project skill ID returned by list_project_skills.",
        },
      },
      required: ["project_id", "project_skill_id"],
      additionalProperties: false,
    },
  },
  {
    name: "upsert_project_skill",
    description:
      "Create a staged project skill or update its canonical title/bucket/stage match. Runtime attribution is supplied by RalphX.",
    inputSchema: {
      type: "object",
      properties: CONTENT_PROPERTIES,
      required: CONTENT_REQUIRED,
      additionalProperties: false,
    },
  },
  {
    name: "patch_project_skill",
    description:
      "Patch a known project skill through the canonical resolution and versioning flow. Runtime attribution is supplied by RalphX.",
    inputSchema: {
      type: "object",
      properties: {
        project_skill_id: {
          type: "string",
          description: "Stable project skill ID to revise.",
        },
        ...CONTENT_PROPERTIES,
      },
      required: ["project_skill_id", ...CONTENT_REQUIRED],
      additionalProperties: false,
    },
  },
  {
    name: "retire_project_skill",
    description:
      "Retire an unpinned staged, approved, or stale project skill. Repeating an already-retired request is safe.",
    inputSchema: {
      type: "object",
      properties: {
        project_id: PROJECT_ID,
        project_skill_id: {
          type: "string",
          description: "Stable project skill ID to retire.",
        },
      },
      required: ["project_id", "project_skill_id"],
      additionalProperties: false,
    },
  },
];

export const LEARNED_SKILL_TOOL_NAMES = LEARNED_SKILL_TOOLS.map((tool) => tool.name);

const ENDPOINT_BY_TOOL: Record<string, string> = {
  list_project_skills: "project_skills/list",
  get_project_skill: "project_skills/get",
  upsert_project_skill: "project_skills/upsert",
  patch_project_skill: "project_skills/patch",
  retire_project_skill: "project_skills/retire",
};

const WRITE_TOOLS = new Set([
  "upsert_project_skill",
  "patch_project_skill",
  "retire_project_skill",
]);
const READ_TOOLS = new Set(["list_project_skills", "get_project_skill"]);

export function learnedSkillEndpoint(toolName: string): string {
  const endpoint = ENDPOINT_BY_TOOL[toolName];
  if (!endpoint) {
    throw new Error(`Unknown learned project skill tool: ${toolName}`);
  }
  return endpoint;
}

export function learnedSkillTransportOptions(
  toolName: string,
  runtimeContext: RuntimeContext
): TauriCallOptions | undefined {
  if (!WRITE_TOOLS.has(toolName)) {
    if (!READ_TOOLS.has(toolName)) return undefined;
    const headers = {
      ...(buildRuntimeTransportHeaders(runtimeContext) ?? {}),
      ...(buildRuntimeIdentityTransportHeaders(runtimeContext) ?? {}),
    };
    return Object.keys(headers).length > 0 ? { headers } : undefined;
  }
  const headers = buildProjectSkillPipelineTransportHeaders(runtimeContext);
  return headers ? { headers } : undefined;
}
