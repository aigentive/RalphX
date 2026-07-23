import { Tool } from "@modelcontextprotocol/sdk/types.js";
import type { RuntimeContext } from "./runtime-context.js";
import type { TauriCallOptions } from "./tauri-client.js";
export declare const LEARNED_SKILL_TOOLS: Tool[];
export declare const LEARNED_SKILL_TOOL_NAMES: string[];
export declare function learnedSkillEndpoint(toolName: string): string;
export declare function learnedSkillTransportOptions(toolName: string, runtimeContext: RuntimeContext): TauriCallOptions | undefined;
//# sourceMappingURL=learned-skill-tools.d.ts.map