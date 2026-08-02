/**
 * Ideation-family MCP tool definitions
 */
import { Tool } from "@modelcontextprotocol/sdk/types.js";
import type { TauriCallOptions } from "./tauri-client.js";
type TauriPost = (path: string, body: Record<string, unknown>, options?: TauriCallOptions) => Promise<unknown>;
export type DelegateContextRuntimeContext = {
    conversationId?: string;
    agentRunId?: string;
};
export declare function callGetParentContextTool(callTauri: TauriPost, args: Record<string, unknown>, runtimeContext: DelegateContextRuntimeContext): Promise<unknown>;
export declare const IDEATION_TOOLS: Tool[];
export {};
//# sourceMappingURL=ideation-tools.d.ts.map