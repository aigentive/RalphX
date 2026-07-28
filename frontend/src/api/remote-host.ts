// Tauri invoke wrappers for the HOST side of remote access (PR 1.7, §5.4).
//
// Binds the PR 1.1/1.2 host-local commands (src-tauri/src/commands/registry.rs,
// `// remote auth (PR 1.2)` block). None of these commands is reachable on :3849 —
// device management and pairing-code minting are host-local only (§3.1).
//
// Rule 14 / C-11: struct params wrap under the Rust param name (`input`); fields
// are camelCase (`#[serde(rename_all = "camelCase")]` on every input/output type).

import { z } from "zod";
import { typedInvoke } from "@/lib/tauri";

// ---------------------------------------------------------------------------
// Local-only session lifecycle events (§5.5)
// ---------------------------------------------------------------------------

/** Local-only event: a remote device session was admitted (§5.5). */
export const REMOTE_SESSION_CONNECTED_EVENT = "remote:session_connected";
/** Local-only event: a remote device session ended (§5.5). */
export const REMOTE_SESSION_CLOSED_EVENT = "remote:session_closed";

// ---------------------------------------------------------------------------
// Schemas
// ---------------------------------------------------------------------------

export const remoteExposureModeSchema = z.enum(["serve", "tailnetDirect"]);
export type RemoteExposureMode = z.infer<typeof remoteExposureModeSchema>;

export const remoteScopeSchema = z.enum([
  "ui:read",
  "ui:operate",
  "ui:agent",
  "ui:elevated",
]);
export type RemoteScope = z.infer<typeof remoteScopeSchema>;

export const remoteListenerStatusSchema = z.object({
  enabled: z.boolean(),
  exposureMode: remoteExposureModeSchema,
  /**
   * The CONFIGURED port — `RALPHX_REMOTE_PORT` can override the actual bind and that
   * override surfaces only through `bindAddress`. Derive advertised URLs from
   * `bindAddress`, never from this field.
   */
  port: z.number(),
  environmentId: z.string(),
  running: z.boolean(),
  bindAddress: z.string().nullable(),
  serveActive: z.boolean(),
  serveDegradedReason: z.string().nullable(),
  /**
   * The machine-readable degradation cause. Branch on THIS, never on `serveDegradedReason`
   * — the prose is a log line, not an API (rule 5). Mirrors `RemoteServeDegradedKind`.
   */
  serveDegradedKind: z
    .enum(["cliUnavailable", "launchFailed", "timeout", "commandFailed"])
    .nullable(),
});
export type RemoteListenerStatus = z.infer<typeof remoteListenerStatusSchema>;

export const mintedRemotePairingCodeSchema = z.object({
  id: z.string(),
  /** Shown once — stored hashed at rest, never returned again (A-9). */
  code: z.string(),
  scopes: z.array(remoteScopeSchema),
  createdAt: z.string(),
  expiresAt: z.string(),
  expiresInSecs: z.number(),
});
export type MintedRemotePairingCode = z.infer<typeof mintedRemotePairingCodeSchema>;

export const remotePairingCodeViewSchema = z.object({
  id: z.string(),
  scopes: z.array(remoteScopeSchema),
  createdAt: z.string(),
  expiresAt: z.string(),
});
export type RemotePairingCodeView = z.infer<typeof remotePairingCodeViewSchema>;

export const remoteDeviceViewSchema = z.object({
  id: z.string(),
  name: z.string(),
  tokenPrefix: z.string(),
  scopes: z.array(remoteScopeSchema),
  agentControlGranted: z.boolean(),
  createdAt: z.string(),
  lastSeenAt: z.string().nullable(),
  revokedAt: z.string().nullable(),
  liveSessionCount: z.number(),
});
export type RemoteDeviceView = z.infer<typeof remoteDeviceViewSchema>;

export const remoteSessionViewSchema = z.object({
  id: z.string(),
  deviceId: z.string(),
  connectedAt: z.string(),
  lastActiveAt: z.string(),
  remoteAddr: z.string(),
  /** Whether the in-memory registry still holds a kill channel for this session. */
  live: z.boolean(),
});
export type RemoteSessionView = z.infer<typeof remoteSessionViewSchema>;

/**
 * Shape mirror of `remote_server/endpoints.rs::AdvertisedEndpoint` (serde camelCase).
 * Scheme is a transport fact — Serve advertises `https://<magicdns>`, tailnet direct
 * advertises `http://<ip>:<bound-port>`. Render schemes as given; never normalize.
 */
export const advertisedEndpointSchema = z.object({
  kind: z.enum(["loopbackServe", "tailnetDirect"]),
  url: z.string(),
  available: z.boolean(),
});
export type AdvertisedEndpoint = z.infer<typeof advertisedEndpointSchema>;

