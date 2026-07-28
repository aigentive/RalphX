/**
 * The two zod-validating invoke wrappers, extracted from the `@/lib/tauri` barrel.
 *
 * They used to live in the barrel itself, which meant that importing `typedInvoke`
 * pulled in EVERY domain API module the barrel re-exports — chat, automations,
 * artifacts and their schema graphs. That is how `environmentStore` (which needs
 * nothing but `typedInvoke` for the environment registry) ended up transitively
 * loading the whole API surface, and why any component consuming the environment
 * store inherited it too.
 *
 * The barrel re-exports both functions, so every existing `from "@/lib/tauri"`
 * import keeps working unchanged.
 */

import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";

/**
 * Generic invoke wrapper with runtime Zod validation.
 *
 * @throws If the response doesn't match the schema
 */
export async function typedInvoke<T>(
  cmd: string,
  args: Record<string, unknown>,
  schema: z.ZodType<T>
): Promise<T> {
  const result = await invoke(cmd, args);
  return schema.parse(result);
}

/**
 * Generic invoke wrapper with runtime Zod validation and a camelCase transform.
 *
 * @throws If the response doesn't match the schema
 */
export async function typedInvokeWithTransform<TRaw, TResult>(
  cmd: string,
  args: Record<string, unknown>,
  schema: z.ZodType<TRaw>,
  transform: (raw: TRaw) => TResult
): Promise<TResult> {
  const result = await invoke(cmd, args);
  const validated = schema.parse(result);
  return transform(validated);
}
