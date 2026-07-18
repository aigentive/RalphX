import { Tool } from "@modelcontextprotocol/sdk/types.js";
import type { RuntimeContext } from "./runtime-context.js";
export declare const FILESYSTEM_TOOL_NAMES: readonly ["fs_read_file", "fs_list_dir", "fs_grep", "fs_glob"];
export declare const FILESYSTEM_TOOLS: Tool[];
type ToolResult = {
    content: Array<{
        type: "text";
        text: string;
    }>;
    isError?: boolean;
};
export declare function handleFilesystemToolCall(name: string, rawArgs: unknown, runtimeContext: Pick<RuntimeContext, "filesystemEnforced">): Promise<ToolResult>;
export declare function formatFilesystemToolError(error: unknown): ToolResult;
export {};
//# sourceMappingURL=filesystem-tools.d.ts.map