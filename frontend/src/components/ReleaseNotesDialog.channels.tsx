import type { KeyboardEvent } from "react";
import { ArrowUpCircle, FileText, Loader2 } from "lucide-react";

import type { ReleaseMetadataAvailability } from "@/api/release-notes";
import type { UpdateChannel } from "@/api/update-channel";
import {
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";

const CHANNEL_LABEL: Record<UpdateChannel, "Stable" | "Nightly"> = {
  stable: "Stable",
  nightly: "Nightly",
};

const CHANNELS: UpdateChannel[] = ["stable", "nightly"];

interface ReleaseNotesChannelsProps {
  browseChannel: UpdateChannel | null;
  contextLabel: string;
  initialChannel: UpdateChannel | undefined;
  isBrowsingPersistedChannel: boolean;
  isUpdateChannelError: boolean;
  isUpdateChannelLoading: boolean;
  isUpdateChannelSaving: boolean;
  isUpdateChannelSettled: boolean;
  metadataAvailability: ReleaseMetadataAvailability | null;
  onBrowseChannel: (channel: UpdateChannel) => void;
  onCheckForUpdates: ((channel: UpdateChannel) => void) | undefined;
  onUseBrowseChannel: () => void;
  updateChannel: UpdateChannel;
  updateChannelSaveError: Error | null;
  versionsByChannel: Record<UpdateChannel, string[]>;
}

export function ReleaseNotesChannels({
  browseChannel,
  contextLabel,
  initialChannel,
  isBrowsingPersistedChannel,
  isUpdateChannelError,
  isUpdateChannelLoading,
  isUpdateChannelSaving,
  isUpdateChannelSettled,
  metadataAvailability,
  onBrowseChannel,
  onCheckForUpdates,
  onUseBrowseChannel,
  updateChannel,
  updateChannelSaveError,
  versionsByChannel,
}: ReleaseNotesChannelsProps) {
  const handleTabKeyDown = (
    event: KeyboardEvent<HTMLButtonElement>,
    channel: UpdateChannel,
  ) => {
    const currentIndex = CHANNELS.indexOf(channel);
    let nextIndex: number | null = null;
    if (event.key === "ArrowRight") {
      nextIndex = (currentIndex + 1) % CHANNELS.length;
    }
    if (event.key === "ArrowLeft") {
      nextIndex = (currentIndex - 1 + CHANNELS.length) % CHANNELS.length;
    }
    if (event.key === "Home") nextIndex = 0;
    if (event.key === "End") nextIndex = CHANNELS.length - 1;
    if (nextIndex === null) return;

    event.preventDefault();
    const nextChannel = CHANNELS[nextIndex];
    if (nextChannel === undefined) return;
    onBrowseChannel(nextChannel);
    document.getElementById(`release-notes-tab-${nextChannel}`)?.focus();
  };

  return (
    <DialogHeader
      className="shrink-0 border-b px-6 py-4 pr-14"
      style={{ borderColor: "var(--border-subtle)" }}
    >
      <div className="flex min-w-0 items-center justify-between gap-3">
        <div className="flex min-w-0 items-center gap-2">
          <FileText
            className="h-4 w-4 shrink-0"
            style={{ color: "var(--accent-primary)" }}
          />
          <div className="min-w-0">
            <DialogTitle id="release-notes-dialog-title" className="truncate">
              Release Notes
            </DialogTitle>
            <DialogDescription id="release-notes-dialog-description">
              {contextLabel}
            </DialogDescription>
          </div>
        </div>
        {isUpdateChannelSettled &&
          isBrowsingPersistedChannel &&
          browseChannel !== null &&
          onCheckForUpdates && (
            <button
              type="button"
              data-testid="release-notes-check-updates-button"
              className="inline-flex shrink-0 items-center gap-1.5 rounded-md px-3 py-1.5 text-xs font-semibold transition-opacity hover:opacity-90"
              style={{
                backgroundColor: "var(--accent-primary)",
                color: "white",
              }}
              onClick={() => onCheckForUpdates(browseChannel)}
            >
              <ArrowUpCircle className="h-3.5 w-3.5" />
              Check {CHANNEL_LABEL[browseChannel]} for updates
            </button>
          )}
        {isUpdateChannelSettled &&
          browseChannel !== null &&
          !isBrowsingPersistedChannel && (
            <button
              type="button"
              data-testid="release-notes-use-channel-button"
              className="inline-flex shrink-0 items-center gap-1.5 rounded-md px-3 py-1.5 text-xs font-semibold transition-opacity hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-60"
              style={{
                backgroundColor: "var(--accent-primary)",
                color: "white",
              }}
              disabled={isUpdateChannelSaving}
              onClick={onUseBrowseChannel}
            >
              {isUpdateChannelSaving && (
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
              )}
              {isUpdateChannelSaving
                ? "Switching..."
                : `Use ${CHANNEL_LABEL[browseChannel]}`}
            </button>
          )}
      </div>

      <div
        className="mt-3 inline-flex w-fit rounded-lg p-1"
        role="tablist"
        aria-label="Release channel history"
        style={{ backgroundColor: "var(--bg-surface)" }}
      >
        {CHANNELS.map((channel) => {
          const isActive = browseChannel === channel;
          const isCurrent =
            isUpdateChannelSettled && updateChannel === channel;
          const count = versionsByChannel[channel].length;
          return (
            <button
              key={channel}
              id={`release-notes-tab-${channel}`}
              type="button"
              role="tab"
              aria-selected={isActive}
              aria-controls="release-notes-panel"
              tabIndex={isActive ? 0 : -1}
              data-testid={`release-notes-channel-${channel}`}
              className="inline-flex min-w-28 items-center justify-center gap-1.5 rounded-md px-3 py-1.5 text-xs font-medium transition-colors"
              style={{
                backgroundColor: isActive
                  ? "var(--bg-elevated)"
                  : "transparent",
                color: isActive ? "var(--text-primary)" : "var(--text-muted)",
              }}
              onClick={() => onBrowseChannel(channel)}
              onKeyDown={(event) => handleTabKeyDown(event, channel)}
            >
              <span>{CHANNEL_LABEL[channel]}</span>
              <span
                className="tabular-nums"
                style={{ color: "var(--text-muted)" }}
              >
                {count}
              </span>
              {isCurrent && (
                <span
                  className="text-[0.625rem] font-semibold uppercase tracking-wide"
                  style={{ color: "var(--accent-primary)" }}
                >
                  Current
                </span>
              )}
            </button>
          );
        })}
      </div>

      {isUpdateChannelLoading && initialChannel === undefined && (
        <p className="mt-2 text-xs" style={{ color: "var(--text-muted)" }}>
          Loading your update channel...
        </p>
      )}
      {isUpdateChannelError && (
        <p className="mt-2 text-xs" style={{ color: "var(--text-muted)" }}>
          Could not load your saved channel. Stable is being used.
        </p>
      )}
      {updateChannelSaveError && !isBrowsingPersistedChannel && (
        <p
          className="mt-2 text-xs"
          role="alert"
          style={{ color: "var(--status-error, #ef4444)" }}
        >
          Could not switch update channels. Try again.
        </p>
      )}
      {metadataAvailability === "unavailable" && (
        <p
          className="mt-2 text-xs"
          role="alert"
          style={{ color: "var(--text-muted)" }}
        >
          Release history is temporarily unavailable. Your current release
          notes are still available.
        </p>
      )}
      {metadataAvailability === "stale" && (
        <p
          className="mt-2 text-xs"
          role="status"
          style={{ color: "var(--text-muted)" }}
        >
          Showing last-known release classification while GitHub refreshes.
        </p>
      )}
    </DialogHeader>
  );
}
