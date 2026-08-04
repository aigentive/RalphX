import { invoke as primitiveInvoke, type InvokeArgs } from "#tauri-core-primitive";

/** Explicit escape hatch for commands whose subject is the Mac rendering this UI. */
export function invokeClientLocal<T>(command: string, args?: InvokeArgs): Promise<T> {
  return primitiveInvoke<T>(command, args);
}
