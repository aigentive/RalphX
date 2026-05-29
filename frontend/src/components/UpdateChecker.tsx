import { useCallback, useEffect, useRef, useState } from "react";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { toast } from "sonner";
import { Download, Sparkles } from "lucide-react";
import {
  getCurrentReleaseNotes,
  getLastSeenReleaseNotesVersion,
  markReleaseNotesSeen,
} from "@/api/release-notes";
import {
  clearPostUpdatePreparing,
  markPostUpdatePreparing,
} from "@/lib/postUpdatePreparing";
import { ReleaseNotesDialog } from "@/components/ReleaseNotesDialog";
import { useUiStore } from "@/stores/uiStore";

const INITIAL_UPDATE_CHECK_DELAY_MS = 3_000;
const STARTUP_RELEASE_NOTES_DELAY_MS = 4_000;
const UPDATE_CHECK_INTERVAL_MS = 30 * 60 * 1_000;
const LIFECYCLE_UPDATE_CHECK_COOLDOWN_MS = 5 * 60 * 1_000;
const UPDATE_CHECK_EVENT = "ralphx://check-for-updates";
const RELEASE_NOTES_EVENT = "ralphx://show-release-notes";
const UPDATE_CHECK_RESULT_TOAST_ID = "update-check-result";
const GITHUB_RELEASE_METADATA_MARKERS =
  /^[ \t]*<!--\s*github-release-metadata:(?:start|end)\s*-->[ \t]*\n?/gm;

interface ReleaseNotesView {
  version: string;
  body: string | null;
  context: "current" | "update";
}

interface ReleaseDialogState {
  open: boolean;
  version?: string | undefined;
  body?: string | null | undefined;
  context?: "current" | "update" | undefined;
}

interface CheckForUpdatesOptions {
  manual?: boolean;
  force?: boolean;
}

