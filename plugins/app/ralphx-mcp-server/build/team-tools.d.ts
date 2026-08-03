import { Tool } from "@modelcontextprotocol/sdk/types.js";
import type { TauriCallOptions } from "./tauri-client.js";
type TauriPost = (path: string, body: Record<string, unknown>, options?: TauriCallOptions) => Promise<unknown>;
type TauriGet = (path: string, options?: TauriCallOptions) => Promise<unknown>;
export type TeamToolRuntimeContext = {
    conversationId?: string;
    agentRunId?: string;
};
export declare const TEAM_TOOLS: Tool[];
export declare function isTeamToolName(name: string): boolean;
export declare function callTeamTool(name: string, callTauri: TauriPost, callTauriGet: TauriGet, args: unknown, runtimeContext?: TeamToolRuntimeContext): Promise<unknown>;
export {};
//# sourceMappingURL=team-tools.d.ts.map