/**
 * Shape mirror of `ralphx-domain/src/entities/remote_access.rs::RemoteAuditEntry`
 * (camelCase view; `action` uses the stable `as_db_value` strings, e.g.
 * "pairing_code_created", "device_revoked", "agent_control_granted").
 */
export const remoteAuditEntrySchema = z.object({
  id: z.number(),
  deviceId: z.string().nullable(),
  action: z.string(),
  detail: z.string().nullable(),
  createdAt: z.string(),
});
export type RemoteAuditEntry = z.infer<typeof remoteAuditEntrySchema>;

// ---------------------------------------------------------------------------
// API
// ---------------------------------------------------------------------------

export const remoteHostApi = {
  getListenerStatus(): Promise<RemoteListenerStatus> {
    return typedInvoke("get_remote_listener_status", {}, remoteListenerStatusSchema);
  },

  /** Enables remote host mode and binds the listener for the persisted exposure mode. */
  startListener(): Promise<RemoteListenerStatus> {
    return typedInvoke("start_remote_listener", {}, remoteListenerStatusSchema);
  },

  /** Disables remote host mode and releases the port (immediate session teardown, §4.4). */
  stopListener(): Promise<RemoteListenerStatus> {
    return typedInvoke("stop_remote_listener", {}, remoteListenerStatusSchema);
  },

  setExposureMode(exposureMode: RemoteExposureMode): Promise<RemoteListenerStatus> {
    return typedInvoke(
      "set_remote_exposure_mode",
      { input: { exposureMode } },
      remoteListenerStatusSchema,
    );
  },

  /** Mints a single-use pairing code with a 10-minute TTL; the raw code is shown once. */
  generatePairingCode(): Promise<MintedRemotePairingCode> {
    return typedInvoke(
      "generate_remote_pairing_code",
      {},
      mintedRemotePairingCodeSchema,
    );
  },

  /** Outstanding (unconsumed, unexpired) codes — without their raw values. */
  listPairingCodes(): Promise<RemotePairingCodeView[]> {
    return typedInvoke(
      "list_remote_pairing_codes",
      {},
      z.array(remotePairingCodeViewSchema),
    );
  },

  /** Cancels an outstanding pairing code before anyone redeems it (§4.6 stolen-QR row). */
  revokePairingCode(id: string): Promise<boolean> {
    return typedInvoke("revoke_remote_pairing_code", { input: { id } }, z.boolean());
  },

  /** Every paired device, revoked ones included, with live session counts. */
  listDevices(): Promise<RemoteDeviceView[]> {
    return typedInvoke("list_remote_devices", {}, z.array(remoteDeviceViewSchema));
  },

  /**
   * Grants or withdraws `ui:agent` for one device (§5.4 — the one deliberate consent).
   * Withdrawing narrows the durable grant first, then fires the device's kill channels.
   */
  setDeviceAgentControl(deviceId: string, enabled: boolean): Promise<RemoteDeviceView> {
    return typedInvoke(
      "set_remote_device_agent_control",
      { input: { deviceId, enabled } },
      remoteDeviceViewSchema,
    );
  },

  /** Revokes a device and tears its live sessions down immediately (registry kill channels). */
  revokeDevice(deviceId: string): Promise<RemoteDeviceView> {
    return typedInvoke(
      "revoke_remote_device",
      { input: { deviceId } },
      remoteDeviceViewSchema,
    );
  },

  listSessions(): Promise<RemoteSessionView[]> {
    return typedInvoke("list_remote_sessions", {}, z.array(remoteSessionViewSchema));
  },

  /** Closes one live session without revoking its device. */
  disconnectSession(sessionId: string): Promise<boolean> {
    return typedInvoke(
      "disconnect_remote_session",
      { input: { sessionId } },
      z.boolean(),
    );
  },

  /**
   * The URLs this Mac currently advertises for its own listener. Empty while nothing is
   * bound; a rejection is a real backend failure, not a missing surface.
   */
  listAdvertisedEndpoints(): Promise<AdvertisedEndpoint[]> {
    return typedInvoke(
      "list_remote_advertised_endpoints",
      {},
      z.array(advertisedEndpointSchema),
    );
  },

  /**
   * Most-recent-first remote-access audit entries for THIS Mac. The command defaults to 100
   * rows; the pane renders the newest 10.
   */
  listAuditEntries(limit?: number): Promise<RemoteAuditEntry[]> {
    return typedInvoke(
      "list_remote_audit_entries",
      limit === undefined ? {} : { limit },
      z.array(remoteAuditEntrySchema),
    );
  },
} as const;
