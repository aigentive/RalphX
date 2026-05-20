export type TauriUnlistenFn = () => void | PromiseLike<void>;

function isPromiseLike(value: unknown): value is PromiseLike<void> {
  if (value === null) {
    return false;
  }
  const valueType = typeof value;
  if (valueType !== "object" && valueType !== "function") {
    return false;
  }
  return typeof (value as { then?: unknown }).then === "function";
}

export function safelyUnlistenTauri(
  unlisten: TauriUnlistenFn | null | undefined,
  label: string,
): void {
  if (!unlisten) {
    return;
  }

  try {
    const result = unlisten();
    if (isPromiseLike(result)) {
      void Promise.resolve(result).catch((error: unknown) => {
        console.warn(`[TauriListener] Failed to unlisten ${label}:`, error);
      });
    }
  } catch (error) {
    console.warn(`[TauriListener] Failed to unlisten ${label}:`, error);
  }
}
