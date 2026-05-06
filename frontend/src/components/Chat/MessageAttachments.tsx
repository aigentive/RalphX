/**
 * MessageAttachments - Compact file chips for message attachments
 *
 * Displays file attachments in a horizontal row with icons, names, and sizes.
 * Used above message text bubbles for user messages with attachments.
 */

import { FileText, Image, FileCode, File } from "lucide-react";

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
}

export interface MessageAttachmentsProps {
  /** Array of attachments to display */
  attachments: MessageAttachment[];
  /** Callback when attachment is clicked (optional - can be placeholder for v1) */
  onClick?: (id: string, filePath: string | undefined) => void;
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

// ============================================================================
// Component
// ============================================================================

export function MessageAttachments({
  attachments,
  onClick,
}: MessageAttachmentsProps) {
  if (attachments.length === 0) {
    return null;
  }

  return (
    <div className="flex gap-2 flex-wrap mb-2">
      {attachments.map((attachment) => (
        <button
          key={attachment.id}
          data-testid="attachment-chip"
          type="button"
          onClick={() => onClick?.(attachment.id, attachment.filePath)}
          className="flex items-center gap-1.5 px-2 py-1 rounded transition-all"
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
          {/* File icon */}
          <div
            className="shrink-0"
            style={{
              color: "var(--text-secondary)",
            }}
          >
            {getFileIcon(attachment.mimeType, attachment.fileName)}
          </div>

          {/* File name */}
          <span
            className="text-xs max-w-[180px]"
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

          {/* File size */}
          <span
            className="text-[0.625rem]"
            style={{
              color: "var(--text-muted)",
            }}
          >
            {formatFileSize(attachment.fileSize)}
          </span>
        </button>
      ))}
    </div>
  );
}
