/**
 * Permission handler for UI-based approval of tool calls
 *
 * This MCP tool is called by Claude CLI when it needs permission for a tool
 * that wasn't pre-approved via --allowedTools. It:
 * 1. Forwards the permission request to the Tauri backend
 * 2. Long-polls for user decision (5 minute timeout)
 * 3. Returns the decision to Claude CLI
 *
 * The Tauri backend emits a Tauri event that triggers the PermissionDialog
 * in the frontend, allowing the user to approve or deny the tool call.
 */

import { Tool } from "@modelcontextprotocol/sdk/types.js";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { safeError } from "./redact.js";
import { buildTauriApiUrl } from "./tauri-client.js";
import {
  buildRuntimeTransportHeaders,
  type RuntimeContext,
} from "./runtime-context.js";
import {
  getConfiguredFilesystemReadRoots,
  isWithin,
  normalizePathLike,
} from "./path-policy.js";
const SAFE_READONLY_BASH_COMMANDS = new Set([
  "ls",
  "cat",
  "find",
  "rg",
  "grep",
  "head",
  "sed",
  "wc",
  "pwd",
  "echo",
]);

/**
 * MCP tool definition for permission handling
 * This tool is NOT scoped by agent type - it's always available
 */
export const permissionRequestTool: Tool = {
  name: "permission_request",
  description:
    "Internal tool for handling permission prompts from Claude CLI. This tool is called automatically when Claude needs permission for a non-pre-approved tool.",
  inputSchema: {
    type: "object",
    properties: {
      tool_name: {
        type: "string",
        description: "Name of the tool requesting permission",
      },
      tool_input: {
        type: "object",
        description: "Input arguments for the tool",
      },
      context: {
        type: "string",
        description: "Additional context about why the tool is being called",
      },
    },
    required: ["tool_name", "tool_input"],
  },
};

interface PermissionDecision {
  decision: "allow" | "deny";
  message?: string;
}

function getStringField(
  input: Record<string, unknown>,
  keys: readonly string[]
): string | undefined {
  for (const key of keys) {
    const value = input[key];
    if (typeof value === "string" && value.length > 0) {
      return value;
    }
  }
  return undefined;
}

function isSensitivePath(targetPath: string): boolean {
  const normalized = normalizePathLike(targetPath);
  const basename = path.basename(normalized);
  const parts = normalized.split(path.sep);
  return (
    basename === ".env" ||
    basename.startsWith(".env.") ||
    parts.includes(".git")
  );
}

function trustedRoots(): string[] {
  const roots = new Set<string>();
  const pwd = process.env.PWD;
  if (pwd) roots.add(normalizePathLike(pwd));
  roots.add(normalizePathLike(process.cwd()));
  roots.add(path.join(os.homedir(), ".reefagent", "agents"));
  return [...roots];
}

function isTrustedReadPath(targetPath: string): boolean {
  const normalized = normalizePathLike(targetPath);
  if (isSensitivePath(normalized)) return false;
  if (isTrustedClaudeProjectMemoryPath(normalized)) return true;

  for (const root of trustedRoots()) {
    if (isWithin(root, normalized)) return true;
  }

  return false;
}

function isTrustedReadRootPath(targetPath: string): boolean {
  try {
    const realTarget = fs.realpathSync(normalizePathLike(targetPath));
    if (isSensitivePath(realTarget)) return false;

    return getConfiguredFilesystemReadRoots().some((root) => {
      try {
        return isWithin(fs.realpathSync(root), realTarget);
      } catch {
        return false;
      }
    });
  } catch {
    return false;
  }
}

function isTrustedClaudeProjectMemoryPath(targetPath: string): boolean {
  const normalized = normalizePathLike(targetPath);
  const memoryRoot = path.join(os.homedir(), ".claude", "projects");
  const ext = path.extname(normalized).toLowerCase();

  if (!isWithin(memoryRoot, normalized)) return false;
  if (ext !== ".md") return false;
  if (isSensitivePath(normalized)) return false;

  const parts = normalized.split(path.sep);
  return parts.includes("memory");
}

