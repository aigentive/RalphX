import { Tool } from "@modelcontextprotocol/sdk/types.js";
import type { TauriCallOptions } from "./tauri-client.js";
type TauriPost = (path: string, body: Record<string, unknown>, options?: TauriCallOptions) => Promise<unknown>;
export type AutomationSetupToolRuntimeContext = {
    conversationId?: string;
};
export declare const AUTOMATION_SETUP_TOOLS: Tool[];
export declare function isAutomationSetupToolName(name: string): boolean;
export declare function callAutomationSetupTool(name: string, callTauri: TauriPost, args: unknown, runtimeContext?: AutomationSetupToolRuntimeContext): Promise<unknown>;
export {};
//# sourceMappingURL=automation-tools.d.ts.map