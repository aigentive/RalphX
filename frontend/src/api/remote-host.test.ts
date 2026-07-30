/**
 * remote-host API wrapper tests.
 *
 * Proves rule-14 binding against the PR 1.2 host-local commands: struct params
 * wrap under their Rust param name (`input`) and all fields are camelCase.
 */

import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";

import {
  REMOTE_SESSION_CLOSED_EVENT,
  REMOTE_SESSION_CONNECTED_EVENT,
  remoteHostApi,
} from "./remote-host";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

const mockInvoke = vi.mocked(invoke);

const listenerStatus = {
  enabled: true,
  exposureMode: "serve",
  port: 3849,
  environmentId: "env-1",
  running: true,
  bindAddress: "127.0.0.1:3849",
  serveActive: true,
  serveDegradedReason: null,
  // Always present on the wire: `RemoteListenerStatusResponse::serve_degraded_kind` is a plain
  // `Option`, so serde emits an explicit null rather than omitting the key.
  serveDegradedKind: null,
};

const mintedCode = {
  id: "pc-1",
  code: "rxp_ABCDEFGHJKLMNPQRSTUVWXYZ012345aa",
  scopes: ["ui:read", "ui:operate"],
  createdAt: "2026-07-27T10:00:00Z",
  expiresAt: "2026-07-27T10:10:00Z",
  expiresInSecs: 600,
};

const deviceView = {
  id: "dev-1",
  name: "Anca's iPhone",
  tokenPrefix: "rxd_live_AbCd",
  scopes: ["ui:read", "ui:operate"],
  agentControlGranted: false,
  createdAt: "2026-07-20T10:00:00Z",
  lastSeenAt: null,
  revokedAt: null,
  liveSessionCount: 0,
};

const sessionView = {
  id: "sess-1",
  deviceId: "dev-1",
  connectedAt: "2026-07-27T09:00:00Z",
  lastActiveAt: "2026-07-27T09:30:00Z",
  remoteAddr: "100.64.0.7:52001",
  live: true,
};

describe("remoteHostApi", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
  });

  it("getListenerStatus invokes with no args and parses the status", async () => {
    mockInvoke.mockResolvedValue(listenerStatus);
    const status = await remoteHostApi.getListenerStatus();
    expect(mockInvoke).toHaveBeenCalledWith("get_remote_listener_status", {});
    expect(status.serveActive).toBe(true);
    expect(status.exposureMode).toBe("serve");
  });

  it("startListener / stopListener invoke the listener lifecycle commands", async () => {
    mockInvoke.mockResolvedValue(listenerStatus);
    await remoteHostApi.startListener();
    expect(mockInvoke).toHaveBeenCalledWith("start_remote_listener", {});
    await remoteHostApi.stopListener();
    expect(mockInvoke).toHaveBeenCalledWith("stop_remote_listener", {});
  });

  it("setExposureMode wraps the mode under the input struct param", async () => {
    mockInvoke.mockResolvedValue({
      ...listenerStatus,
      exposureMode: "tailnetDirect",
    });
    await remoteHostApi.setExposureMode("tailnetDirect");
    expect(mockInvoke).toHaveBeenCalledWith("set_remote_exposure_mode", {
      input: { exposureMode: "tailnetDirect" },
    });
  });

  it("generatePairingCode invokes with no args and returns the raw code once", async () => {
    mockInvoke.mockResolvedValue(mintedCode);
    const minted = await remoteHostApi.generatePairingCode();
    expect(mockInvoke).toHaveBeenCalledWith("generate_remote_pairing_code", {});
    expect(minted.code).toBe(mintedCode.code);
    expect(minted.expiresInSecs).toBe(600);
  });

  it("listPairingCodes parses outstanding codes without raw values", async () => {
    mockInvoke.mockResolvedValue([
      {
        id: "pc-2",
        scopes: ["ui:read"],
        createdAt: "2026-07-27T10:00:00Z",
        expiresAt: "2026-07-27T10:10:00Z",
      },
    ]);
    const codes = await remoteHostApi.listPairingCodes();
    expect(mockInvoke).toHaveBeenCalledWith("list_remote_pairing_codes", {});
    expect(codes).toHaveLength(1);
    expect(codes[0]?.id).toBe("pc-2");
  });

  it("revokePairingCode wraps the id under input", async () => {
    mockInvoke.mockResolvedValue(true);
    await expect(remoteHostApi.revokePairingCode("pc-1")).resolves.toBe(true);
    expect(mockInvoke).toHaveBeenCalledWith("revoke_remote_pairing_code", {
      input: { id: "pc-1" },
    });
  });

  it("listDevices parses device views", async () => {
    mockInvoke.mockResolvedValue([deviceView]);
    const devices = await remoteHostApi.listDevices();
    expect(mockInvoke).toHaveBeenCalledWith("list_remote_devices", {});
    expect(devices[0]?.agentControlGranted).toBe(false);
    expect(devices[0]?.tokenPrefix).toBe("rxd_live_AbCd");
  });

  it("setDeviceAgentControl wraps deviceId + enabled in camelCase under input", async () => {
    mockInvoke.mockResolvedValue({ ...deviceView, agentControlGranted: true });
    await remoteHostApi.setDeviceAgentControl("dev-1", true);
    expect(mockInvoke).toHaveBeenCalledWith("set_remote_device_agent_control", {
      input: { deviceId: "dev-1", enabled: true },
    });
  });

  it("revokeDevice wraps deviceId under input", async () => {
    mockInvoke.mockResolvedValue({
      ...deviceView,
      revokedAt: "2026-07-27T11:00:00Z",
    });
    const revoked = await remoteHostApi.revokeDevice("dev-1");
    expect(mockInvoke).toHaveBeenCalledWith("revoke_remote_device", {
      input: { deviceId: "dev-1" },
    });
    expect(revoked.revokedAt).toBe("2026-07-27T11:00:00Z");
  });

  it("listSessions parses session views", async () => {
    mockInvoke.mockResolvedValue([sessionView]);
    const sessions = await remoteHostApi.listSessions();
    expect(mockInvoke).toHaveBeenCalledWith("list_remote_sessions", {});
    expect(sessions[0]?.remoteAddr).toBe("100.64.0.7:52001");
    expect(sessions[0]?.live).toBe(true);
  });

  it("disconnectSession wraps sessionId under input", async () => {
    mockInvoke.mockResolvedValue(true);
    await remoteHostApi.disconnectSession("sess-1");
    expect(mockInvoke).toHaveBeenCalledWith("disconnect_remote_session", {
      input: { sessionId: "sess-1" },
    });
  });

  it("rejects a listener status payload that drops serve fields", async () => {
    const { serveActive: _serveActive, ...broken } = listenerStatus;
    mockInvoke.mockResolvedValue(broken);
    await expect(remoteHostApi.getListenerStatus()).rejects.toThrow();
  });

  it("exposes the local-only session lifecycle event names as constants", () => {
    expect(REMOTE_SESSION_CONNECTED_EVENT).toBe("remote:session_connected");
    expect(REMOTE_SESSION_CLOSED_EVENT).toBe("remote:session_closed");
  });
});
