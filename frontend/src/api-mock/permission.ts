/**
 * Mock Permission API
 *
 * Provides mock implementation for permission resolution operations.
 * Used for browser testing and visual regression testing.
 */

import type { ResolvePermissionInput } from "@/api/permission";
import type { PermissionRequest } from "@/types/permission";

/**
 * Mock Permission API matching the real API interface
 */
export const mockPermissionApi = {
  /**
   * Mock permission resolution - no-op for visual testing
   * In web mode, permission dialogs are simulated via events
   */
  resolveRequest: async (_input: ResolvePermissionInput): Promise<void> => {
    // No-op - visual testing doesn't process permission responses
    console.log("[mock] resolveRequest called");
  },

  /**
   * Mock get pending permissions - returns empty array for visual testing
   */
  getPendingPermissions: async (): Promise<PermissionRequest[]> => {
    return [];
  },

  /**
   * Mock pending-gate listing — the replacement-set read the dialog reconciles against.
   * The mock harness has no host, so nothing is ever pending; returning [] keeps the
   * dialog closed rather than leaving a gate on screen nobody is waiting for.
   *
   * This must exist for the app to BOOT in web/mock mode: PermissionDialog calls it
   * during mount, and an absent function threw
   * `api.permission.listPendingPermissionGates is not a function` straight into the
   * error boundary, which is what took down every Playwright job.
   */
  listPendingPermissionGates: async (): Promise<PermissionRequest[]> => {
    return [];
  },
} as const;
