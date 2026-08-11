/**
 * HTTP client for calling RalphX Tauri backend
 * All MCP tools forward to these endpoints (proxy pattern)
 */
export declare function buildTauriApiUrl(endpoint: string): string;
export interface TauriApiError {
    error: string;
    details?: string;
}
export declare class TauriClientError extends Error {
    statusCode: number;
    details?: string | undefined;
    constructor(message: string, statusCode: number, details?: string | undefined);
}
export interface TauriCallOptions {
    headers?: Record<string, string>;
}
type TauriHttpTransport = (url: string, init: RequestInit) => Promise<Response>;
export declare function __setTauriHttpTransportForTests(transport: TauriHttpTransport | undefined): void;
/**
 * Call a Tauri backend endpoint via HTTP POST
 * @param endpoint - Endpoint path (e.g., "create_task_proposal")
 * @param args - Request body (JSON)
 * @returns Response data
 * @throws TauriClientError on HTTP errors
 */
export declare function callTauri(endpoint: string, args: Record<string, unknown>, options?: TauriCallOptions): Promise<unknown>;
/**
 * Call a Tauri backend endpoint via HTTP GET
 * @param endpoint - Endpoint path (e.g., "task_context/task-123")
 * @returns Response data
 * @throws TauriClientError on HTTP errors
 */
export declare function callTauriGet(endpoint: string, options?: TauriCallOptions): Promise<unknown>;
export {};
//# sourceMappingURL=tauri-client.d.ts.map