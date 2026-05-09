import { lazy, Suspense, useCallback, useEffect, useRef, useState } from "react";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { toast } from "sonner";
import { Download, FileText, Sparkles } from "lucide-react";
import {
  getCurrentReleaseNotes,
  getLastSeenReleaseNotesVersion,
  markReleaseNotesSeen,
} from "@/api/release-notes";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { markdownComponents } from "@/components/Chat/MessageItem.markdown";

const INITIAL_UPDATE_CHECK_DELAY_MS = 3_000;
const STARTUP_RELEASE_NOTES_DELAY_MS = 4_000;
const UPDATE_CHECK_INTERVAL_MS = 30 * 60 * 1_000;
const LIFECYCLE_UPDATE_CHECK_COOLDOWN_MS = 5 * 60 * 1_000;
const UPDATE_CHECK_EVENT = "ralphx://check-for-updates";
const RELEASE_NOTES_EVENT = "ralphx://show-release-notes";

interface ReleaseNotesView {
  version: string;
  body: string | null;
  context: "current" | "update";
}

interface CheckForUpdatesOptions {
  manual?: boolean;
  force?: boolean;
}

const LazyReleaseNotesMarkdown = lazy(async () => {
  const [{ default: ReactMarkdown }, { default: remarkGfm }] = await Promise.all([
    import("react-markdown"),
    import("remark-gfm"),
  ]);

  return {
    default: function ReleaseNotesMarkdown({ body }: { body: string }) {
      return (
        <ReactMarkdown remarkPlugins={[remarkGfm]} components={markdownComponents}>
          {body}
        </ReactMarkdown>
      );
    },
  };
});

export function UpdateChecker() {
  const checkInFlight = useRef(false);
  const notifiedVersion = useRef<string | null>(null);
  const lastCheckAt = useRef<number | null>(null);
  const whatsNewVersion = useRef<string | null>(null);
  const [releaseNotes, setReleaseNotes] = useState<ReleaseNotesView | null>(null);

  const openCurrentReleaseNotes = useCallback(async () => {
    try {
      const notes = await getCurrentReleaseNotes();
      setReleaseNotes({
        version: notes.version,
        body: notes.body,
        context: "current",
      });
    } catch (error) {
      console.error("Failed to load release notes:", error);
      toast.error("Failed to open release notes. Please try again later.", {
        id: "release-notes-error",
      });
    }
  }, []);

  const checkForUpdates = useCallback(
    async ({ manual = false, force = false }: CheckForUpdatesOptions = {}) => {
      if (checkInFlight.current) return;

      const now = Date.now();
      if (
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
        if (update && notifiedVersion.current !== update.version) {
          notifiedVersion.current = update.version;
          showUpdateNotification(update, (notes) => setReleaseNotes(notes));
        } else if (manual && !update) {
          toast.success("RalphX is up to date.", { id: "update-check-result" });
        }
      } catch (error) {
        console.debug("Update check failed:", error);
        if (manual) {
          toast.error("Failed to check for updates. Please try again later.", {
            id: "update-check-result",
          });
        }
      } finally {
        checkInFlight.current = false;
      }
    },
    [],
  );

  const showStartupReleaseNotes = useCallback(async () => {
    try {
      const [notes, lastSeenVersion] = await Promise.all([
        getCurrentReleaseNotes(),
        getLastSeenReleaseNotesVersion(),
      ]);

      if (
        !notes.body ||
        lastSeenVersion === notes.version ||
        whatsNewVersion.current === notes.version
      ) {
        return;
      }

      whatsNewVersion.current = notes.version;
      showWhatsNewToast(
        { version: notes.version, body: notes.body, context: "current" },
        (releaseNotes) => setReleaseNotes(releaseNotes),
      );
    } catch (error) {
      console.debug("Release notes startup check failed:", error);
    }
  }, []);

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
      notes={releaseNotes}
      onClose={() => setReleaseNotes(null)}
    />
  );
}

function showUpdateNotification(
  update: Update,
  onOpenReleaseNotes: (notes: ReleaseNotesView) => void,
) {
  const notes = typeof update.body === "string" ? update.body.trim() : "";
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
) {
  const toastId = `whats-new-${releaseNotes.version}`;

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

function ReleaseNotesDialog({
  notes,
  onClose,
}: {
  notes: ReleaseNotesView | null;
  onClose: () => void;
}) {
  return (
    <Dialog open={Boolean(notes)} onOpenChange={(open) => !open && onClose()}>
      <DialogContent
        className="max-h-[82vh] max-w-2xl overflow-hidden"
        style={{
          backgroundColor: "var(--dialog-bg, var(--bg-elevated))",
          borderColor: "var(--border-subtle)",
        }}
      >
        <DialogHeader>
          <div className="flex min-w-0 items-center gap-2">
            <FileText
              className="h-4 w-4 shrink-0"
              style={{ color: "var(--accent-primary)" }}
            />
            <div className="min-w-0">
              <DialogTitle className="truncate">RalphX {notes?.version}</DialogTitle>
              <DialogDescription>
                {notes?.context === "update" ? "Available update" : "Current version"}
              </DialogDescription>
            </div>
          </div>
        </DialogHeader>
        <div
          className="max-h-[64vh] overflow-y-auto px-6 py-5 text-[0.8125rem] leading-relaxed text-[var(--text-primary)]"
          data-testid="release-notes-dialog-body"
        >
          {notes?.body ? (
            <Suspense fallback={<PlainReleaseNotes body={notes.body} />}>
              <LazyReleaseNotesMarkdown body={notes.body} />
            </Suspense>
          ) : (
            <p style={{ color: "var(--text-muted)" }}>
              Release notes are not available for this version.
            </p>
          )}
        </div>
      </DialogContent>
    </Dialog>
  );
}

function PlainReleaseNotes({ body }: { body: string }) {
  return <pre className="whitespace-pre-wrap font-sans">{body}</pre>;
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
    setTimeout(async () => {
      await relaunch();
    }, 1500);
  } catch (error) {
    toast.error("Failed to install update. Please try again later.", {
      id: toastId,
    });
    console.error("Update installation failed:", error);
  }
}
