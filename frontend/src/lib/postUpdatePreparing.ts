export const POST_UPDATE_PREPARING_STORAGE_KEY = "ralphx:post-update-preparing";

const POST_UPDATE_PREPARING_TTL_MS = 2 * 60 * 1_000;

export interface PostUpdatePreparingMarker {
  startedAt: number;
  version?: string;
}

function getLocalStorage(): Storage | null {
  if (typeof window === "undefined") {
    return null;
  }

  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

function normalizeMarker(value: unknown): PostUpdatePreparingMarker | null {
  if (!value || typeof value !== "object") {
    return null;
  }

  const candidate = value as { startedAt?: unknown; version?: unknown };
  if (typeof candidate.startedAt !== "number" || !Number.isFinite(candidate.startedAt)) {
    return null;
  }

  if (typeof candidate.version === "string" && candidate.version.length > 0) {
    return { startedAt: candidate.startedAt, version: candidate.version };
  }

  return { startedAt: candidate.startedAt };
}

export function markPostUpdatePreparing(version?: string, now = Date.now()): void {
  const storage = getLocalStorage();
  if (!storage) {
    return;
  }

  const marker: PostUpdatePreparingMarker =
    version && version.length > 0 ? { startedAt: now, version } : { startedAt: now };
  storage.setItem(POST_UPDATE_PREPARING_STORAGE_KEY, JSON.stringify(marker));
}

export function clearPostUpdatePreparing(): void {
  getLocalStorage()?.removeItem(POST_UPDATE_PREPARING_STORAGE_KEY);
}

export function readFreshPostUpdatePreparingMarker(
  now = Date.now(),
): PostUpdatePreparingMarker | null {
  const storage = getLocalStorage();
  if (!storage) {
    return null;
  }

  const raw = storage.getItem(POST_UPDATE_PREPARING_STORAGE_KEY);
  if (!raw) {
    return null;
  }

  let marker: PostUpdatePreparingMarker | null = null;
  try {
    marker = normalizeMarker(JSON.parse(raw));
  } catch {
    marker = null;
  }

  const age = marker ? now - marker.startedAt : Number.POSITIVE_INFINITY;
  if (!marker || age < 0 || age > POST_UPDATE_PREPARING_TTL_MS) {
    clearPostUpdatePreparing();
    return null;
  }

  return marker;
}
