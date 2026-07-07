import { Tool } from "@modelcontextprotocol/sdk/types.js";
type TauriPost = (path: string, body: Record<string, unknown>) => Promise<unknown>;
export declare const TICKET_ATTACHMENT_TOOLS: Tool[];
export declare function isTicketAttachmentToolName(name: string): boolean;
export declare function callTicketAttachmentTool(name: string, callTauri: TauriPost, args: unknown): Promise<unknown>;
export {};
//# sourceMappingURL=ticket-attachment-tools.d.ts.map