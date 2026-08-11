/**
 * MCP tool definitions and transport helpers for read-only ticket attachments.
 */
import { Tool } from "@modelcontextprotocol/sdk/types.js";
export declare const TICKET_ATTACHMENT_TOOL_NAMES: readonly ["list_ticket_attachments", "fetch_ticket_attachment"];
export type TicketAttachmentToolName = (typeof TICKET_ATTACHMENT_TOOL_NAMES)[number];
export declare const TICKET_ATTACHMENT_TOOLS: Tool[];
export declare function isTicketAttachmentToolName(name: string): name is TicketAttachmentToolName;
type CallTauri = (endpoint: string, body: Record<string, unknown>) => Promise<unknown>;
export declare function safeTicketAttachmentResult(result: unknown): unknown;
export declare function callTicketAttachmentTool(name: TicketAttachmentToolName, callTauri: CallTauri, args: unknown): Promise<unknown>;
export {};
//# sourceMappingURL=ticket-attachment-tools.d.ts.map