export function UpdateChecker() {
  const activeModal = useUiStore((s) => s.activeModal);
  const checkInFlight = useRef(false);
  const manualCheckRequested = useRef(false);
  const notifiedVersion = useRef<string | null>(null);
  const lastCheckAt = useRef<number | null>(null);
  const whatsNewVersion = useRef<string | null>(null);
  const pendingWhatsNew = useRef<ReleaseNotesView | null>(null);
  const visibleWhatsNew = useRef<ReleaseNotesView | null>(null);
  const visibleWhatsNewToastId = useRef<string | null>(null);
  const isGlobalModalOpen = useRef(activeModal !== null);
  const [dialogState, setDialogState] = useState<ReleaseDialogState>({ open: false });

  const clearVisibleWhatsNew = useCallback((version?: string) => {
    if (version !== undefined && visibleWhatsNew.current?.version !== version) {
      return;
    }
    visibleWhatsNew.current = null;
    visibleWhatsNewToastId.current = null;
  }, []);

  const openReleaseNotes = useCallback(
    (notes: ReleaseNotesView) => {
      if (notes.context === "current") {
        toast.dismiss(whatsNewToastId(notes.version));
        pendingWhatsNew.current = null;
        clearVisibleWhatsNew(notes.version);
      }
      setDialogState({
        open: true,
        version: notes.version,
        body: notes.body,
        context: notes.context,
      });
    },
    [clearVisibleWhatsNew],
  );

  const presentWhatsNewToast = useCallback(
    (notes: ReleaseNotesView) => {
      if (isGlobalModalOpen.current) {
        pendingWhatsNew.current = notes;
        return;
      }

      const toastId = whatsNewToastId(notes.version);
      pendingWhatsNew.current = null;
      visibleWhatsNew.current = notes;
      visibleWhatsNewToastId.current = toastId;
      showWhatsNewToast(notes, openReleaseNotes, () => clearVisibleWhatsNew(notes.version));
    },
    [clearVisibleWhatsNew, openReleaseNotes],
  );

  const openCurrentReleaseNotes = useCallback(() => {
    setDialogState({ open: true, context: "current" });
  }, []);

  const handleUpdateFromDialog = useCallback(async () => {
    setDialogState({ open: false });
    showCheckingForUpdatesToast();
    try {
      const update = await check();
      if (update) {
        toast.dismiss(UPDATE_CHECK_RESULT_TOAST_ID);
        void installUpdate(update);
      } else {
        toast.success("RalphX is up to date.", { id: UPDATE_CHECK_RESULT_TOAST_ID });
      }
    } catch {
      toast.error("Failed to check for updates.", { id: UPDATE_CHECK_RESULT_TOAST_ID });
    }
  }, []);

  const checkForUpdates = useCallback(
    async ({ manual = false, force = false }: CheckForUpdatesOptions = {}) => {
      if (manual) {
        manualCheckRequested.current = true;
        showCheckingForUpdatesToast();
      }

      if (checkInFlight.current) return;

      const now = Date.now();
      if (
        !manual &&
        !force &&
        lastCheckAt.current !== null &&
        now - lastCheckAt.current < LIFECYCLE_UPDATE_CHECK_COOLDOWN_MS
      ) {
        return;
      }

      checkInFlight.current = true;
      lastCheckAt.current = now;

      try {
        const update = await check();
        const shouldShowManualResult = manualCheckRequested.current;
        if (
          update &&
          (shouldShowManualResult || notifiedVersion.current !== update.version)
        ) {
          notifiedVersion.current = update.version;
          if (shouldShowManualResult) {
            toast.dismiss(UPDATE_CHECK_RESULT_TOAST_ID);
          }
          showUpdateNotification(update, openReleaseNotes);
        } else if (shouldShowManualResult && !update) {
          toast.success("RalphX is up to date.", { id: UPDATE_CHECK_RESULT_TOAST_ID });
        }
      } catch (error) {
        console.debug("Update check failed:", error);
        if (manualCheckRequested.current) {
          toast.error("Failed to check for updates. Please try again later.", {
            id: UPDATE_CHECK_RESULT_TOAST_ID,
          });
        }
      } finally {
        checkInFlight.current = false;
        manualCheckRequested.current = false;
      }
    },
    [openReleaseNotes],
  );

  const showStartupReleaseNotes = useCallback(async () => {
    try {
      const [notes, lastSeenVersion] = await Promise.all([
        getCurrentReleaseNotes(),
        getLastSeenReleaseNotesVersion(),
      ]);
      const body = sanitizeReleaseNotesBody(notes.body);

      if (
        !body ||
        lastSeenVersion === notes.version ||
        whatsNewVersion.current === notes.version
      ) {
        return;
      }

      whatsNewVersion.current = notes.version;
      presentWhatsNewToast({ version: notes.version, body, context: "current" });
    } catch (error) {
      console.debug("Release notes startup check failed:", error);
    }
  }, [presentWhatsNewToast]);

  useEffect(() => {
    isGlobalModalOpen.current = activeModal !== null;
    if (activeModal !== null) {
      if (visibleWhatsNew.current && visibleWhatsNewToastId.current) {
        pendingWhatsNew.current = visibleWhatsNew.current;
        toast.dismiss(visibleWhatsNewToastId.current);
        clearVisibleWhatsNew();
      }
      return;
    }

    if (pendingWhatsNew.current) {
      presentWhatsNewToast(pendingWhatsNew.current);
    }
  }, [activeModal, clearVisibleWhatsNew, presentWhatsNewToast]);

  useEffect(() => {
    const timeoutId = window.setTimeout(
      () => void checkForUpdates({ force: true }),
      INITIAL_UPDATE_CHECK_DELAY_MS,
    );
    const intervalId = window.setInterval(
      () => void checkForUpdates({ force: true }),
      UPDATE_CHECK_INTERVAL_MS,
    );
    return () => {
      window.clearTimeout(timeoutId);
      window.clearInterval(intervalId);
    };
  }, [checkForUpdates]);

  useEffect(() => {
    const timeoutId = window.setTimeout(
      () => void showStartupReleaseNotes(),
      STARTUP_RELEASE_NOTES_DELAY_MS,
    );
    return () => window.clearTimeout(timeoutId);
  }, [showStartupReleaseNotes]);

  useEffect(() => {
    const checkIfActive = () => {
      if (document.visibilityState === "hidden") return;
      void checkForUpdates();
    };

    window.addEventListener("focus", checkIfActive);
    window.addEventListener("online", checkIfActive);
    document.addEventListener("visibilitychange", checkIfActive);

    return () => {
      window.removeEventListener("focus", checkIfActive);
      window.removeEventListener("online", checkIfActive);
      document.removeEventListener("visibilitychange", checkIfActive);
    };
  }, [checkForUpdates]);

  useEffect(() => {
    const unlisteners: UnlistenFn[] = [];
    let isMounted = true;

    void listen(UPDATE_CHECK_EVENT, () => {
      void checkForUpdates({ manual: true, force: true });
    }).then((unlisten) => {
      if (isMounted) {
        unlisteners.push(unlisten);
      } else {
        unlisten();
      }
    });

    void listen(RELEASE_NOTES_EVENT, () => {
      void openCurrentReleaseNotes();
    }).then((unlisten) => {
      if (isMounted) {
        unlisteners.push(unlisten);
      } else {
        unlisten();
      }
    });

    return () => {
      isMounted = false;
      unlisteners.forEach((unlisten) => unlisten());
    };
  }, [checkForUpdates, openCurrentReleaseNotes]);

  return (
    <ReleaseNotesDialog
      open={dialogState.open}
      onClose={() => setDialogState({ open: false })}
      initialVersion={dialogState.version}
      initialBody={dialogState.body}
      initialContext={dialogState.context}
      onRequestUpdate={handleUpdateFromDialog}
    />
  );
}

