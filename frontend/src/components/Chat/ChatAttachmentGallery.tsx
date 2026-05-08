/**
 * ChatAttachmentGallery - Grid display of file attachments
 *
 * Displays file preview cards with type icons, names, sizes, and remove buttons.
 * Supports compact (single-row scroll) and full (multi-row grid) variants.
 */

import { FileText, Image, FileCode, File, X } from "lucide-react";

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

// ============================================================================
// Component
// ============================================================================

export function ChatAttachmentGallery({
  attachments,
  onRemove,
  uploading = false,
  compact = false,
}: ChatAttachmentGalleryProps) {
  if (attachments.length === 0) {
    return null;
  }

  const containerClass = compact
    ? "flex gap-2 overflow-x-auto pb-2" // Single row scroll for compact
    : "grid grid-cols-2 sm:grid-cols-3 gap-2"; // Multi-row grid for full

  return (
    <div data-testid="chat-attachment-gallery" className={containerClass}>
      {attachments.map((attachment) => (
        <AttachmentCard
          key={attachment.id}
          attachment={attachment}
          onRemove={onRemove}
          uploading={uploading}
          compact={compact}
        />
      ))}
    </div>
  );
}

// ============================================================================
// Sub-components
// ============================================================================

interface AttachmentCardProps {
  attachment: ChatAttachment;
  onRemove: ((id: string) => void) | undefined;
  uploading: boolean;
  compact: boolean;
}

function AttachmentCard({
  attachment,
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
      {/* File icon */}
      <div
        className="shrink-0 flex items-center justify-center"
        style={{
          color: "var(--text-secondary)",
        }}
      >
        {getFileIcon(attachment.mimeType, attachment.fileName)}
      </div>

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
