/**
 * The project-owned invoke wrapper (A-13, §6.2 point 1).
 *
 * `frontend/vite.config.ts` aliases `@tauri-apps/api/core` to THIS module outside web
 * mode, so every existing importer — including `typedInvoke`/`typedInvokeWithTransform`
 * in `lib/tauri.ts` — switches transport with zero call-site edits (A-3).
 *
 * What is deliberately NOT aliased: the rest of `@tauri-apps/api/core`. Aliasing the
 * whole module would silently give `convertFileSrc`, `Channel`, `transformCallback`
 * and friends remote semantics they must not have — `convertFileSrc` resolves a LOCAL
 * asset-protocol URL and a remote attachment has to go through the scoped
 * `/remote/v1/attachments/{id}` endpoint instead (§6.2). The `export *` below therefore
 * re-exports every non-invoke member untouched, and only `invoke` is replaced.
 *
 * P-18 (wrapper half): this module and its imports open no socket and issue no
 * cross-origin request. Remote traffic leaves through the local `remote_invoke`
 * command, i.e. through Rust, which is where the bearer lives (C-15).
 */

import { invoke as primitiveInvoke } from "#tauri-core-primitive";
import type { InvokeArgs, InvokeOptions } from "#tauri-core-primitive";

import {
  getTransportEnvironmentId,
  isRemoteEnvironmentId,
} from "./active-environment";
import { findLocalOnlyCommand } from "./local-only-commands";
import { networkInvoke } from "./network-invoke";
import { RemoteTransportError } from "./transport-errors";

// Every non-invoke member of the real core, unchanged, with local semantics.
// A local `invoke` declaration shadows the star-exported one, which is exactly the
// A-13 shape: replace one export, pass the rest through. Using `export *` rather than
// an enumerated list means a future core member cannot be silently dropped by this
// wrapper.
export * from "#tauri-core-primitive";

/**
 * Routes one command to the active environment's transport.
 *
 * Local environment → the Tauri IPC primitive, byte-identical to no wrapper at all.
 * Remote environment → `NetworkInvoke`, except for commands classified local-only by
 * `local-only-commands.ts` — either an explicit reason-coded row or the `plugin:`
 * prefix rule, which keeps the seven Tauri plugin namespaces on THIS device.
 *
 * Signature-compatible with `@tauri-apps/api/core`'s `invoke` by construction — the
 * alias makes every existing caller depend on that.
 */
export async function invoke<T>(
  cmd: string,
  args?: InvokeArgs,
  options?: InvokeOptions
): Promise<T> {
  const environmentId = getTransportEnvironmentId();
  if (!isRemoteEnvironmentId(environmentId)) {
    return primitiveInvoke<T>(cmd, args, options);
  }

  const localOnly = findLocalOnlyCommand(cmd);
  if (localOnly !== undefined) {
    if (localOnly.disposition === "run-locally") {
      return primitiveInvoke<T>(cmd, args, options);
    }
    throw new RemoteTransportError({
      code: "REMOTE_COMMAND_UNAVAILABLE",
      message: `\`${cmd}\` is local-only and has no remote equivalent. ${localOnly.reason}`,
      environmentId,
      cmd,
    });
  }

  // `InvokeOptions` (custom IPC headers) is a local-transport concern; the remote
  // wire carries `{requestId, cmd, args}` only (§3.1). Nothing in the app passes
  // options today, and dropping them silently for a remote call is the honest
  // behaviour: there is no header channel to forward them onto.
  return networkInvoke<T>(environmentId, cmd, args);
}
