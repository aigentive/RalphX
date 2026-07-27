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
      "Atomically create the linked plan bundle: a concise overview plus a detailed, codebase-grounded implementation blueprint. Both are Specification artifacts and both must be supplied. " +
      "For child sessions that inherited a parent's bundle, this creates a completely independent bundle for the child.",
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
            "Concise, human-oriented plan overview in markdown.",
        },
        blueprint_title: {
          type: "string",
          description:
            "Optional blueprint title. Defaults to '<overview title> — Implementation Blueprint'.",
        },
        blueprint_content: {
          type: "string",
          description:
            "Self-contained implementation blueprint with ordered steps, exact files and symbols, state/data effects, failure behavior, integration wiring, and focused proof obligations. It must not leave architecture-discovery steps unresolved.",
        },
      },
      required: ["session_id", "title", "content", "blueprint_content"],
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
      "Link multiple task proposals to the current implementation plan bundle. The backend derives and records the exact Overview and Blueprint pair atomically. Use after creating proposals to establish the connection between the plan and its derived tasks. Stale Overview artifact IDs are auto-resolved to the latest current Overview before linking.",
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
            "The Overview artifact ID to link proposals from. Can be any version ID — stale IDs are auto-resolved to the latest current Overview and its exact Blueprint.",
        },
      },
      required: ["proposal_ids", "artifact_id"],
    },
  },
  {
    name: "get_session_plan",
    description:
      "Get the current plan bundle. The overview remains at the top level for compatibility; `blueprint_artifact` contains the detailed implementation blueprint and `plan_target_id` identifies the exact current pair. " +
      "Read both before proposing, implementing, or reviewing work. `is_inherited` means the bundle is read-only.",
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
      "Persist a complexity assessment for the current approved plan bundle. " +
      "For v2 plans, both overview and blueprint ids/versions must match the approved pair.",
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
        blueprint_artifact_id: {
          type: "string",
          description: "Approved implementation blueprint artifact ID; required for v2 plans",
        },
        blueprint_artifact_version: {
          type: "integer",
          description: "Approved implementation blueprint version; required for v2 plans",
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
