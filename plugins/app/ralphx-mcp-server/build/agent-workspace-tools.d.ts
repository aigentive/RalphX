/**
 * Agent workspace MCP tool definitions.
 *
 * These are intentionally separate from task-pipeline workflow tools.
 */
import { Tool } from "@modelcontextprotocol/sdk/types.js";
type TauriPost = (path: string, body: Record<string, unknown>) => Promise<unknown>;
export declare const AGENT_WORKSPACE_TOOLS: Tool[];
export declare function callCompleteAgentWorkspaceRepairTool(callTauri: TauriPost, args: unknown): Promise<unknown>;
export declare function callSubmitAgentWorkspacePrDescriptionTool(callTauri: TauriPost, args: unknown): Promise<unknown>;
export {};
//# sourceMappingURL=agent-workspace-tools.d.ts.map