import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  remote: true,
  backendFetch: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("@/api/backend", () => ({ backendFetch: mocks.backendFetch }));
vi.mock("@/lib/remote/active-environment", () => ({
  getTransportEnvironmentId: () => (mocks.remote ? "remote-1" : "local"),
  isRemoteEnvironmentId: (id: string) => id !== "local",
}));

import { artifactApi, RemotePlanIntentError } from "./artifact";

const NOW = "2026-08-05T10:00:00Z";
const RAW_ARTIFACT = {
  id: "plan-2",
  name: "Displayed plan",
  artifact_type: "specification",
  content_type: "inline",
  content: "# Plan",
  created_at: NOW,
  created_by: "user",
  version: 8,
  bucket_id: null,
  task_id: null,
  process_id: null,
  derived_from: ["plan-1"],
  artifact_role: "overview",
  plan_approval_status: "approved",
  plan_approved_artifact_id: "plan-2",
  plan_approved_version: 8,
  plan_approved_blueprint_artifact_id: null,
  plan_approved_blueprint_version: null,
  plan_approved_at: NOW,
  blueprint_artifact: null,
  plan_contract_version: 2,
  plan_target_id: null,
};

describe("remote plan intent branches", () => {
  beforeEach(() => {
    mocks.invoke.mockReset();
    mocks.backendFetch.mockReset();
    mocks.remote = true;
  });

  it("submits the displayed approval tuple and settles the serialized artifact", async () => {
    mocks.invoke
      .mockResolvedValueOnce({ requestId: "req-1", status: "pending", deduplicated: false, createdAt: NOW })
      .mockResolvedValueOnce({ requestId: "req-1", status: "completed", errorCode: null, result: RAW_ARTIFACT, createdAt: NOW, updatedAt: NOW });
    const result = await artifactApi.approvePlanArtifact({
      sessionId: "session-1",
      artifactId: "plan-2",
      blueprintArtifactId: "blueprint-4",
      blueprintArtifactVersion: 6,
    });
    expect(mocks.invoke).toHaveBeenNthCalledWith(1, "request_remote_plan_approval", {
      input: { sessionId: "session-1", artifactId: "plan-2", blueprintArtifactId: "blueprint-4", blueprintArtifactVersion: 6 },
    });
    expect(mocks.invoke).toHaveBeenNthCalledWith(2, "get_remote_plan_approval_request", { requestId: "req-1" });
    expect(result.metadata.version).toBe(8);
  });

  it("preserves the local approval HTTP branch", async () => {
    mocks.remote = false;
    mocks.backendFetch.mockResolvedValue({
      ok: true,
      json: async () => RAW_ARTIFACT,
    });
    await artifactApi.approvePlanArtifact({
      sessionId: "session-1",
      artifactId: "plan-2",
      blueprintArtifactId: "blueprint-4",
      blueprintArtifactVersion: 6,
    });
    expect(mocks.backendFetch).toHaveBeenCalledWith(
      "approve_plan_artifact",
      expect.objectContaining({
        method: "POST",
        body: JSON.stringify({
          session_id: "session-1",
          artifact_id: "plan-2",
          blueprint_artifact_id: "blueprint-4",
          blueprint_artifact_version: 6,
        }),
      }),
    );
  });

  it("surfaces plan-changed as a typed non-resubmitting error", async () => {
    mocks.invoke
      .mockResolvedValueOnce({ requestId: "req-2", status: "pending", deduplicated: false, createdAt: NOW })
      .mockResolvedValueOnce({ requestId: "req-2", status: "failed", errorCode: "REMOTE_PLAN_APPROVAL_PLAN_CHANGED", result: null, createdAt: NOW, updatedAt: NOW });
    const error = await artifactApi.approvePlanArtifact({ sessionId: "session-1", artifactId: "plan-2" }).catch((value) => value);
    expect(error).toBeInstanceOf(RemotePlanIntentError);
    expect(error.errorCode).toBe("REMOTE_PLAN_APPROVAL_PLAN_CHANGED");
    expect(error.message).toBe("Plan changed — refresh and approve again");
    expect(mocks.invoke).toHaveBeenCalledTimes(2);
  });

  it("submits the displayed edit version and preserves the local HTTP branch", async () => {
    mocks.invoke
      .mockResolvedValueOnce({ requestId: "edit-1", status: "pending", deduplicated: false, createdAt: NOW })
      .mockResolvedValueOnce({ requestId: "edit-1", status: "completed", errorCode: null, result: RAW_ARTIFACT, createdAt: NOW, updatedAt: NOW });
    await artifactApi.updatePlanArtifact({ artifactId: "plan-2", content: "changed", expectedVersion: 7 });
    expect(mocks.invoke).toHaveBeenNthCalledWith(1, "request_remote_plan_artifact_edit", {
      input: { artifactId: "plan-2", content: "changed", expectedVersion: 7 },
    });

    mocks.remote = false;
    mocks.backendFetch.mockResolvedValue({ ok: true, json: async () => RAW_ARTIFACT });
    await artifactApi.updatePlanArtifact({ artifactId: "plan-2", content: "local", expectedVersion: 7 });
    expect(mocks.backendFetch).toHaveBeenCalledWith("update_plan_artifact", expect.objectContaining({
      method: "POST",
      body: JSON.stringify({ artifact_id: "plan-2", content: "local" }),
    }));
  });

  it.each([
    ["REMOTE_PLAN_EDIT_VERSION_CONFLICT", "Plan changed — reload before editing"],
    ["REMOTE_PLAN_EDIT_FROZEN", "Plan is frozen — verification agent is actively working. Wait for the current round to complete."],
  ])("surfaces %s distinctly", async (errorCode, message) => {
    mocks.invoke
      .mockResolvedValueOnce({ requestId: "edit-2", status: "pending", deduplicated: false, createdAt: NOW })
      .mockResolvedValueOnce({ requestId: "edit-2", status: "failed", errorCode, result: null, createdAt: NOW, updatedAt: NOW });
    const error = await artifactApi.updatePlanArtifact({ artifactId: "plan-2", content: "changed", expectedVersion: 7 }).catch((value) => value);
    expect(error.errorCode).toBe(errorCode);
    expect(error.message).toBe(message);
  });
});
