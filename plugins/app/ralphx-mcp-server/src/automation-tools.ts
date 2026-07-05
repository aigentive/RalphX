import { Tool } from "@modelcontextprotocol/sdk/types.js";
import type { TauriCallOptions } from "./tauri-client.js";

type TauriPost = (
  path: string,
  body: Record<string, unknown>,
  options?: TauriCallOptions
) => Promise<unknown>;

export type AutomationSetupToolRuntimeContext = {
  conversationId?: string;
};

export const AUTOMATION_SETUP_TOOLS: Tool[] = [
  {
    name: "get_automation",
    description:
      "Read the automation record and run list bound to the current automation setup conversation. " +
      "The backend resolves ownership from the caller conversation; do not pass an automation id.",
    inputSchema: {
      type: "object",
      properties: {},
      required: [],
    },
  },
  {
    name: "update_automation",
    description:
      "Update exposed settings for the automation bound to the current setup conversation. " +
      "The backend resolves ownership from the caller conversation; do not pass an automation id.",
    inputSchema: {
      type: "object",
      properties: {
        name: {
          type: "string",
          description: "Optional automation display name.",
        },
        max_runs: {
          type: "integer",
          description: "Optional positive maximum number of automation runs.",
        },
        max_consecutive_failures: {
          type: "integer",
          description:
            "Optional positive maximum number of consecutive failures before pausing.",
        },
      },
      required: [],
    },
  },
  {
    name: "finalize_automation",
    description:
      "Activate the draft automation bound to the current setup conversation after backend validation passes. " +
      "The backend resolves ownership from the caller conversation; do not pass an automation id.",
    inputSchema: {
      type: "object",
      properties: {},
      required: [],
    },
  },
];

const AUTOMATION_SETUP_TOOL_NAMES = new Set(
  AUTOMATION_SETUP_TOOLS.map((tool) => tool.name)
);

const CALLER_SESSION_ID_HEADER = "X-RalphX-Caller-Session-Id";
const UPDATE_AUTOMATION_FIELDS = [
  "name",
  "max_runs",
  "max_consecutive_failures",
] as const;

export function isAutomationSetupToolName(name: string): boolean {
  return AUTOMATION_SETUP_TOOL_NAMES.has(name);
}

export async function callAutomationSetupTool(
  name: string,
  callTauri: TauriPost,
  args: unknown,
  runtimeContext?: AutomationSetupToolRuntimeContext
): Promise<unknown> {
  const headers = automationSetupHeaders(name, runtimeContext);

  switch (name) {
    case "get_automation":
      return callTauri("get_automation", {}, { headers });
    case "update_automation":
      return callTauri(
        "update_automation",
        updateAutomationPayload(args),
        { headers }
      );
    case "finalize_automation":
      return callTauri("finalize_automation", {}, { headers });
    default:
      throw new Error(`Unsupported automation setup tool: ${name}`);
  }
}

function automationSetupHeaders(
  toolName: string,
  runtimeContext?: AutomationSetupToolRuntimeContext
): Record<string, string> {
  const conversationId = runtimeContext?.conversationId?.trim() ?? "";
  if (conversationId.length === 0) {
    throw new Error(
      `${toolName} requires the current setup conversation id from the RalphX MCP runtime context.`
    );
  }

  return {
    [CALLER_SESSION_ID_HEADER]: conversationId,
  };
}

function updateAutomationPayload(args: unknown): Record<string, unknown> {
  const input = args && typeof args === "object"
    ? (args as Record<string, unknown>)
    : {};
  const payload: Record<string, unknown> = {};

  for (const field of UPDATE_AUTOMATION_FIELDS) {
    if (Object.prototype.hasOwnProperty.call(input, field)) {
      payload[field] = input[field];
    }
  }

  return payload;
}
