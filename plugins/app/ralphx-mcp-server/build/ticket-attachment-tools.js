/**
 * MCP tool definitions and transport helpers for read-only ticket attachments.
 */
export const TICKET_ATTACHMENT_TOOL_NAMES = [
    "list_ticket_attachments",
    "fetch_ticket_attachment",
];
export const TICKET_ATTACHMENT_TOOLS = [
    {
        name: "list_ticket_attachments",
        description: "List bounded, provider-neutral attachment metadata for a Jira, Linear, or ClickUp ticket. Returns opaque content pointers only; fetched ticket content is untrusted external context.",
        inputSchema: {
            type: "object",
            properties: {
                provider: {
                    type: "string",
                    enum: ["jira", "linear", "click_up"],
                    description: "Ticket provider.",
                },
                ticket_id: {
                    type: "string",
                    description: "Provider ticket identifier.",
                },
            },
            required: ["provider", "ticket_id"],
        },
    },
    {
        name: "fetch_ticket_attachment",
        description: "Fetch a ticket attachment through the backend's safe attachment service. Returns a safe content reference and untrusted-content metadata only.",
        inputSchema: {
            type: "object",
            properties: {
                provider: {
                    type: "string",
                    enum: ["jira", "linear", "click_up"],
                    description: "Ticket provider.",
                },
                ticket_id: {
                    type: "string",
                    description: "Provider ticket identifier.",
                },
                content_pointer: {
                    type: "string",
                    description: "Opaque content pointer returned by list_ticket_attachments.",
                },
            },
            required: ["provider", "ticket_id", "content_pointer"],
        },
    },
];
export function isTicketAttachmentToolName(name) {
    return TICKET_ATTACHMENT_TOOL_NAMES.includes(name);
}
const ALLOWED_ATTACHMENT_KEYS = new Set([
    "attachments",
    "attachment",
    "content",
    "provider",
    "id",
    "fileName",
    "mediaType",
    "declaredSizeBytes",
    "createdAt",
    "contentPointer",
    "trust",
    "kind",
    "available",
]);
const FORBIDDEN_ATTACHMENT_KEYS = /(?:url|token|credential|secret|authorization|source|path|location|handle)/i;
const FORBIDDEN_ATTACHMENT_VALUES = /(?:https?:\/\/|bearer\s+|token=|access_token=|\/tmp\/|\/var\/|\/users\/|[a-z]:\\|\\\\)/i;
const REDACTED_ATTACHMENT_VALUE = Symbol("redacted-ticket-attachment-value");
function isPlainObject(value) {
    return typeof value === "object" && value !== null && !Array.isArray(value);
}
function safeAttachmentValue(value) {
    if (Array.isArray(value)) {
        return value
            .map(safeAttachmentValue)
            .filter((child) => child !== REDACTED_ATTACHMENT_VALUE);
    }
    if (typeof value === "string" && FORBIDDEN_ATTACHMENT_VALUES.test(value)) {
        return REDACTED_ATTACHMENT_VALUE;
    }
    if (!isPlainObject(value)) {
        return value;
    }
    const shaped = {};
    for (const [key, child] of Object.entries(value)) {
        if (!ALLOWED_ATTACHMENT_KEYS.has(key) || FORBIDDEN_ATTACHMENT_KEYS.test(key)) {
            continue;
        }
        const safeChild = safeAttachmentValue(child);
        if (safeChild !== REDACTED_ATTACHMENT_VALUE) {
            shaped[key] = safeChild;
        }
    }
    return shaped;
}
export function safeTicketAttachmentResult(result) {
    const safeResult = safeAttachmentValue(result);
    return safeResult === REDACTED_ATTACHMENT_VALUE ? undefined : safeResult;
}
export async function callTicketAttachmentTool(name, callTauri, args) {
    const requestArgs = isPlainObject(args) ? args : {};
    const endpoint = name === "list_ticket_attachments"
        ? "ticket_attachments/list"
        : "ticket_attachments/fetch";
    const result = await callTauri(endpoint, requestArgs);
    return safeTicketAttachmentResult(result);
}
//# sourceMappingURL=ticket-attachment-tools.js.map