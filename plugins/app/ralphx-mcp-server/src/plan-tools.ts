/**
 * MCP tool definitions for plan artifact management
 * Used by ralphx-ideation agent to create and manage implementation plans
 */

import { Tool } from "@modelcontextprotocol/sdk/types.js";

/**
 * Plan artifact tools for ralphx-ideation agent
 * All tools are proxies that forward to Tauri backend via HTTP
 */
export const PLAN_TOOLS: Tool[] = [
  {
    name: "create_plan_artifact",
    description:
      "Create a new implementation plan artifact linked to the ideation session. Use this when the user describes a complex feature that needs architectural planning before breaking into tasks. The plan is stored as a Specification artifact and can be referenced by task proposals. " +
      "For child sessions that inherited a parent's plan: calling this creates a completely independent plan for the child session — it does NOT modify or copy from the parent's plan.",
    inputSchema: {
      type: "object",
      properties: {
        session_id: {
          type: "string",
          description: "The ideation session ID (provided in context)",
        },
        title: {
          type: "string",
          description:
            "Plan title (e.g., 'Real-time Collaboration Implementation Plan')",
        },
        content: {
          type: "string",
          description:
            "Plan content in markdown format. Should include architecture decisions, data flow, key implementation details, and considerations.",
        },
      },
      required: ["session_id", "title", "content"],
    },
  },
  {
    name: "update_plan_artifact",
    description:
      "Update an existing implementation plan's content. Creates a NEW version with a new artifact ID (immutable version chain). Stale artifact IDs are auto-resolved: you can pass any previous version's ID and it will resolve to the latest before updating. Linked proposals are automatically re-linked to the new version (plan_version_at_creation is preserved). The response includes `previous_artifact_id` and `session_id` for reference. You do NOT need to call get_session_plan between updates to refresh the ID. Caller-session routing for verification freeze bypass is derived automatically from live app context; do not pass it manually.",
    inputSchema: {
      type: "object",
      properties: {
        artifact_id: {
          type: "string",
          description:
            "The artifact ID of the plan to update. Can be any version ID — stale IDs are auto-resolved to the latest version.",
        },
        content: {
          type: "string",
          description:
            "Updated plan content in markdown format. This will create a new version of the artifact with a new ID.",
        },
      },
      required: ["artifact_id", "content"],
    },
  },
  {
    name: "link_proposals_to_plan",
    description:
      "Link multiple task proposals to an implementation plan. Use after creating proposals to establish the connection between the plan and its derived tasks. Stale artifact IDs are auto-resolved: you can pass any previous version's ID and it will resolve to the latest before linking. This enables traceability and allows the system to suggest updates when the plan changes.",
    inputSchema: {
      type: "object",
      properties: {
        proposal_ids: {
          type: "array",
          items: { type: "string" },
          description: "Array of proposal IDs to link to the plan",
        },
        artifact_id: {
          type: "string",
          description:
            "The plan artifact ID to link proposals to. Can be any version ID — stale IDs are auto-resolved to the latest version.",
        },
      },
      required: ["proposal_ids", "artifact_id"],
    },
  },
  {
    name: "get_session_plan",
    description:
      "Get the implementation plan artifact for the current ideation session, if one exists. Use to check if a plan has already been created before suggesting a new one. " +
      "Response includes an `is_inherited` boolean: if true, the plan was inherited from a parent session and is read-only — call create_plan_artifact to create an independent plan for this session.",
    inputSchema: {
      type: "object",
      properties: {
        session_id: {
          type: "string",
          description: "The ideation session ID",
        },
      },
      required: ["session_id"],
    },
  },
  {
    name: "submit_plan_complexity_assessment",
    description:
      "Persist a complexity assessment for the current approved Plan-mode artifact version. " +
      "Only the ralphx-utility-plan-complexity helper should call this after grading an approved plan.",
    inputSchema: {
      type: "object",
      properties: {
        session_id: {
          type: "string",
          description: "Planning session ID for the approved plan",
        },
        artifact_id: {
          type: "string",
          description: "Approved plan artifact ID",
        },
        artifact_version: {
          type: "integer",
          description: "Approved plan artifact version",
        },
        level: {
          type: "string",
          enum: ["trivial", "simple", "moderate", "complex", "very_complex"],
          description: "Overall plan complexity tier",
        },
        score: {
          type: "integer",
          minimum: 0,
          maximum: 100,
          description:
            "0-100 score where higher means proposal decomposition is more appropriate",
        },
        recommended_action: {
          type: "string",
          enum: ["implement_directly", "create_proposals"],
          description: "Recommended primary CTA for the approved plan",
        },
        confidence: {
          type: "number",
          minimum: 0,
          maximum: 1,
          description: "Confidence in the recommendation",
        },
        reason_summary: {
          type: "string",
          description: "Concise user-facing reason for the recommendation",
        },
        signals: {
          type: "object",
          description:
            "Compact signals used to grade the plan, such as dependent_steps, affected_areas, migration_risk, or ambiguity",
          additionalProperties: true,
        },
      },
      required: [
        "session_id",
        "artifact_id",
        "artifact_version",
        "level",
        "score",
        "recommended_action",
        "confidence",
        "reason_summary",
      ],
    },
  },
  {
    name: "complete_plan_verification",
    description:
      "Record exact-artifact verification proof for the current Verify Plan action. Call exactly once only after reviewing repository evidence, revising the linked plan where needed, and confirming the current plan is implementation-ready. The backend derives the run, conversation, planning session, and current artifact from trusted runtime context; pass no bookkeeping fields.",
    inputSchema: {
      type: "object",
      properties: {},
      required: [],
    },
  },
  {
    name: "get_plan_verification",
    description:
      "Read the current plan's Verify Plan action status and exact-artifact proof. Pass session_id when the active runtime context is not an ideation session.",
    inputSchema: {
      type: "object",
      properties: {
        session_id: {
          type: "string",
          description: "Planning session ID. Optional in an ideation-session runtime.",
        },
      },
      required: [],
    },
  },
  {
    name: "edit_plan_artifact",
    description:
      "Apply anchor-based edit operations to an existing implementation plan. More token-efficient than update_plan_artifact for targeted changes — only send the text to find and replace, not the entire plan content. Each edit finds the first occurrence of old_text and replaces it with new_text. Stale artifact IDs are auto-resolved to the latest version. Edits are applied sequentially; if any edit fails (old_text not found or ambiguous), the entire operation is rejected with details of which edit failed. Caller-session routing for verification freeze bypass is derived automatically from live app context; do not pass it manually.",
    inputSchema: {
      type: "object",
      properties: {
        artifact_id: {
          type: "string",
          description:
            "The artifact ID of the plan to edit. Can be any version ID — stale IDs are auto-resolved to the latest version.",
        },
        edits: {
          type: "array",
          minItems: 1,
          maxItems: 20,
          items: {
            type: "object",
            properties: {
              old_text: {
                type: "string",
                minLength: 1,
                description:
                  "The exact text to find in the plan. Must be unique within the plan content to avoid ambiguous replacements.",
              },
              new_text: {
                type: "string",
                description:
                  "The replacement text. Can be empty string to delete the matched text.",
              },
            },
            required: ["old_text", "new_text"],
          },
          description:
            "List of edit operations to apply sequentially. Each operation finds old_text and replaces with new_text.",
        },
      },
      required: ["artifact_id", "edits"],
    },
  },
];
