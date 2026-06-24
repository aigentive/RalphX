/**
 * Agent workspace MCP tool definitions.
 *
 * These are intentionally separate from task-pipeline workflow tools.
 */
import { Tool } from "@modelcontextprotocol/sdk/types.js";
type TauriPost = (path: string, body: Record<string, unknown>) => Promise<unknown>;
type TauriGet = (path: string) => Promise<unknown>;
export type AgentWorkspaceToolRuntimeContext = {
    parentConversationId?: string;
};
export declare const AGENT_WORKSPACE_TOOLS: Tool[];
export declare function isAgentWorkspaceToolName(name: string): boolean;
export declare function callAgentWorkspaceTool(name: string, callTauri: TauriPost, callTauriGet: TauriGet, args: unknown, runtimeContext?: AgentWorkspaceToolRuntimeContext): Promise<unknown>;
export declare function callGetAgentWorkspacePublishStatusTool(callTauriGet: TauriGet, args: unknown, runtimeContext?: AgentWorkspaceToolRuntimeContext): Promise<unknown>;
export declare function callCheckAgentWorkspacePublishReadinessTool(callTauriGet: TauriGet, args: unknown, runtimeContext?: AgentWorkspaceToolRuntimeContext): Promise<unknown>;
export declare function callUpdateAgentWorkspaceFromBaseTool(callTauri: TauriPost, args: unknown, runtimeContext?: AgentWorkspaceToolRuntimeContext): Promise<unknown>;
export declare function callPublishAgentWorkspaceTool(callTauri: TauriPost, args: unknown, runtimeContext?: AgentWorkspaceToolRuntimeContext): Promise<unknown>;
export declare function callGetAgentWorkspacePrFixContextTool(callTauriGet: TauriGet, args: unknown): Promise<unknown>;
export declare function callGetPrReviewContextTool(callTauriGet: TauriGet, args: unknown, runtimeContext?: AgentWorkspaceToolRuntimeContext): Promise<unknown>;
export declare function callProposePrReviewActionTool(callTauri: TauriPost, args: unknown, runtimeContext?: AgentWorkspaceToolRuntimeContext): Promise<unknown>;
export declare function callWritePrReviewArtifactTool(callTauri: TauriPost, args: unknown, runtimeContext?: AgentWorkspaceToolRuntimeContext): Promise<unknown>;
export declare function callCompletePrReviewRunTool(callTauri: TauriPost, args: unknown, runtimeContext?: AgentWorkspaceToolRuntimeContext): Promise<unknown>;
export declare function callReadAgentWorkspacePrCommentTool(callTauriGet: TauriGet, args: unknown): Promise<unknown>;
export declare function callCompleteAgentWorkspacePrFixTool(callTauri: TauriPost, args: unknown): Promise<unknown>;
export declare function callCompleteAgentWorkspaceRepairTool(callTauri: TauriPost, args: unknown): Promise<unknown>;
export declare function callSubmitAgentWorkspacePrDescriptionTool(callTauri: TauriPost, args: unknown): Promise<unknown>;
export {};
//# sourceMappingURL=agent-workspace-tools.d.ts.map