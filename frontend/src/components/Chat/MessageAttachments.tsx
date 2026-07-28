/**
 * MessageAttachments - Sent-message attachment previews and file chips
 *
 * Displays image attachments as previews and other files as compact chips.
 * Used above message text bubbles for user messages with attachments.
 */

import { FileText, Image, FileCode, File } from "lucide-react";
import { useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";

import { HostPathCopyButton } from "@/components/remote/HostPathCopyButton";
import { useIsRemoteEnvironment } from "@/hooks/useActiveEnvironment";
import { HOST_ATTACHMENT_HINT } from "@/lib/remote/host-affordances";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogTitle,
} from "@/components/ui/dialog";
import { cn } from "@/lib/utils";

// ============================================================================
// Types
// ============================================================================

export interface MessageAttachment {
  /** Unique identifier for the attachment */
  id: string;
  /** File name */
  fileName: string;
  /** File size in bytes */
  fileSize: number;
  /** MIME type of the file */
  mimeType?: string;
  /** File path for opening/downloading */
  filePath?: string;
  /** Frontend-only preview URL for optimistic local files */
  previewUrl?: string;
}

export interface MessageAttachmentsProps {
  /** Array of attachments to display */
  attachments: MessageAttachment[];
  /** Callback when attachment is clicked (optional - can be placeholder for v1) */
  onClick?: (id: string, filePath: string | undefined) => void;
  /** Side to align the sent-message attachment group within the chat row. */
  align?: "start" | "end";
}

// ============================================================================
// Helpers
// ============================================================================

/**
 * Get appropriate icon for file type
 */
function getFileIcon(mimeType?: string, fileName?: string) {
  if (mimeType?.startsWith("image/")) {
    return <Image className="w-3 h-3" />;
  }
  if (mimeType?.startsWith("text/")) {
    return <FileText className="w-3 h-3" />;
  }
  if (mimeType === "application/pdf") {
    return <FileText className="w-3 h-3" />;
  }
  if (fileName?.match(/\.(js|ts|tsx|jsx|py|rs|go|java|cpp|c|h)$/)) {
    return <FileCode className="w-3 h-3" />;
  }
  return <File className="w-3 h-3" />;
}

/**
 * Format file size for display
 */
function formatFileSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

/**
 * Preview source for an attachment, or `null` when there is nothing honest to show.
 *
 * `convertFileSrc` mints an `asset://` URL for a path on THIS device's filesystem.
 * Under a remote environment `attachment.filePath` names a file on the HOST, so the
 * URL would resolve to nothing and the user would get a broken image icon with no
 * explanation. Returning `null` routes every remote attachment through the
 * placeholder card instead (2.6-a, Fixed Decision 14).
 *
 * Real remote attachment rendering needs the 1.5-C `/remote/v1/attachments/{id}`
 * endpoint, 2.7's response-header envelope, and a binary-safe body — none of which
 * exist on this base. That work is DEFERRED TO 3.1; this is the honest interim.
 *
 * `previewUrl` is deliberately still honoured: when a caller already resolved a
 * displayable URL by some other means, that is not a local-filesystem assumption.
 */
function getImagePreviewSrc(
  attachment: MessageAttachment,
  isRemoteEnvironment: boolean
): string | null {
  if (!attachment.mimeType?.startsWith("image/")) {
    return null;
  }

  if (attachment.previewUrl) {
    return attachment.previewUrl;
  }

  if (!attachment.filePath || isRemoteEnvironment) {
    return null;
  }

  return convertFileSrc(attachment.filePath);
}

interface AttachmentPreviewEntry {
  attachment: MessageAttachment;
  previewSrc: string | null;
}

// ============================================================================
// Component
// ============================================================================

