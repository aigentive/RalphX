/**
 * ChatAttachmentPicker - Paperclip icon button that triggers file input
 *
 * Design spec: specs/design/pages/chat-panel.md
 * - macOS Tahoe flat styling (matches ChatInput button style)
 * - Orange accent color #ff6b35
 * - Compact size to fit in chat footer
 * - File type filtering and validation
 */

import { useRef, useCallback, useState } from "react";
import { Paperclip } from "lucide-react";

import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";

// ============================================================================
// Types
// ============================================================================

export interface ChatAttachmentPickerProps {
  /** Callback when files are selected */
  onFilesSelected?: (files: File[]) => void;
  /** Whether the picker is disabled */
  disabled?: boolean;
  /** Maximum number of files allowed (default 5) */
  maxFiles?: number;
  /** Maximum file size in bytes (default 10MB) */
  maxFileSize?: number;
  /** Subtle mode: icon-only, no orange fill — for use inside a unified input container */
  subtle?: boolean;
}

// ============================================================================
// Constants
// ============================================================================

const DEFAULT_MAX_FILES = 5;
const DEFAULT_MAX_FILE_SIZE = 10 * 1024 * 1024; // 10MB

const ACCEPTED_TYPES = [
  "text/*",
  "image/*",
  "application/pdf",
  "application/json",
  ".md",
  ".txt",
  ".js",
  ".ts",
  ".tsx",
  ".jsx",
  ".py",
  ".rs",
  ".go",
  ".java",
  ".cpp",
  ".c",
  ".h",
].join(",");

// ============================================================================
// Component
// ============================================================================

export function ChatAttachmentPicker({
  onFilesSelected,
  disabled = false,
  maxFiles = DEFAULT_MAX_FILES,
  maxFileSize = DEFAULT_MAX_FILE_SIZE,
  subtle = false,
}: ChatAttachmentPickerProps) {
  const fileInputRef = useRef<HTMLInputElement>(null);
  const [isDragging, setIsDragging] = useState(false);

  // Validate and filter selected files
  const validateFiles = useCallback(
    (fileList: FileList | null): File[] => {
      if (!fileList || fileList.length === 0) {
        return [];
      }

      const files = Array.from(fileList);

      // Filter out files that exceed size limit
      const validFiles = files.filter((file) => file.size <= maxFileSize);

      // Limit to maxFiles
      return validFiles.slice(0, maxFiles);
    },
    [maxFiles, maxFileSize]
  );

  // Handle file selection
  const handleFileChange = useCallback(
    (event: React.ChangeEvent<HTMLInputElement>) => {
      const validFiles = validateFiles(event.target.files);

      if (validFiles.length > 0 && onFilesSelected) {
        onFilesSelected(validFiles);
      }

      // Reset input so the same file can be selected again
      if (fileInputRef.current) {
        fileInputRef.current.value = "";
      }
    },
    [onFilesSelected, validateFiles]
  );

  // Trigger file input on button click
  const handleButtonClick = useCallback(() => {
    if (!disabled && fileInputRef.current) {
      fileInputRef.current.click();
    }
  }, [disabled]);

  // Drag and drop handlers
  const handleDragEnter = useCallback(
    (event: React.DragEvent<HTMLDivElement>) => {
      if (disabled) return;
      event.preventDefault();
      event.stopPropagation();
      setIsDragging(true);
    },
    [disabled]
  );

  const handleDragOver = useCallback(
    (event: React.DragEvent<HTMLDivElement>) => {
      if (disabled) return;
      event.preventDefault();
      event.stopPropagation();
    },
    [disabled]
  );

  const handleDragLeave = useCallback(
    (event: React.DragEvent<HTMLDivElement>) => {
      if (disabled) return;
      event.preventDefault();
      event.stopPropagation();

      // Only set isDragging to false if we're actually leaving the drop zone
      // Check if the relatedTarget is outside the drop zone
      const currentTarget = event.currentTarget;
      const relatedTarget = event.relatedTarget as Node | null;

      if (!relatedTarget || !currentTarget.contains(relatedTarget)) {
        setIsDragging(false);
      }
    },
    [disabled]
  );

  const handleDrop = useCallback(
    (event: React.DragEvent<HTMLDivElement>) => {
      if (disabled) return;
      event.preventDefault();
      event.stopPropagation();

      setIsDragging(false);

      // Extract files from dataTransfer
      const validFiles = validateFiles(event.dataTransfer.files);

      if (validFiles.length > 0 && onFilesSelected) {
        onFilesSelected(validFiles);
      }
    },
    [disabled, validateFiles, onFilesSelected]
  );

  return (
    <>
      {/* Hidden file input */}
      <input
        ref={fileInputRef}
        data-testid="attachment-file-input"
        type="file"
        multiple
        accept={ACCEPTED_TYPES}
        onChange={handleFileChange}
        className="hidden"
        aria-hidden="true"
        tabIndex={-1}
      />

      {/* Drop zone wrapper */}
      <div
        data-testid="attachment-drop-zone"
        onDragEnter={handleDragEnter}
        onDragOver={handleDragOver}
        onDragLeave={handleDragLeave}
        onDrop={handleDrop}
        className="relative"
      >
        {/* Paperclip button - macOS Tahoe flat styling */}
        <TooltipProvider delayDuration={250}>
          <Tooltip>
            <TooltipTrigger asChild>
              <button
                data-testid="attachment-picker-button"
                type="button"
                onClick={handleButtonClick}
                disabled={disabled}
                aria-label="Attach files"
                className={
                  subtle
                    ? "rounded transition-all disabled:opacity-40 shrink-0 w-[30px] h-[30px] flex items-center justify-center"
                    : "rounded-lg transition-colors disabled:opacity-40 shrink-0 w-[38px] h-[38px] flex items-center justify-center hover:brightness-110"
                }
                style={
                  subtle
                    ? { background: "transparent", color: "var(--text-muted)", boxShadow: "none" }
                    : {
                        /* Muted gray chrome matching the Send button's disabled
                           baseline so the attachment picker reads as a neutral
                           control, not a primary CTA. Per 2026-04-19 feedback. */
                        background: "color-mix(in srgb, var(--text-primary) 8%, transparent)",
                        border: "1px solid var(--border-subtle)",
                        color: "var(--text-muted)",
                        boxShadow: "none",
                      }
                }
                onMouseEnter={
                  subtle && !disabled
                    ? (e) => {
                        e.currentTarget.style.color = "var(--accent-primary)";
                      }
                    : undefined
                }
                onMouseLeave={
                  subtle
                    ? (e) => {
                        e.currentTarget.style.color = "var(--text-muted)";
                      }
                    : undefined
                }
              >
                <Paperclip size={subtle ? 15 : 16} />
              </button>
            </TooltipTrigger>
            <TooltipContent side="top" className="text-xs">
              Attach files
            </TooltipContent>
          </Tooltip>
        </TooltipProvider>

        {/* Drop overlay - shown when dragging files over zone */}
        {isDragging && (
          <div
            data-testid="attachment-drop-overlay"
            className="absolute inset-0 rounded-lg flex items-center justify-center pointer-events-none"
            style={{
              background: "color-mix(in srgb, var(--accent-primary) 10%, transparent)",
              border: "2px dashed var(--accent-primary)",
            }}
          >
            <span className="text-[0.8125rem] font-medium" style={{ color: "var(--accent-primary)" }}>
              Drop files here
            </span>
          </div>
        )}
      </div>
    </>
  );
}
