/**
 * ChatAttachmentGallery - Grid display of file attachments
 *
 * Displays image previews or file cards with names, sizes, and remove buttons.
 * Supports compact (single-row scroll) and full (multi-row grid) variants.
 */

import { FileText, Image, FileCode, File, X } from "lucide-react";
import { useEffect, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { useIsRemoteEnvironment } from "@/hooks/useActiveEnvironment";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogTitle,
} from "@/components/ui/dialog";

// ============================================================================
// Types
// ============================================================================

export interface ChatAttachment {
  /** Unique identifier for the attachment */
  id: string;
  /** File name */
  fileName: string;
  /** File size in bytes */
  fileSize: number;
  /** MIME type of the file */
  mimeType?: string;
  /** Local file while the attachment is still optimistic/pre-upload. */
  file?: File;
  /** Frontend-only preview URL for optimistic files. */
  previewUrl?: string;
  /** Durable path returned after upload. */
  filePath?: string;
}

export interface ChatAttachmentGalleryProps {
  /** Array of attachments to display */
  attachments: ChatAttachment[];
  /** Callback when remove button is clicked */
  onRemove?: (id: string) => void;
  /** Show upload progress indicator */
  uploading?: boolean;
  /** Compact variant for input area (single row scroll) */
  compact?: boolean;
}

// ============================================================================
// Helpers
// ============================================================================

/**
 * Get appropriate icon for file type
 */
