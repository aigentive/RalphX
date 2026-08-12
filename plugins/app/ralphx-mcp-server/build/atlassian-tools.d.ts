/**
 * Atlassian (Jira + Confluence) MCP tool definitions.
 *
 * These proxy to the RalphX backend, which reuses the already-configured
 * Atlassian integration credentials. Access is tiered per agent routing role,
 * so which of these tools an agent can see is decided at spawn time and
 * re-checked by the backend on every call.
 *
 * Schemas never accept run, conversation, or orchestration ids: caller identity
 * is transport-owned and injected as headers. No schema names a credential,
 * token, site URL, or cloud id.
 */
import { Tool } from "@modelcontextprotocol/sdk/types.js";
import type { TauriCallOptions } from "./tauri-client.js";
type TauriPost = (path: string, body: Record<string, unknown>, options?: TauriCallOptions) => Promise<unknown>;
export type AtlassianToolRuntimeContext = {
    conversationId?: string;
    agentRunId?: string;
};
export declare const ATLASSIAN_TOOLS: Tool[];
export declare function isAtlassianToolName(name: string): boolean;
/**
 * Dispatch an Atlassian tool call to its backend endpoint.
 *
 * The payload is forwarded as-is; the backend owns validation, tier
 * enforcement, and credential resolution. Caller identity travels in headers,
 * never in the payload.
 */
export declare function callAtlassianTool(name: string, callTauri: TauriPost, args: unknown, runtimeContext?: AtlassianToolRuntimeContext): Promise<unknown>;
export {};
//# sourceMappingURL=atlassian-tools.d.ts.map