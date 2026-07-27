// Tauri invoke wrappers for the remote environment registry (PR 2.1, §6.1/§6.4).
//
// P-18: no wrapper here can see a device token — the backend never serializes one.
// The active-environment id is MIRRORED to Rust via setActiveEnvironment; the Rust
// copy is the authority the proxy commands enforce (P-26).

import { z } from "zod";
import { typedInvoke } from "@/lib/tauri";

export const remoteEnvironmentStatusSchema = z.enum([
  "active",
  "pending_add",
  "pending_delete",
]);

export type RemoteEnvironmentStatus = z.infer<typeof remoteEnvironmentStatusSchema>;

export const remoteEnvironmentSummarySchema = z.object({
  id: z.string(),
  environmentId: z.string(),
  name: z.string(),
  baseUrl: z.string(),
  candidateUrls: z.array(z.string()),
  scopes: z.array(z.string()),
  protocolVersion: z.number(),
  status: remoteEnvironmentStatusSchema,
  createdAt: z.string(),
  lastConnectedAt: z.string().nullable(),
});

export type RemoteEnvironmentSummary = z.infer<typeof remoteEnvironmentSummarySchema>;

export const remoteEnvironmentsApi = {
  /** Pairing exchange runs entirely in the Rust backend (§4.2). */
  pair(url: string, code: string, name: string): Promise<RemoteEnvironmentSummary> {
    return typedInvoke(
      "pair_remote_environment",
      { input: { url, code, name } },
      remoteEnvironmentSummarySchema
    );
  },

  list(): Promise<RemoteEnvironmentSummary[]> {
    return typedInvoke(
      "list_remote_environments",
      {},
      z.array(remoteEnvironmentSummarySchema)
    );
  },

  remove(id: string): Promise<null> {
    return typedInvoke("remove_remote_environment", { input: { id } }, z.null());
  },

  getActiveEnvironment(): Promise<string> {
    return typedInvoke("get_active_environment", {}, z.string());
  },

  /** Mirrors the switch into the Rust-side authoritative store (§6.4). */
  setActiveEnvironment(id: string): Promise<null> {
    return typedInvoke("set_active_environment", { input: { id } }, z.null());
  },
};