function getFileIcon(mimeType?: string, fileName?: string) {
  if (mimeType?.startsWith("image/")) {
    return <Image className="w-4 h-4" />;
  }
  if (mimeType?.startsWith("text/")) {
    return <FileText className="w-4 h-4" />;
  }
  if (fileName?.match(/\.(js|ts|tsx|jsx|py|rs|go|java|cpp|c|h)$/)) {
    return <FileCode className="w-4 h-4" />;
  }
  return <File className="w-4 h-4" />;
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
 * Resolve a displayable preview URL, or `null` when none can be minted on THIS device.
 *
 * `convertFileSrc` mints an `asset://` URL against this device's filesystem. Under a
 * remote environment `attachment.filePath` names a file on the HOST, so the URL resolves
 * to nothing and the user gets a broken image with no explanation. This mirrors the branch
 * `MessageAttachments.getImagePreviewSrc` already carries (2.6-a, Fixed Decision 14) —
 * 2.6 hardened only that renderer, leaving this one to render broken images for the same
 * attachment the message surface rendered as a placeholder.
 *
 * Two sources are deliberately still honoured under a remote environment, because neither
 * assumes the host filesystem:
 * - `previewUrl`, which a caller already resolved by some other means;
 * - `localPreviewUrls`, the `createObjectURL` blobs for files the user just picked on THIS
 *   device and has not uploaded yet.
 *
 * Real remote rendering of a HOST attachment needs the scoped attachments endpoint and a
 * binary-safe body; until then this is the honest interim.
 */
function getImagePreviewSrc(
  attachment: ChatAttachment,
  localPreviewUrls: Record<string, string>,
  isRemoteEnvironment: boolean,
): string | null {
  if (!attachment.mimeType?.startsWith("image/")) {
    return null;
  }

  if (attachment.previewUrl) {
    return attachment.previewUrl;
  }

  const localPreviewUrl = localPreviewUrls[attachment.id];
  if (localPreviewUrl) {
    return localPreviewUrl;
  }

  if (attachment.filePath && !isRemoteEnvironment) {
    return convertFileSrc(attachment.filePath);
  }

  return null;
}

interface AttachmentPreviewEntry {
  attachment: ChatAttachment;
  previewSrc: string | null;
}

// ============================================================================
// Component
// ============================================================================

export function ChatAttachmentGallery({
  attachments,
  onRemove,
  uploading = false,
  compact = false,
}: ChatAttachmentGalleryProps) {
  const isRemoteEnvironment = useIsRemoteEnvironment();
  const [localPreviewUrls, setLocalPreviewUrls] = useState<Record<string, string>>({});
  const [failedPreviewIds, setFailedPreviewIds] = useState<Set<string>>(() => new Set());
  const [selectedImageId, setSelectedImageId] = useState<string | null>(null);

  useEffect(() => {
    if (typeof URL === "undefined" || typeof URL.createObjectURL !== "function") {
      setLocalPreviewUrls({});
      return;
    }

    const nextUrls: Record<string, string> = {};
    for (const attachment of attachments) {
      if (
        attachment.file &&
        attachment.mimeType?.startsWith("image/") &&
        !attachment.previewUrl &&
        !attachment.filePath
      ) {
        nextUrls[attachment.id] = URL.createObjectURL(attachment.file);
      }
    }
    setLocalPreviewUrls(nextUrls);

    return () => {
      for (const url of Object.values(nextUrls)) {
        if (typeof URL.revokeObjectURL === "function") {
          URL.revokeObjectURL(url);
        }
      }
    };
  }, [attachments]);

  if (attachments.length === 0) {
    return null;
  }

  const attachmentEntries: AttachmentPreviewEntry[] = attachments.map((attachment) => ({
    attachment,
    previewSrc: failedPreviewIds.has(attachment.id)
      ? null
      : getImagePreviewSrc(attachment, localPreviewUrls, isRemoteEnvironment),
  }));
  const selectedImageEntry =
    attachmentEntries.find(
      (entry) => entry.attachment.id === selectedImageId && entry.previewSrc !== null,
    ) ?? null;

  const markPreviewFailed = (attachmentId: string) => {
    setFailedPreviewIds((current) => {
      if (current.has(attachmentId)) return current;
      const next = new Set(current);
      next.add(attachmentId);
      return next;
    });
    setSelectedImageId((current) => current === attachmentId ? null : current);
  };

  const containerClass = compact
    ? "flex gap-2 overflow-x-auto pb-2" // Single row scroll for compact
    : "grid grid-cols-2 sm:grid-cols-3 gap-2"; // Multi-row grid for full

  return (
    <div data-testid="chat-attachment-gallery" className={containerClass}>
      {attachmentEntries.map(({ attachment, previewSrc }) => (
        <AttachmentCard
          key={attachment.id}
          attachment={attachment}
          previewSrc={previewSrc}
          onPreviewClick={previewSrc ? () => setSelectedImageId(attachment.id) : undefined}
          onPreviewError={() => markPreviewFailed(attachment.id)}
          onRemove={onRemove}
          uploading={uploading}
          compact={compact}
        />
      ))}
      <Dialog
        open={selectedImageEntry !== null}
        onOpenChange={(open) => {
          if (!open) setSelectedImageId(null);
        }}
      >
        {selectedImageEntry && (
          <DialogContent
            data-testid="chat-attachment-image-dialog"
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
              <span className="shrink-0 text-xs" style={{ color: "var(--text-muted)" }}>
                {formatFileSize(selectedImageEntry.attachment.fileSize)}
              </span>
            </div>
            <div
              className="max-h-[calc(90vh-4rem)] overflow-auto p-3"
              style={{ background: "var(--bg-surface)" }}
            >
              <img
                data-testid="chat-attachment-image-large"
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

// ============================================================================
// Sub-components
// ============================================================================

interface AttachmentCardProps {
  attachment: ChatAttachment;
  previewSrc: string | null;
  onPreviewClick: (() => void) | undefined;
  onPreviewError: () => void;
  onRemove: ((id: string) => void) | undefined;
  uploading: boolean;
  compact: boolean;
}

function AttachmentCard({
  attachment,
  previewSrc,
  onPreviewClick,
  onPreviewError,
  onRemove,
  uploading,
  compact,
}: AttachmentCardProps) {
  const cardClass = compact
    ? "flex items-center gap-2 px-2 py-1.5 rounded-lg shrink-0"
    : "flex items-start gap-2 p-2.5 rounded-lg";

  return (
    <div
      data-testid="attachment-card"
      className={cardClass}
      style={{
        background: "var(--bg-surface)",
        border: "1px solid var(--bg-hover)",
      }}
    >
      {previewSrc ? (
        <button
          data-testid="chat-attachment-image-preview-button"
          type="button"
          onClick={onPreviewClick}
          className={compact
            ? "h-10 w-10 shrink-0 overflow-hidden rounded-md"
            : "h-12 w-12 shrink-0 overflow-hidden rounded-md"}
          style={{
            background: "var(--bg-elevated)",
            border: "1px solid var(--bg-hover)",
          }}
          aria-label={`Preview ${attachment.fileName}`}
        >
          <img
            data-testid="chat-attachment-image-preview"
            src={previewSrc}
            alt={attachment.fileName}
            loading="lazy"
            className="h-full w-full object-cover"
            onError={onPreviewError}
          />
        </button>
      ) : (
        <div
          className="shrink-0 flex items-center justify-center"
          style={{
            color: "var(--text-secondary)",
          }}
        >
          {getFileIcon(attachment.mimeType, attachment.fileName)}
        </div>
      )}

      {/* File info */}
      <div className="flex-1 min-w-0">
        <p
          className={compact ? "text-[0.6875rem]" : "text-xs"}
          style={{
            color: "var(--text-primary)",
            overflow: "hidden",
            textOverflow: "ellipsis",
            whiteSpace: "nowrap",
          }}
          title={attachment.fileName}
        >
          {attachment.fileName}
        </p>
        <p
          className="text-[0.625rem]"
          style={{
            color: "var(--text-muted)",
          }}
        >
          {formatFileSize(attachment.fileSize)}
        </p>
      </div>

      {/* Upload progress or remove button */}
      {uploading ? (
        <div
          data-testid="upload-progress"
          className="shrink-0"
          style={{
            color: "var(--accent-primary)",
          }}
        >
          <svg
            className="animate-spin w-4 h-4"
            viewBox="0 0 16 16"
            fill="none"
          >
            <circle
              cx="8"
              cy="8"
              r="6"
              stroke="currentColor"
              strokeWidth="2"
              strokeOpacity="0.3"
            />
            <path
              d="M14 8A6 6 0 0 0 8 2"
              stroke="currentColor"
              strokeWidth="2"
              strokeLinecap="round"
            />
          </svg>
        </div>
      ) : (
        onRemove && (
          <button
            data-testid="remove-attachment"
            type="button"
            onClick={() => onRemove(attachment.id)}
            className="shrink-0 rounded p-0.5 transition-colors hover:brightness-110"
            style={{
              color: "var(--text-secondary)",
              background: "transparent",
            }}
            onMouseEnter={(e: React.MouseEvent<HTMLButtonElement>) => {
              const target = e.currentTarget;
              target.style.background = "var(--bg-hover)";
              target.style.color = "var(--text-primary)";
            }}
            onMouseLeave={(e: React.MouseEvent<HTMLButtonElement>) => {
              const target = e.currentTarget;
              target.style.background = "transparent";
              target.style.color = "var(--text-secondary)";
            }}
            aria-label={`Remove ${attachment.fileName}`}
          >
            <X className="w-3.5 h-3.5" />
          </button>
        )
      )}
    </div>
  );
}