function showCheckingForUpdatesToast() {
  toast.loading("Checking for updates...", { id: UPDATE_CHECK_RESULT_TOAST_ID });
}

function showUpdateNotification(
  update: Update,
  onOpenReleaseNotes: (notes: ReleaseNotesView) => void,
) {
  const notes = sanitizeReleaseNotesBody(typeof update.body === "string" ? update.body : null);
  const releaseNotes = notes
    ? { version: update.version, body: notes, context: "update" as const }
    : null;

  toast(
    <div className="flex flex-col gap-2" data-testid="update-available-toast">
      <div className="flex items-center gap-2">
        <Download
          className="h-4 w-4"
          style={{ color: "var(--accent-primary)" }}
        />
        <span className="font-medium">Update available</span>
      </div>
      <p
        className="text-xs"
        style={{ color: "var(--text-muted)", lineHeight: 1.4 }}
      >
        Version {update.version} is ready to install.
      </p>
      {notes ? (
        <p
          className="line-clamp-2 text-xs"
          style={{ color: "var(--text-muted)", lineHeight: 1.4 }}
        >
          {notes}
        </p>
      ) : null}
      <div className="flex gap-2 mt-1">
        <button
          type="button"
          data-testid="update-install-button"
          className="git-auth-startup-toast-action inline-flex h-7 items-center rounded-[6px] px-3 text-xs font-semibold"
          onClick={() => installUpdate(update)}
        >
          Update Now
        </button>
        {releaseNotes ? (
          <button
            type="button"
            data-testid="update-release-notes-button"
            className="inline-flex h-7 items-center rounded-[6px] px-3 text-xs font-medium"
            style={{ color: "var(--accent-primary)" }}
            onClick={() => onOpenReleaseNotes(releaseNotes)}
          >
            Release Notes
          </button>
        ) : null}
        <button
          type="button"
          data-testid="update-later-button"
          className="inline-flex h-7 items-center rounded-[6px] px-3 text-xs font-medium"
          style={{ color: "var(--text-muted)" }}
          onClick={() => toast.dismiss("update-available")}
        >
          Later
        </button>
      </div>
    </div>,
    {
      duration: Infinity,
      id: "update-available",
      className: "git-auth-startup-toast",
    }
  );
}

