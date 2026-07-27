/**
 * Transport-visible active-environment identity (§6.4).
 *
 * This module is deliberately a LEAF: it imports nothing. The invoke wrapper
 * (`lib/remote/invoke.ts`) is aliased over `@tauri-apps/api/core`, so anything it
 * imports is transitively imported by ~116 modules. Reading the active id straight
 * from `stores/environmentStore.ts` would close a cycle
 * (`environmentStore` → `api/remote-environments` → `lib/tauri` → `@tauri-apps/api/core`
 * → wrapper → `environmentStore`), so the store WRITES here instead and the wrapper
 * only reads.
 *
 * Authority model (unchanged by this module): the Rust proxy holds the authoritative
 * active-environment id and enforces it (P-26). This value is the same UI mirror
 * `environmentStore` paints from — it decides which transport a call takes, never
 * whether the call is allowed.
 *
 * Default is `"local"`. A module graph that never loads `environmentStore` (tests,
 * isolated imports) therefore routes locally, which is the fail-safe direction.
 */

/** The always-present local environment identity (§6.4). */
export const LOCAL_ENVIRONMENT_ID = "local";

let transportEnvironmentId: string = LOCAL_ENVIRONMENT_ID;

/** The environment the invoke/fetch transports currently route to. */
export function getTransportEnvironmentId(): string {
  return transportEnvironmentId;
}

/**
 * Mirrors an environment switch into the transport layer.
 *
 * Single writer: `stores/environmentStore.ts` subscribes to its own
 * `activeEnvironmentId` and calls this. Nothing else should.
 */
export function setTransportEnvironmentId(id: string): void {
  transportEnvironmentId = id;
}

/** True when `id` names a paired remote environment rather than this Mac. */
export function isRemoteEnvironmentId(id: string): boolean {
  return id !== LOCAL_ENVIRONMENT_ID;
}

/**
 * Test-only reset so a suite cannot leak a remote identity into the next file.
 * Production code has no reason to call this — use `setTransportEnvironmentId`.
 */
export function resetTransportEnvironmentId(): void {
  transportEnvironmentId = LOCAL_ENVIRONMENT_ID;
}
