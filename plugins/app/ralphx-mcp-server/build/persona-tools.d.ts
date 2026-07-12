import { Tool } from "@modelcontextprotocol/sdk/types.js";
import type { TauriCallOptions } from "./tauri-client.js";
type TauriPost = (path: string, body: Record<string, unknown>, options?: TauriCallOptions) => Promise<unknown>;
type TauriGet = (path: string, options?: TauriCallOptions) => Promise<unknown>;
export type PersonaToolRuntimeContext = {
    conversationId?: string;
};
export declare const PERSONA_BUILDER_TOOLS: Tool[];
export declare function isPersonaToolName(name: string): boolean;
export declare function callPersonaTool(name: string, callTauri: TauriPost, callTauriGet: TauriGet, args: unknown, runtimeContext?: PersonaToolRuntimeContext): Promise<unknown>;
export {};
//# sourceMappingURL=persona-tools.d.ts.map