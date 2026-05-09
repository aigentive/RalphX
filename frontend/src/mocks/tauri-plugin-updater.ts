/**
 * Mock implementation of @tauri-apps/plugin-updater for web mode
 *
 * Update checking is opt-in in browser mode.
 *
 * Use `?mockUpdate=available` or localStorage key `ralphx-mock-update=available`
 * to preview the update UI without signed release artifacts.
 */

export interface DownloadProgress {
  event: "Started" | "Progress" | "Finished";
  data: {
    contentLength?: number;
    chunkLength: number;
  };
}

export interface Update {
  /** Whether an update is available */
  available: true;
  /** New version string */
  version: string;
  /** Current installed version */
  currentVersion: string;
  /** Release notes/changelog */
  body?: string;
  /** Release date */
  date?: string;
  /** Download and install the update with progress callback */
  downloadAndInstall: (
    onProgress?: (progress: DownloadProgress) => void
  ) => Promise<void>;
  /** Download the update package */
  download: (onProgress?: (progress: DownloadProgress) => void) => Promise<void>;
  /** Install a previously downloaded update package */
  install: () => Promise<void>;
  /** Release updater resources */
  close: () => Promise<void>;
}

const MOCK_UPDATE_STORAGE_KEY = "ralphx-mock-update";

function shouldReturnMockUpdate(): boolean {
  if (typeof window === "undefined") {
    return false;
  }

  const params = new URLSearchParams(window.location.search);
  return (
    params.get("mockUpdate") === "available" ||
    window.localStorage.getItem(MOCK_UPDATE_STORAGE_KEY) === "available"
  );
}

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function emitMockDownloadProgress(
  onProgress?: (progress: DownloadProgress) => void
): Promise<void> {
  onProgress?.({
    event: "Started",
    data: { contentLength: 1_000, chunkLength: 0 },
  });
  await delay(20);
  onProgress?.({
    event: "Progress",
    data: { chunkLength: 420 },
  });
  await delay(20);
  onProgress?.({
    event: "Progress",
    data: { chunkLength: 580 },
  });
  await delay(20);
  onProgress?.({
    event: "Finished",
    data: { chunkLength: 0 },
  });
}

function createMockUpdate(): Update {
  return {
    available: true,
    currentVersion: "0.3.1",
    version: "0.3.2",
    date: "2026-05-01T06:00:00Z",
    body: "Daily release with reliability fixes and UI polish.",
    downloadAndInstall: async (onProgress) => {
      await emitMockDownloadProgress(onProgress);
    },
    download: async (onProgress) => {
      await emitMockDownloadProgress(onProgress);
    },
    install: async () => undefined,
    close: async () => undefined,
  };
}

export async function check(): Promise<Update | null> {
  if (!shouldReturnMockUpdate()) {
    console.debug("[mock] updater.check called - returning null (no update)");
    return null;
  }

  console.debug("[mock] updater.check called - returning mock update");
  return createMockUpdate();
}