function showWhatsNewToast(
  releaseNotes: ReleaseNotesView,
  onOpenReleaseNotes: (notes: ReleaseNotesView) => void,
  onDismiss: () => void,
) {
  const toastId = whatsNewToastId(releaseNotes.version);

  toast(
    <div className="flex flex-col gap-2" data-testid="whats-new-toast">
      <div className="flex items-center gap-2">
        <Sparkles
          className="h-4 w-4"
          style={{ color: "var(--accent-primary)" }}
        />
        <span className="font-medium">What&apos;s new in RalphX {releaseNotes.version}</span>
      </div>
      <p
        className="line-clamp-2 text-xs"
        style={{ color: "var(--text-muted)", lineHeight: 1.4 }}
      >
        {releaseNotesPreview(releaseNotes.body)}
      </p>
      <div className="flex gap-2 mt-1">
        <button
          type="button"
          data-testid="whats-new-open-button"
          className="git-auth-startup-toast-action inline-flex h-7 items-center rounded-[6px] px-3 text-xs font-semibold"
          onClick={() => onOpenReleaseNotes(releaseNotes)}
        >
          Read Release Notes
        </button>
        <button
          type="button"
          data-testid="whats-new-dismiss-button"
          className="inline-flex h-7 items-center rounded-[6px] px-3 text-xs font-medium"
          style={{ color: "var(--text-muted)" }}
          onClick={() => {
            void markReleaseNotesSeen(releaseNotes.version).catch((error) => {
              console.debug("Failed to mark release notes as seen:", error);
            });
            toast.dismiss(toastId);
            onDismiss();
          }}
        >
          Dismiss
        </button>
      </div>
    </div>,
    {
      duration: Infinity,
      id: toastId,
      className: "git-auth-startup-toast",
    },
  );
}

function whatsNewToastId(version: string): string {
  return `whats-new-${version}`;
}

function sanitizeReleaseNotesBody(body: string | null): string | null {
  if (!body) {
    return null;
  }

  const sanitized = body
    .replace(GITHUB_RELEASE_METADATA_MARKERS, "")
    .replace(/\n{3,}/g, "\n\n")
    .trim();

  return sanitized.length > 0 ? sanitized : null;
}

function releaseNotesPreview(body: string | null): string {
  return (body ?? "")
    .replace(/^#+\s+/gm, "")
    .replace(/\*\*/g, "")
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean)
    .slice(0, 2)
    .join(" ");
}

async function installUpdate(update: Update) {
  const toastId = "update-progress";

  toast.dismiss("update-available");
  toast.loading("Downloading update...", { id: toastId });

  try {
    let totalBytes = 0;
    let downloadedBytes = 0;

    await update.downloadAndInstall((progress) => {
      if (progress.event === "Started" && progress.data.contentLength) {
        totalBytes = progress.data.contentLength;
      } else if (progress.event === "Progress") {
        downloadedBytes += progress.data.chunkLength;
        if (totalBytes > 0) {
          const percent = Math.round((downloadedBytes / totalBytes) * 100);
          toast.loading(`Downloading update... ${percent}%`, { id: toastId });
        }
      } else if (progress.event === "Finished") {
        toast.loading("Installing update...", { id: toastId });
      }
    });

    toast.success("Update installed! Restarting...", { id: toastId });

    // Give user a moment to see the success message
    setTimeout(() => {
      markPostUpdatePreparing(update.version);
      void relaunch().catch((error) => {
        clearPostUpdatePreparing();
        toast.error("Failed to restart RalphX. Please reopen the app manually.", {
          id: toastId,
        });
        console.error("Update relaunch failed:", error);
      });
    }, 1500);
  } catch (error) {
    toast.error("Failed to install update. Please try again later.", {
      id: toastId,
    });
    console.error("Update installation failed:", error);
  }
}
