/**
 * UpdateChecker - Checks for app updates on mount and shows notification
 *
 * Uses tauri-plugin-updater to check GitHub releases for new versions.
 * Shows a toast notification with option to download and install.
 */

import { useEffect, useRef } from "react";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { toast } from "sonner";
import { Download } from "lucide-react";

const INITIAL_UPDATE_CHECK_DELAY_MS = 3_000;
const UPDATE_CHECK_INTERVAL_MS = 30 * 60 * 1_000;

export function UpdateChecker() {
  const checkInFlight = useRef(false);
  const notifiedVersion = useRef<string | null>(null);

  useEffect(() => {
    const checkForUpdates = async () => {
      if (checkInFlight.current) return;
      checkInFlight.current = true;

      try {
        const update = await check();
        if (update && notifiedVersion.current !== update.version) {
          notifiedVersion.current = update.version;
          showUpdateNotification(update);
        }
      } catch (error) {
        // Silently fail - update check is non-critical
        // In development or when endpoint is not configured, this will fail
        console.debug("Update check failed:", error);
      } finally {
        checkInFlight.current = false;
      }
    };

    // Delay initial check to avoid blocking app startup, then keep polling so
    // users who leave RalphX open can still pick up same-day releases.
    const timeoutId = window.setTimeout(checkForUpdates, INITIAL_UPDATE_CHECK_DELAY_MS);
    const intervalId = window.setInterval(checkForUpdates, UPDATE_CHECK_INTERVAL_MS);
    return () => {
      window.clearTimeout(timeoutId);
      window.clearInterval(intervalId);
    };
  }, []);

  return null;
}

function showUpdateNotification(update: Update) {
  const notes = typeof update.body === "string" ? update.body.trim() : "";

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