function extractGlobRoot(pattern: string): string | null {
  const wildcardIndex = pattern.search(/[*?[{]/);
  if (wildcardIndex === -1) {
    return pattern;
  }

  const prefix = pattern.slice(0, wildcardIndex);
  if (!prefix) return null;

  if (prefix.endsWith(path.sep) || prefix.endsWith("/")) {
    return prefix;
  }

  return path.dirname(prefix);
}

function shellSegments(command: string): string[] {
  return command
    .split(/\s*(?:&&|\|\||;|\|)\s*/)
    .map((segment) => segment.trim())
    .filter((segment) => segment.length > 0);
}

function tokenizeShellSegment(segment: string): string[] {
  return segment.match(/"(?:[^"\\]|\\.)*"|'(?:[^'\\]|\\.)*'|\S+/g) ?? [];
}

function unquote(token: string): string {
  if (
    (token.startsWith("\"") && token.endsWith("\"")) ||
    (token.startsWith("'") && token.endsWith("'"))
  ) {
    return token.slice(1, -1);
  }
  return token;
}

function isPathToken(token: string): boolean {
  return (
    token.startsWith("/") ||
    token.startsWith("~/") ||
    token.startsWith("./") ||
    token.startsWith("../")
  );
}

function segmentIsTrustedReadonlyBash(segment: string): boolean {
  const rawTokens = tokenizeShellSegment(segment).map(unquote);
  if (rawTokens.length === 0) return true;

  let index = 0;
  while (
    index < rawTokens.length &&
    /^[A-Za-z_][A-Za-z0-9_]*=.*/.test(rawTokens[index] ?? "")
  ) {
    index += 1;
  }

  const command = rawTokens[index];
  if (!command || !SAFE_READONLY_BASH_COMMANDS.has(command)) {
    return false;
  }

  if (command === "echo" || command === "pwd") {
    return true;
  }

  const pathTokens = rawTokens
    .slice(index + 1)
    .filter((token) => isPathToken(token) && !token.includes(">"));

  if (pathTokens.length === 0) {
    return isTrustedReadPath(process.env.PWD ?? process.cwd());
  }

  return pathTokens.every((token) => isTrustedReadPath(token));
}

export function shouldAutoApprovePermission(
  toolName: string,
  toolInput: Record<string, unknown>
): boolean {
  switch (toolName) {
    case "Write":
    case "Edit": {
      const targetPath = getStringField(toolInput, ["file_path", "filePath", "path"]);
      return Boolean(targetPath && isTrustedClaudeProjectMemoryPath(targetPath));
    }
    case "Read": {
      const targetPath = getStringField(toolInput, ["file_path", "filePath", "path"]);
      return Boolean(
        targetPath &&
          (isTrustedReadPath(targetPath) || isTrustedReadRootPath(targetPath))
      );
    }
    case "LS":
    case "Grep": {
      const targetPath = getStringField(toolInput, ["file_path", "filePath", "path"]);
      return Boolean(
        targetPath &&
          (isTrustedReadPath(targetPath) || isTrustedReadRootPath(targetPath))
      );
    }
    case "Glob": {
      const pattern = getStringField(toolInput, ["pattern"]);
      const root = pattern ? extractGlobRoot(pattern) : null;
      return Boolean(
        root && (isTrustedReadPath(root) || isTrustedReadRootPath(root))
      );
    }
    case "Bash": {
      const command = getStringField(toolInput, ["command"]);
      return Boolean(command) && shellSegments(command!).every(segmentIsTrustedReadonlyBash);
    }
    default:
      return false;
  }
}

export function normalizePermissionToolInput(
  toolName: string,
  toolInput: Record<string, unknown>
): Record<string, unknown> {
  const normalized = { ...toolInput };

  if (toolName === "Write" || toolName === "Edit" || toolName === "Read") {
    const path = getStringField(toolInput, ["file_path", "filePath", "path"]);
    if (path) {
      if (normalized.file_path === undefined) normalized.file_path = path;
      if (normalized.filePath === undefined) normalized.filePath = path;
      if (normalized.path === undefined) normalized.path = path;
    }
  }

  if (toolName === "Edit") {
    const oldString = getStringField(toolInput, ["old_string", "oldString"]);
    if (oldString) {
      if (normalized.old_string === undefined) normalized.old_string = oldString;
      if (normalized.oldString === undefined) normalized.oldString = oldString;
    }

    const newString = getStringField(toolInput, ["new_string", "newString"]);
    if (newString) {
      if (normalized.new_string === undefined) normalized.new_string = newString;
      if (normalized.newString === undefined) normalized.newString = newString;
    }
  }

  return normalized;
}

/** Normalize permission args from CLI (may send snake_case, camelCase, or name/input). */
function normalizePermissionArgs(
  args: Record<string, unknown>
): { tool_name: string; tool_input: Record<string, unknown>; context?: string } {
  const tool_name =
    (args.tool_name as string) ??
    (args.toolName as string) ??
    (args.name as string) ??
    "";
  const raw_input = args.tool_input ?? args.toolInput ?? args.input;
  const tool_input =
    raw_input != null && typeof raw_input === "object" && !Array.isArray(raw_input)
      ? (raw_input as Record<string, unknown>)
      : {};
  const context =
    (args.context as string) ?? (args.reason as string) ?? undefined;
  return { tool_name, tool_input, context };
}

/**
 * Handle a permission request by forwarding to Tauri backend
 * and waiting for user decision via long-poll.
 *
 * Flow:
 * 1. POST to /api/permission/request - registers request, emits Tauri event
 * 2. GET /api/permission/await/:id - blocks until user decides (5 min timeout)
 * 3. Return decision to Claude CLI
 *
 * @param args - Tool call details from Claude CLI (shape may vary)
 * @returns MCP tool result with decision (behavior + updatedInput / message)
 */
export async function handlePermissionRequest(
  args: Record<string, unknown>,
  runtimeContext: Pick<RuntimeContext, "conversationId"> = {}
): Promise<{ content: Array<{ type: "text"; text: string }> }> {
  const { tool_name, tool_input, context } = normalizePermissionArgs(args);
  const normalizedToolInput = normalizePermissionToolInput(tool_name, tool_input);

  if (!tool_name) {
    safeError("[RalphX MCP] Permission request missing tool name", args);
    return {
      content: [
        {
          type: "text",
          text: JSON.stringify({
            behavior: "deny" as const,
            message: "Permission request missing tool name",
          }),
        },
      ],
    };
  }

  safeError(`[RalphX MCP] Permission request for tool: ${tool_name}`);

  if (shouldAutoApprovePermission(tool_name, normalizedToolInput)) {
    safeError(`[RalphX MCP] Auto-allowing safe read-only permission for tool: ${tool_name}`);
    return {
      content: [
        {
          type: "text",
          text: JSON.stringify({
            behavior: "allow" as const,
            updatedInput: normalizedToolInput,
          }),
        },
      ],
    };
  }

  // 1. Register permission request with Tauri backend
  let request_id: string;
  try {
    interface PermissionRequestBody {
      tool_name: string;
      tool_input: Record<string, unknown>;
      context?: string;
      agent_type?: string;
      task_id?: string;
      context_type?: string;
      context_id?: string;
    }

    const agentType = process.env.RALPHX_AGENT_TYPE;
    const taskId = process.env.RALPHX_TASK_ID;
    const contextType = process.env.RALPHX_CONTEXT_TYPE;
    const contextId = process.env.RALPHX_CONTEXT_ID;

    const body: PermissionRequestBody = {
      tool_name,
      tool_input: normalizedToolInput,
    };
    if (context !== undefined && context !== "") body.context = context;
    if (agentType && agentType !== "unknown") body.agent_type = agentType;
    if (taskId) body.task_id = taskId;
    if (contextType) body.context_type = contextType;
    if (contextId) body.context_id = contextId;

    const registerResponse = await fetch(
      buildTauriApiUrl("permission/request"),
      {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
          ...(buildRuntimeTransportHeaders(runtimeContext) ?? {}),
        },
        body: JSON.stringify(body),
      }
    );

    if (!registerResponse.ok) {
      throw new Error(
        `Failed to register permission request: ${registerResponse.statusText}`
      );
    }

    const result = (await registerResponse.json()) as { request_id: string };
    request_id = result.request_id;

    safeError(
      `[RalphX MCP] Permission request registered: ${request_id}`
    );
  } catch (error) {
    safeError(`[RalphX MCP] Failed to register permission request:`, error);
    return {
      content: [
        {
          type: "text",
          text: JSON.stringify({
            behavior: "deny" as const,
            message: `Failed to register permission request: ${
              error instanceof Error ? error.message : String(error)
            }`,
          }),
        },
      ],
    };
  }

  // 2. Long-poll for user decision (5 minute timeout)
  const controller = new AbortController();
  const timeoutId = setTimeout(() => controller.abort(), 5 * 60 * 1000);

  try {
    const decisionResponse = await fetch(
      buildTauriApiUrl(`permission/await/${encodeURIComponent(request_id)}`),
      {
        method: "GET",
        signal: controller.signal,
      }
    );

    clearTimeout(timeoutId);

    if (!decisionResponse.ok) {
      if (decisionResponse.status === 408) {
        // Timeout - treat as deny
        safeError(
          `[RalphX MCP] Permission request ${request_id} timed out`
        );
        return {
          content: [
            {
              type: "text",
              text: JSON.stringify({
                behavior: "deny" as const,
                message:
                  "Permission request timed out waiting for user response",
              }),
            },
          ],
        };
      }
      throw new Error(`Permission decision error: ${decisionResponse.statusText}`);
    }

    const decision = (await decisionResponse.json()) as PermissionDecision;

    safeError(
      `[RalphX MCP] Permission ${decision.decision} for tool: ${tool_name}`
    );

    // Claude CLI expects permission-prompt-tool result to be a union:
    // - allow: { behavior: "allow", updatedInput: <record> }
    // - deny:  { behavior: "deny", message: <string> }
    const payload =
      decision.decision === "allow"
        ? { behavior: "allow" as const, updatedInput: normalizedToolInput }
        : {
            behavior: "deny" as const,
            message:
              decision.message ?? "User denied the tool call",
          };

    return {
      content: [
        {
          type: "text",
          text: JSON.stringify(payload),
        },
      ],
    };
  } catch (error) {
    clearTimeout(timeoutId);
    if (error instanceof Error && error.name === "AbortError") {
      safeError(`[RalphX MCP] Permission request ${request_id} aborted`);
      return {
        content: [
          {
            type: "text",
            text: JSON.stringify({
              behavior: "deny" as const,
              message: "Permission request timed out",
            }),
          },
        ],
      };
    }
    safeError(`[RalphX MCP] Permission request error:`, error);
    throw error;
  }
}
