import { relaunch } from "@tauri-apps/plugin-process";
import { type Update } from "@tauri-apps/plugin-updater";
import { toast } from "sonner";

import {
  clearPostUpdatePreparing,
  markPostUpdatePreparing,
} from "@/lib/postUpdatePreparing";

export async function installUpdate(update: Update) {
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
