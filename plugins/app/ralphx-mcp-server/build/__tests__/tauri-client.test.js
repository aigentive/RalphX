/**
 * Unit tests for tauri-client retry logic with exponential backoff.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { TauriClientError, buildTauriApiUrl, callTauri, callTauriGet } from "../tauri-client.js";
// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------
function makeNetworkError() {
    return new TauriClientError("Failed to connect: ECONNREFUSED", 0);
}
function makeStatusError(status) {
    return new TauriClientError(`Tauri API error: status ${status}`, status);
}
// ---------------------------------------------------------------------------
// TauriClientError — isRetryable semantics (tested indirectly via callTauri)
// ---------------------------------------------------------------------------
describe("callTauri — retry on network errors", () => {
    beforeEach(() => {
        vi.useFakeTimers();
    });
    afterEach(() => {
        vi.restoreAllMocks();
        vi.useRealTimers();
    });
    it("succeeds on first attempt when no error", async () => {
        const mockFetch = vi.fn().mockResolvedValue(new Response(JSON.stringify({ ok: true }), { status: 200 }));
        vi.stubGlobal("fetch", mockFetch);
        const result = callTauri("test_endpoint", { foo: "bar" });
        await vi.runAllTimersAsync();
        expect(await result).toEqual({ ok: true });
        expect(mockFetch).toHaveBeenCalledTimes(1);
    });
    it("passes custom transport headers through artifact mutation requests", async () => {
        const mockFetch = vi.fn().mockResolvedValue(new Response(JSON.stringify({ ok: true }), { status: 200 }));
        vi.stubGlobal("fetch", mockFetch);
        const result = callTauri("update_plan_artifact", { artifact_id: "artifact-1", content: "updated" }, { headers: { "X-RalphX-Caller-Session-Id": "child-session" } });
        await vi.runAllTimersAsync();
        expect(await result).toEqual({ ok: true });
        expect(mockFetch).toHaveBeenCalledWith("http://127.0.0.1:3847/api/update_plan_artifact", expect.objectContaining({
            method: "POST",
            headers: expect.objectContaining({
                "Content-Type": "application/json",
                "X-RalphX-Caller-Session-Id": "child-session",
            }),
        }));
    });
    it("retries on network error (statusCode 0) and succeeds on 2nd attempt", async () => {
        const successResponse = new Response(JSON.stringify({ ok: true }), {
            status: 200,
        });
        const mockFetch = vi
            .fn()
            .mockRejectedValueOnce(new Error("ECONNREFUSED"))
            .mockResolvedValueOnce(successResponse);
        vi.stubGlobal("fetch", mockFetch);
        const result = callTauri("test_endpoint", {});
        await vi.runAllTimersAsync();
        expect(await result).toEqual({ ok: true });
        expect(mockFetch).toHaveBeenCalledTimes(2);
    });
    it("retries on 502 and succeeds on 3rd attempt", async () => {
        const errorResponse = new Response(JSON.stringify({ error: "bad gateway" }), {
            status: 502,
            statusText: "Bad Gateway",
        });
        const successResponse = new Response(JSON.stringify({ ok: true }), {
            status: 200,
        });
        const mockFetch = vi
            .fn()
            .mockResolvedValueOnce(errorResponse)
            .mockResolvedValueOnce(errorResponse)
            .mockResolvedValueOnce(successResponse);
        vi.stubGlobal("fetch", mockFetch);
        const result = callTauri("test_endpoint", {});
        await vi.runAllTimersAsync();
        expect(await result).toEqual({ ok: true });
        expect(mockFetch).toHaveBeenCalledTimes(3);
    });
    it("exhausts all 3 retries (4 total attempts) and throws on persistent network error", async () => {
        const mockFetch = vi
            .fn()
            .mockRejectedValue(new Error("ECONNREFUSED"));
        vi.stubGlobal("fetch", mockFetch);
        const resultPromise = callTauri("test_endpoint", {});
        // Attach rejects handler BEFORE advancing timers to avoid unhandled rejection
        const assertion = expect(resultPromise).rejects.toThrow(TauriClientError);
        await vi.runAllTimersAsync();
        await assertion;
        // 1 initial + 3 retries = 4 total
        expect(mockFetch).toHaveBeenCalledTimes(4);
    });
    it("retries on 503", async () => {
        const errorResponse = new Response(JSON.stringify({ error: "service unavailable" }), { status: 503, statusText: "Service Unavailable" });
        const successResponse = new Response(JSON.stringify({ ok: true }), {
            status: 200,
        });
        const mockFetch = vi
            .fn()
            .mockResolvedValueOnce(errorResponse)
            .mockResolvedValueOnce(successResponse);
        vi.stubGlobal("fetch", mockFetch);
        const result = callTauri("test_endpoint", {});
        await vi.runAllTimersAsync();
        expect(await result).toEqual({ ok: true });
        expect(mockFetch).toHaveBeenCalledTimes(2);
    });
    it("retries on 504", async () => {
        const errorResponse = new Response(JSON.stringify({ error: "gateway timeout" }), { status: 504, statusText: "Gateway Timeout" });
        const successResponse = new Response(JSON.stringify({ ok: true }), {
            status: 200,
        });
        const mockFetch = vi
            .fn()
            .mockResolvedValueOnce(errorResponse)
            .mockResolvedValueOnce(successResponse);
        vi.stubGlobal("fetch", mockFetch);
        const result = callTauri("test_endpoint", {});
        await vi.runAllTimersAsync();
        expect(await result).toEqual({ ok: true });
        expect(mockFetch).toHaveBeenCalledTimes(2);
    });
});
describe("buildTauriApiUrl", () => {
    afterEach(() => {
        delete process.env.TAURI_API_URL;
    });
    it("builds localhost API URLs for safe endpoints", () => {
        expect(buildTauriApiUrl("question/request")).toBe("http://127.0.0.1:3847/api/question/request");
    });
    it("accepts explicit local alternate backend ports", () => {
        process.env.TAURI_API_URL = "http://127.0.0.1:3857";
        expect(buildTauriApiUrl("question/request")).toBe("http://127.0.0.1:3857/api/question/request");
    });
    it("rejects non-local TAURI_API_URL values", () => {
        process.env.TAURI_API_URL = "https://example.com";
        expect(() => buildTauriApiUrl("question/request")).toThrow("Invalid TAURI_API_URL protocol");
    });
    it("rejects traversal in endpoints", () => {
        expect(() => buildTauriApiUrl("../question/request")).toThrow("Invalid endpoint traversal sequence");
    });
});
describe("callTauri — no retry on client errors", () => {
    afterEach(() => {
        vi.restoreAllMocks();
    });
    it("does NOT retry on 400 (bad request)", async () => {
        const errorResponse = new Response(JSON.stringify({ error: "bad request" }), {
            status: 400,
            statusText: "Bad Request",
        });
        const mockFetch = vi.fn().mockResolvedValue(errorResponse);
        vi.stubGlobal("fetch", mockFetch);
        await expect(callTauri("test_endpoint", {})).rejects.toThrow(TauriClientError);
        // No retry — only 1 attempt
        expect(mockFetch).toHaveBeenCalledTimes(1);
    });
    it("does NOT retry on 404", async () => {
        const errorResponse = new Response(JSON.stringify({ error: "not found" }), {
            status: 404,
            statusText: "Not Found",
        });
        const mockFetch = vi.fn().mockResolvedValue(errorResponse);
        vi.stubGlobal("fetch", mockFetch);
        await expect(callTauri("test_endpoint", {})).rejects.toThrow(TauriClientError);
        expect(mockFetch).toHaveBeenCalledTimes(1);
    });
    it("does NOT retry on 408 (permission await timeout — stale = reject by design)", async () => {
        const errorResponse = new Response(JSON.stringify({ error: "permission request timed out" }), { status: 408, statusText: "Request Timeout" });
        const mockFetch = vi.fn().mockResolvedValue(errorResponse);
        vi.stubGlobal("fetch", mockFetch);
        await expect(callTauri("permission/await/some-id", {})).rejects.toThrow(TauriClientError);
        expect(mockFetch).toHaveBeenCalledTimes(1);
    });
    it("does NOT retry on 422 (unprocessable entity)", async () => {
        const errorResponse = new Response(JSON.stringify({ error: "validation failed" }), { status: 422, statusText: "Unprocessable Entity" });
        const mockFetch = vi.fn().mockResolvedValue(errorResponse);
        vi.stubGlobal("fetch", mockFetch);
        await expect(callTauri("test_endpoint", {})).rejects.toThrow(TauriClientError);
        expect(mockFetch).toHaveBeenCalledTimes(1);
    });
});
describe("callTauriGet — retry on network errors", () => {
    afterEach(() => {
        vi.restoreAllMocks();
        vi.useRealTimers();
    });
    it("retries GET on network error and succeeds", async () => {
        vi.useFakeTimers();
        const successResponse = new Response(JSON.stringify({ data: "value" }), {
            status: 200,
        });
        const mockFetch = vi
            .fn()
            .mockRejectedValueOnce(new Error("ECONNRESET"))
            .mockResolvedValueOnce(successResponse);
        vi.stubGlobal("fetch", mockFetch);
        const result = callTauriGet("some/endpoint");
        await vi.runAllTimersAsync();
        expect(await result).toEqual({ data: "value" });
        expect(mockFetch).toHaveBeenCalledTimes(2);
    });
    it("does NOT retry GET on 404", async () => {
        const errorResponse = new Response(JSON.stringify({ error: "not found" }), {
            status: 404,
            statusText: "Not Found",
        });
        const mockFetch = vi.fn().mockResolvedValue(errorResponse);
        vi.stubGlobal("fetch", mockFetch);
        await expect(callTauriGet("missing/endpoint")).rejects.toThrow(TauriClientError);
        expect(mockFetch).toHaveBeenCalledTimes(1);
    });
});
// ---------------------------------------------------------------------------
// safeJsonParse — empty body and non-JSON resilience
// ---------------------------------------------------------------------------
describe("callTauri — safeJsonParse resilience", () => {
    afterEach(() => {
        vi.restoreAllMocks();
    });
    it("2xx valid JSON → returns parsed object", async () => {
        const mockFetch = vi.fn().mockResolvedValue(new Response(JSON.stringify({ id: "abc" }), { status: 200 }));
        vi.stubGlobal("fetch", mockFetch);
        const result = await callTauri("test_endpoint", {});
        expect(result).toEqual({ id: "abc" });
    });
    it("2xx empty body → returns null instead of throwing", async () => {
        const mockFetch = vi.fn().mockResolvedValue(new Response("", { status: 200 }));
        vi.stubGlobal("fetch", mockFetch);
        const result = await callTauri("set_cross_project_checked", {});
        expect(result).toBeNull();
    });
    it("2xx non-JSON text → returns null instead of throwing", async () => {
        const mockFetch = vi.fn().mockResolvedValue(new Response("OK", { status: 200 }));
        vi.stubGlobal("fetch", mockFetch);
        const result = await callTauri("test_endpoint", {});
        expect(result).toBeNull();
    });
    it("4xx → throws TauriClientError (not swallowed)", async () => {
        const mockFetch = vi.fn().mockResolvedValue(new Response(JSON.stringify({ error: "forbidden" }), {
            status: 403,
            statusText: "Forbidden",
        }));
        vi.stubGlobal("fetch", mockFetch);
        await expect(callTauri("test_endpoint", {})).rejects.toThrow(TauriClientError);
    });
    it("5xx → throws TauriClientError (after retries)", async () => {
        const mockFetch = vi.fn().mockResolvedValue(new Response(JSON.stringify({ error: "server error" }), {
            status: 500,
            statusText: "Internal Server Error",
        }));
        vi.stubGlobal("fetch", mockFetch);
        await expect(callTauri("test_endpoint", {})).rejects.toThrow(TauriClientError);
    });
});
describe("callTauriGet — safeJsonParse resilience", () => {
    afterEach(() => {
        vi.restoreAllMocks();
    });
    it("2xx empty body → returns null", async () => {
        const mockFetch = vi.fn().mockResolvedValue(new Response("", { status: 200 }));
        vi.stubGlobal("fetch", mockFetch);
        const result = await callTauriGet("some/endpoint");
        expect(result).toBeNull();
    });
    it("2xx non-JSON text → returns null", async () => {
        const mockFetch = vi.fn().mockResolvedValue(new Response("plain text", { status: 200 }));
        vi.stubGlobal("fetch", mockFetch);
        const result = await callTauriGet("some/endpoint");
        expect(result).toBeNull();
    });
});
// ---------------------------------------------------------------------------
// parseErrorResponse — body consumption bug fix (text-first reading)
// ---------------------------------------------------------------------------
describe("parseErrorResponse — error body surfacing", () => {
    afterEach(() => {
        vi.restoreAllMocks();
    });
    it("plain-text 400 body → surfaces the actual error text (not statusText)", async () => {
        const mockFetch = vi.fn().mockResolvedValue(new Response("review notes are missing required fields", {
            status: 400,
            statusText: "Bad Request",
        }));
        vi.stubGlobal("fetch", mockFetch);
        const err = (await callTauri("complete_review", {}).catch((e) => e));
        expect(err).toBeInstanceOf(TauriClientError);
        expect(err.message).toBe("review notes are missing required fields");
        expect(err.statusCode).toBe(400);
    });
    it("JSON body {error: msg} 400 → surfaces the error field value (regression check)", async () => {
        const mockFetch = vi.fn().mockResolvedValue(new Response(JSON.stringify({ error: "task is not in reviewable state" }), {
            status: 400,
            statusText: "Bad Request",
        }));
        vi.stubGlobal("fetch", mockFetch);
        const err = (await callTauri("complete_review", {}).catch((e) => e));
        expect(err).toBeInstanceOf(TauriClientError);
        expect(err.message).toBe("task is not in reviewable state");
        expect(err.statusCode).toBe(400);
    });
    it("JSON body with details field → surfaces both error and details", async () => {
        const mockFetch = vi.fn().mockResolvedValue(new Response(JSON.stringify({ error: "validation failed", details: "field 'name' is required" }), { status: 422, statusText: "Unprocessable Entity" }));
        vi.stubGlobal("fetch", mockFetch);
        const err = (await callTauri("test_endpoint", {}).catch((e) => e));
        expect(err).toBeInstanceOf(TauriClientError);
        expect(err.message).toBe("validation failed");
        expect(err.details).toBe("field 'name' is required");
    });
    it("empty body 400 → graceful fallback to statusText", async () => {
        const mockFetch = vi.fn().mockResolvedValue(new Response("", {
            status: 400,
            statusText: "Bad Request",
        }));
        vi.stubGlobal("fetch", mockFetch);
        const err = (await callTauri("test_endpoint", {}).catch((e) => e));
        expect(err).toBeInstanceOf(TauriClientError);
        expect(err.message).toBe("Tauri API error: Bad Request");
        expect(err.statusCode).toBe(400);
    });
    it("callTauriGet: plain-text 400 body → surfaces actual error text", async () => {
        const mockFetch = vi.fn().mockResolvedValue(new Response("endpoint not found", {
            status: 404,
            statusText: "Not Found",
        }));
        vi.stubGlobal("fetch", mockFetch);
        const err = (await callTauriGet("missing/endpoint").catch((e) => e));
        expect(err).toBeInstanceOf(TauriClientError);
        expect(err.message).toBe("endpoint not found");
        expect(err.statusCode).toBe(404);
    });
});
//# sourceMappingURL=tauri-client.test.js.map