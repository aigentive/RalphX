/**
 * Permission API Module
 *
 * Provides a centralized API wrapper for resolving permission requests.
 * This module follows the domain API pattern used by other centralized modules.
 */

import { invoke } from "@tauri-apps/api/core";
import {
  getTransportEnvironmentId,
  isRemoteEnvironmentId,
} from "@/lib/remote/active-environment";
import type { PermissionRequest } from "@/types/permission";

// ============================================================================
// Types
// ============================================================================

/** Raw shape returned by the backend get_pending_permissions command (snake_case) */
interface PendingPermissionInfoRaw {
  request_id: string;
  tool_name: string;
  tool_input: Record<string, unknown>;
  context?: string | null;
  agent_type?: string | null;
  task_id?: string | null;
  context_type?: string | null;
  context_id?: string | null;
}

export interface ResolvePermissionInput {
  requestId: string;
  decision: "allow" | "deny";
  message?: string;
}

// ============================================================================
// Permission API Object
// ============================================================================

/**
 * Permission API object containing typed Tauri command wrappers
 */
export const permissionApi = {
  /**
   * Resolve a permission request from an agent.
   *
   * Both brakes on a paired device (Phase 1). `resolve_permission_request` is a
   * DUAL-DECISION command and the facade deliberately does not register it: one op that
   * can allow or deny by argument would have to sit at `agentControl`, which would take
   * the deny brake away from every default pairing. The facade splits it into two
   * server-pinned ops instead — `approve_permission_request` (`agentControl`, pins
   * `decision: "allow"`) and `deny_permission_request` (`operate`, pins
   * `decision: "deny"`) — both dispatching to this same target fn.
   *
   * The pin is written server-side by `extract_pinned_arg`, overwriting whatever the
   * client sent, so the payload is byte-identical across all three names and the ONLY
   * difference is the command chosen. That is deliberate: the decision the host acts on
   * comes from the op name it was reached through, never from a field a narrowed device
   * could flip.
   *
   * Three literal `invoke` sites, not one computed name (P-11):
   * `scripts/check-remote-transport-drift.mjs` reads the invoke ARGUMENT, so a helper
   * returning the name would be a hole in the proof that no command reaches the facade
   * unclassified. Mirrors the `sendAgentMessage`/`sendRemoteChatMessage` split.
   *
   * @param input The resolution details including request ID and decision
   */
  resolveRequest: async (input: ResolvePermissionInput): Promise<void> => {
    const args = {
      request_id: input.requestId,
      decision: input.decision,
      message: input.message,
    };
    if (isRemoteEnvironmentId(getTransportEnvironmentId())) {
      if (input.decision === "deny") {
        // `operate`-class: the brake must work on a DEFAULT pairing, with no `ui:agent`.
        await invoke("deny_permission_request", { args });
        return;
      }
      await invoke("approve_permission_request", { args });
      return;
    }
    await invoke("resolve_permission_request", { args });
    // Command returns () on success, no parsing needed
  },

  /**
   * Fetch all currently pending permission requests from the backend in-memory state.
   * Used to hydrate the UI for requests whose Tauri events were missed
   * (e.g., because the permission dialog wasn't mounted when the event fired).
   */
  getPendingPermissions: async (): Promise<PermissionRequest[]> => {
    return toPermissionRequests(
      await invoke<PendingPermissionInfoRaw[]>("get_pending_permissions")
    );
  },

  /**
   * The AUTHORITATIVE pending set (P-21, PR 2.7-c).
   *
   * Distinct from `getPendingPermissions` in two ways that matter. It calls the strict
   * facade command, which FAILS rather than answering an empty list when the backing
   * state cannot be read — "no data" must never be mistaken for "no gates" and used to
   * clear the dialog. And its result is a REPLACEMENT set, not an additive one: a gate
   * the host no longer lists has been resolved or expired, and leaving it on screen
   * invites the user to answer something nobody is waiting for.
   */
  listPendingPermissionGates: async (): Promise<PermissionRequest[]> => {
    return toPermissionRequests(
      await invoke<PendingPermissionInfoRaw[]>("list_pending_permission_gates")
    );
  },
} as const;

function toPermissionRequests(
  raw: PendingPermissionInfoRaw[]
): PermissionRequest[] {
  return raw.map((item) => ({
      request_id: item.request_id,
      tool_name: item.tool_name,
      tool_input: item.tool_input,
      ...(item.context != null && { context: item.context }),
      ...(item.agent_type != null && { agent_type: item.agent_type }),
      ...(item.task_id != null && { task_id: item.task_id }),
      ...(item.context_type != null && { context_type: item.context_type }),
      ...(item.context_id != null && { context_id: item.context_id }),
  }));
}
