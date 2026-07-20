import { Tool } from "@modelcontextprotocol/sdk/types.js";
import type { TauriCallOptions } from "./tauri-client.js";

type TauriPost = (
  path: string,
  body: Record<string, unknown>,
  options?: TauriCallOptions
) => Promise<unknown>;
type TauriGet = (
  path: string,
  options?: TauriCallOptions
) => Promise<unknown>;

export type PersonaToolRuntimeContext = {
  conversationId?: string;
};

export const PERSONA_BUILDER_TOOLS: Tool[] = [
  {
    name: "save_persona_draft",
    description:
      "Create or update a persona draft owned by the current PersonaBuilder conversation. " +
      "Use the exact persona content to save; RalphX resolves ownership from the caller conversation. " +
      "A prose or Markdown-only response does not create a Persona; this tool must succeed before reporting a draft as ready.",
    inputSchema: {
      type: "object",
      properties: {
        draftId: {
          type: "string",
          description: "Optional existing draft ID when updating a draft.",
        },
        slug: {
          type: "string",
          description: "Stable lowercase persona slug.",
        },
        content: {
          type: "string",
          description: "Complete persona document content.",
        },
        sourceSessionId: {
          type: "string",
          description: "Optional source session ID recorded with a newly created draft.",
        },
      },
      required: ["slug", "content"],
    },
  },
  {
    name: "get_persona_draft",
    description: "Read a persona draft by its draft ID.",
    inputSchema: {
      type: "object",
      properties: {
        draft_id: {
          type: "string",
          description: "Persona draft ID to read.",
        },
      },
      required: ["draft_id"],
    },
  },
];

const PERSONA_TOOL_NAMES = new Set(PERSONA_BUILDER_TOOLS.map((tool) => tool.name));
const CALLER_SESSION_ID_HEADER = "X-RalphX-Caller-Session-Id";
const SAVE_PERSONA_DRAFT_FIELDS = [
  "draftId",
  "slug",
  "content",
  "sourceSessionId",
] as const;

export function isPersonaToolName(name: string): boolean {
  return PERSONA_TOOL_NAMES.has(name);
}

export async function callPersonaTool(
  name: string,
  callTauri: TauriPost,
  callTauriGet: TauriGet,
  args: unknown,
  runtimeContext?: PersonaToolRuntimeContext
): Promise<unknown> {
  const headers = personaToolHeaders(name, runtimeContext);

  switch (name) {
    case "save_persona_draft":
      return callTauri("save_persona_draft", savePersonaDraftPayload(args), { headers });
    case "get_persona_draft":
      return callTauriGet(`get_persona_draft/${personaDraftId(args)}`, { headers });
    default:
      throw new Error(`Unsupported persona tool: ${name}`);
  }
}

function personaToolHeaders(
  toolName: string,
  runtimeContext?: PersonaToolRuntimeContext
): Record<string, string> {
  const conversationId = runtimeContext?.conversationId?.trim() ?? "";
  if (conversationId.length === 0) {
    throw new Error(
      `${toolName} requires the current PersonaBuilder conversation id from the RalphX MCP runtime context.`
    );
  }

  return {
    [CALLER_SESSION_ID_HEADER]: conversationId,
  };
}

function savePersonaDraftPayload(args: unknown): Record<string, unknown> {
  const input = args && typeof args === "object"
    ? (args as Record<string, unknown>)
    : {};
  const payload: Record<string, unknown> = {};

  for (const field of SAVE_PERSONA_DRAFT_FIELDS) {
    if (Object.prototype.hasOwnProperty.call(input, field)) {
      payload[field] = input[field];
    }
  }

  return payload;
}

function personaDraftId(args: unknown): string {
  const draftId = args && typeof args === "object"
    ? (args as Record<string, unknown>).draft_id
    : undefined;
  if (typeof draftId !== "string" || draftId.trim().length === 0) {
    throw new Error("get_persona_draft requires a non-empty draft_id.");
  }

  return draftId;
}