export function MessageAttachments({
  attachments,
  onClick,
  align = "start",
}: MessageAttachmentsProps) {
  const [failedPreviewIds, setFailedPreviewIds] = useState<Set<string>>(() => new Set());
  const [selectedImageId, setSelectedImageId] = useState<string | null>(null);
  const isRemoteEnvironment = useIsRemoteEnvironment();

  if (attachments.length === 0) {
    return null;
  }

  const attachmentEntries: AttachmentPreviewEntry[] = attachments.map((attachment) => ({
    attachment,
    previewSrc: failedPreviewIds.has(attachment.id)
      ? null
      : getImagePreviewSrc(attachment, isRemoteEnvironment),
  }));
  const imageEntries = attachmentEntries.filter((entry) => entry.previewSrc !== null);
  const fileEntries = attachmentEntries.filter((entry) => entry.previewSrc === null);
  const selectedImageEntry =
    imageEntries.find((entry) => entry.attachment.id === selectedImageId) ?? null;
  const alignEnd = align === "end";

  const markPreviewFailed = (attachmentId: string) => {
    setFailedPreviewIds((current) => {
      if (current.has(attachmentId)) return current;
      const next = new Set(current);
      next.add(attachmentId);
      return next;
    });
    setSelectedImageId((current) => current === attachmentId ? null : current);
  };

  return (
    <div
      data-testid="message-attachment-list"
      className={cn("mb-2 space-y-2", alignEnd && "self-end")}
    >
      {imageEntries.length > 0 && (
        <div
          data-testid="attachment-image-grid"
          className={cn(
            "grid gap-2",
            imageEntries.length === 1 ? "grid-cols-1 max-w-[280px]" : "grid-cols-2 max-w-[420px]",
            alignEnd && "ml-auto",
          )}
        >
          {imageEntries.map(({ attachment, previewSrc }) => (
            <button
              key={attachment.id}
              data-testid="attachment-image-tile"
              type="button"
              onClick={() => {
                setSelectedImageId(attachment.id);
                onClick?.(attachment.id, attachment.filePath);
              }}
              className="group min-w-0 overflow-hidden rounded-md text-left transition-colors focus:outline-none focus:ring-2 focus:ring-[var(--accent-primary)]"
              style={{
                background: "var(--bg-elevated)",
                border: "1px solid var(--bg-hover)",
              }}
              title={attachment.fileName}
              aria-label={`Open ${attachment.fileName}`}
            >
              <span
                className="block aspect-[4/3] w-full overflow-hidden"
                style={{ background: "var(--bg-surface)" }}
              >
                <img
                  data-testid="attachment-image-preview"
                  src={previewSrc ?? ""}
                  alt={attachment.fileName}
                  loading="lazy"
                  className="h-full w-full object-cover transition-transform group-hover:scale-[1.02]"
                  onError={() => markPreviewFailed(attachment.id)}
                />
              </span>
              <span className="flex min-w-0 items-center justify-between gap-2 px-2 py-1.5">
                <span
                  className="min-w-0 truncate text-xs"
                  style={{ color: "var(--text-primary)" }}
                  title={attachment.fileName}
                >
                  {attachment.fileName}
                </span>
                <span
                  className="shrink-0 text-[0.625rem]"
                  style={{ color: "var(--text-muted)" }}
                >
                  {formatFileSize(attachment.fileSize)}
                </span>
              </span>
            </button>
          ))}
        </div>
      )}

      {fileEntries.length > 0 && (
        <div className={cn("flex flex-wrap gap-2", alignEnd && "justify-end")}>
          {fileEntries.map(({ attachment }) => (
            <AttachmentChip
              key={attachment.id}
              attachment={attachment}
              onClick={onClick}
              isRemoteEnvironment={isRemoteEnvironment}
            />
          ))}
        </div>
      )}

      <Dialog
        open={selectedImageEntry !== null}
        onOpenChange={(open) => {
          if (!open) setSelectedImageId(null);
        }}
      >
        {selectedImageEntry && (
          <DialogContent
            data-testid="attachment-image-dialog"
            className="max-h-[90vh] max-w-[min(92vw,1100px)] overflow-hidden p-0"
          >
            <div
              className="flex min-w-0 items-center gap-3 border-b px-4 py-3 pr-14"
              style={{ borderColor: "var(--border-subtle)" }}
            >
              <DialogTitle className="min-w-0 truncate text-sm">
                {selectedImageEntry.attachment.fileName}
              </DialogTitle>
              <DialogDescription className="sr-only">
                Preview image attachment {selectedImageEntry.attachment.fileName}.
              </DialogDescription>
              <span
                className="shrink-0 text-xs"
                style={{ color: "var(--text-muted)" }}
              >
                {formatFileSize(selectedImageEntry.attachment.fileSize)}
              </span>
            </div>
            <div
              className="max-h-[calc(90vh-4rem)] overflow-auto p-3"
              style={{ background: "var(--bg-surface)" }}
            >
              <img
                data-testid="attachment-image-large"
                src={selectedImageEntry.previewSrc ?? ""}
                alt={selectedImageEntry.attachment.fileName}
                className="mx-auto max-h-[calc(90vh-6rem)] max-w-full object-contain"
                onError={() => markPreviewFailed(selectedImageEntry.attachment.id)}
              />
            </div>
          </DialogContent>
        )}
      </Dialog>
    </div>
  );
}

function AttachmentChip({
  attachment,
  onClick,
  isRemoteEnvironment = false,
}: {
  attachment: MessageAttachment;
  onClick: ((id: string, filePath: string | undefined) => void) | undefined;
  isRemoteEnvironment?: boolean;
}) {
  // The chip's click opens the file on this device; on a remote host there is
  // nothing to open, so the card states where the file is and offers the path.
  const hostPath = isRemoteEnvironment ? attachment.filePath : undefined;

  return (
    <span
      data-testid={isRemoteEnvironment ? "attachment-host-card" : undefined}
      className="inline-flex items-center gap-1.5"
    >
    <button
      data-testid="attachment-chip"
      type="button"
      disabled={isRemoteEnvironment}
      onClick={
        isRemoteEnvironment
          ? undefined
          : () => onClick?.(attachment.id, attachment.filePath)
      }
      className="flex items-center gap-1.5 rounded px-2 py-1 transition-all"
      style={{
        background: "var(--bg-elevated)",
        border: "1px solid var(--bg-hover)",
      }}
      onMouseEnter={(e: React.MouseEvent<HTMLButtonElement>) => {
        e.currentTarget.style.background = "var(--bg-hover)";
      }}
      onMouseLeave={(e: React.MouseEvent<HTMLButtonElement>) => {
        e.currentTarget.style.background = "var(--bg-elevated)";
      }}
      title={attachment.fileName}
    >
      <span
        className="shrink-0"
        style={{
          color: "var(--text-secondary)",
        }}
      >
        {getFileIcon(attachment.mimeType, attachment.fileName)}
      </span>

      <span
        className="max-w-[180px] text-xs"
        style={{
          color: "var(--text-primary)",
          overflow: "hidden",
          textOverflow: "ellipsis",
          whiteSpace: "nowrap",
        }}
        title={attachment.fileName}
      >
        {attachment.fileName}
      </span>

      <span
        className="text-[0.625rem]"
        style={{
          color: "var(--text-muted)",
        }}
      >
        {formatFileSize(attachment.fileSize)}
      </span>

      {isRemoteEnvironment ? (
        <span
          className="text-[0.625rem]"
          style={{ color: "var(--text-muted)" }}
          data-testid="attachment-host-hint"
        >
          {HOST_ATTACHMENT_HINT}
        </span>
      ) : null}
    </button>
    {hostPath ? (
      <HostPathCopyButton path={hostPath} testId="attachment-host-copy" />
    ) : null}
    </span>
  );
}
