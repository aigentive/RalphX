import type { WorkspaceOpenTarget } from "@/api/chat";

export const PREFERRED_WORKSPACE_OPEN_TARGET_KEY =
  "ralphx:agents:preferred-workspace-open-target";
const PREFERRED_WORKSPACE_OPEN_TARGET_EVENT =
  "ralphx:agents:preferred-workspace-open-target-changed";

export function readPreferredWorkspaceOpenTargetId(): string | null {
  if (typeof window === "undefined") {
    return null;
  }
  return window.localStorage.getItem(PREFERRED_WORKSPACE_OPEN_TARGET_KEY);
}

export function writePreferredWorkspaceOpenTargetId(targetId: string): void {
  if (typeof window === "undefined") {
    return;
  }
  window.localStorage.setItem(PREFERRED_WORKSPACE_OPEN_TARGET_KEY, targetId);
  window.dispatchEvent(
    new CustomEvent(PREFERRED_WORKSPACE_OPEN_TARGET_EVENT, {
      detail: { targetId },
    }),
  );
}

export function resolvePreferredWorkspaceOpenTarget(
  targets: readonly WorkspaceOpenTarget[],
  preferredTargetId: string | null,
): WorkspaceOpenTarget | null {
  return (
    targets.find((target) => target.id === preferredTargetId) ??
    targets[0] ??
    null
  );
}

export function subscribePreferredWorkspaceOpenTargetId(
  onChange: (targetId: string | null) => void,
): () => void {
  if (typeof window === "undefined") {
    return () => undefined;
  }

  const handlePreferenceChange = (event: Event) => {
    if (event instanceof CustomEvent) {
      const targetId =
        typeof event.detail?.targetId === "string" ? event.detail.targetId : null;
      onChange(targetId);
    }
  };
  const handleStorageChange = (event: StorageEvent) => {
    if (event.key === PREFERRED_WORKSPACE_OPEN_TARGET_KEY) {
      onChange(event.newValue);
    }
  };

  window.addEventListener(
    PREFERRED_WORKSPACE_OPEN_TARGET_EVENT,
    handlePreferenceChange,
  );
  window.addEventListener("storage", handleStorageChange);

  return () => {
    window.removeEventListener(
      PREFERRED_WORKSPACE_OPEN_TARGET_EVENT,
      handlePreferenceChange,
    );
    window.removeEventListener("storage", handleStorageChange);
  };
}
