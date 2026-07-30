const MODULE_LOAD_ERROR_MESSAGES = [
  "importing a module script failed",
  "failed to fetch dynamically imported module",
  "error loading dynamically imported module",
  "unable to preload css",
] as const;

/** Identifies browser transport failures while loading a dynamic module. */
export function isModuleLoadError(error: unknown): boolean {
  const message = getErrorMessage(error);
  if (!message) return false;

  const normalizedMessage = message.toLowerCase();
  return MODULE_LOAD_ERROR_MESSAGES.some((pattern) =>
    normalizedMessage.includes(pattern),
  );
}

function getErrorMessage(error: unknown): string | null {
  if (typeof error === "string") return error;
  if (error === null || typeof error !== "object") return null;

  try {
    const { message } = error as { message?: unknown };
    return typeof message === "string" ? message : null;
  } catch {
    return null;
  }
}
