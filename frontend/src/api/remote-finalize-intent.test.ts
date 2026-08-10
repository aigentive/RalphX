import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), remote: true, backendFetch: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("@/api/backend", () => ({ backendFetch: mocks.backendFetch }));
vi.mock("@/lib/remote/active-environment", () => ({
  getTransportEnvironmentId: () => (mocks.remote ? "remote-1" : "local"),
  isRemoteEnvironmentId: (id: string) => id !== "local",
}));

import { ideationApi, RemoteFinalizeIntentError } from "./ideation";

const NOW = "2026-08-05T10:00:00Z";

describe("remote finalize intent branch", () => {
  beforeEach(() => {
    mocks.invoke.mockReset();
    mocks.backendFetch.mockReset();
    mocks.remote = true;
  });

  it.each(["accept", "reject"] as const)("submits and settles %s", async (decision) => {
    mocks.invoke
      .mockResolvedValueOnce({ requestId: "finalize-1", status: "pending", deduplicated: false, createdAt: NOW })
      .mockResolvedValueOnce({
        requestId: "finalize-1",
        status: "completed",
        errorCode: null,
        result: decision === "accept" ? ["task-1", "task-2"] : { status: "rejected", sessionId: "session-1" },
        createdAt: NOW,
        updatedAt: NOW,
      });
    const result = await ideationApi.acceptance[decision]("session-1");
    expect(mocks.invoke).toHaveBeenNthCalledWith(1, "request_remote_ideation_finalize_decision", {
      input: { sessionId: "session-1", decision },
    });
    expect(mocks.invoke).toHaveBeenNthCalledWith(2, "get_remote_ideation_finalize_request", { requestId: "finalize-1" });
    expect(result).toEqual({ status: decision === "accept" ? "accepted" : "rejected", sessionId: "session-1" });
  });

  it("surfaces no-longer-pending distinctly without resubmission", async () => {
    mocks.invoke
      .mockResolvedValueOnce({ requestId: "finalize-2", status: "pending", deduplicated: false, createdAt: NOW })
      .mockResolvedValueOnce({ requestId: "finalize-2", status: "failed", errorCode: "REMOTE_FINALIZE_NOT_PENDING", result: null, createdAt: NOW, updatedAt: NOW });
    const error = await ideationApi.acceptance.accept("session-1").catch((value) => value);
    expect(error).toBeInstanceOf(RemoteFinalizeIntentError);
    expect(error.errorCode).toBe("REMOTE_FINALIZE_NOT_PENDING");
    expect(error.message).toBe("No longer awaiting confirmation");
    expect(mocks.invoke).toHaveBeenCalledTimes(2);
  });

  it("preserves the local accept HTTP branch", async () => {
    mocks.remote = false;
    mocks.backendFetch.mockResolvedValue({
      ok: true,
      json: async () => ({ status: "accepted", session_id: "session-1" }),
    });
    await expect(ideationApi.acceptance.accept("session-1")).resolves.toEqual({
      status: "accepted",
      sessionId: "session-1",
    });
    expect(mocks.backendFetch).toHaveBeenCalledWith(
      "ideation/sessions/session-1/accept-finalize",
      expect.objectContaining({ method: "POST" }),
    );
  });
});
