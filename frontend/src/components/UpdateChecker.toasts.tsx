import { Download, Sparkles } from "lucide-react";
import { toast } from "sonner";
import { type Update } from "@tauri-apps/plugin-updater";

import { markReleaseNotesSeen } from "@/api/release-notes";
import type { UpdateChannel } from "@/api/update-channel";

const GITHUB_RELEASE_METADATA_MARKERS =
  /^[ \t]*<!--\s*github-release-metadata:(?:start|end)\s*-->[ \t]*\n?/gm;

export interface ReleaseNotesView {
  version: string;
  body: string | null;
  context: "current" | "update";
  channel: UpdateChannel;
}

export function updateChannelLabel(channel: UpdateChannel): "Stable" | "Nightly" {
  return channel === "nightly" ? "Nightly" : "Stable";
}

export function showUpdateNotification(
  update: Update,
  channel: UpdateChannel,
  onOpenReleaseNotes: (notes: ReleaseNotesView) => void,
  onInstallUpdate: (update: Update) => void,
) {
  const notes = sanitizeReleaseNotesBody(typeof update.body === "string" ? update.body : null);
  const releaseNotes = notes
    ? {
        version: update.version,
        body: notes,
        context: "update" as const,
        channel,
      }
    : null;

  toast(
    <div className="flex flex-col gap-2" data-testid="update-available-toast">
      <div className="flex items-center gap-2">
        <Download
          className="h-4 w-4"
          style={{ color: "var(--accent-primary)" }}
        />
        <span className="font-medium">{updateChannelLabel(channel)} update available</span>
      </div>
      <p
        className="text-xs"
        style={{ color: "var(--text-muted)", lineHeight: 1.4 }}
      >
        Version {update.version} is ready to install from the {updateChannelLabel(channel)} channel.
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
          onClick={() => onInstallUpdate(update)}
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

export function showWhatsNewToast(
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

export function whatsNewToastId(version: string): string {
  return `whats-new-${version}`;
}

export function sanitizeReleaseNotesBody(body: string | null): string | null {
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
