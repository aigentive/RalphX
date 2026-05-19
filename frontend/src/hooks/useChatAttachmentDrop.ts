import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type DragEvent,
  type RefObject,
} from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { readFile, stat } from "@tauri-apps/plugin-fs";

import {
  CHAT_ATTACHMENT_MAX_FILE_SIZE,
  CHAT_ATTACHMENT_MAX_FILES,
  getFileNameFromPath,
  inferMimeTypeFromFileName,
  validateChatAttachmentFiles,
} from "@/components/Chat/chatAttachmentFiles";

interface NativeDropPosition {
  x: number;
  y: number;
}

interface NativeDropPayload {
  type: string;
  paths?: string[];
  position?: NativeDropPosition;
}

interface NativeDropEvent {
  payload: NativeDropPayload;
}

interface UseChatAttachmentDropOptions {
  enabled: boolean;
  targetRef: RefObject<HTMLElement | null>;
  onFilesSelected?: ((files: File[]) => void | Promise<unknown>) | undefined;
  maxFiles?: number;
  maxFileSize?: number;
}

interface UseChatAttachmentDropResult {
  isDragging: boolean;
  dropProps: {
    onDragEnter: (event: DragEvent<HTMLElement>) => void;
    onDragOver: (event: DragEvent<HTMLElement>) => void;
    onDragLeave: (event: DragEvent<HTMLElement>) => void;
    onDrop: (event: DragEvent<HTMLElement>) => void;
  };
}

function dataTransferHasFiles(dataTransfer: DataTransfer): boolean {
  if (Array.from(dataTransfer.types ?? []).includes("Files")) {
    return true;
  }

  if (Array.from(dataTransfer.items ?? []).some((item) => item.kind === "file")) {
    return true;
  }

  return dataTransfer.files.length > 0;
}

function isPositionInsideElement(
  position: NativeDropPosition | undefined,
  element: HTMLElement | null,
): boolean {
  if (!position || !element) {
    return false;
  }

  const rect = element.getBoundingClientRect();
  return (
    position.x >= rect.left &&
    position.x <= rect.right &&
    position.y >= rect.top &&
    position.y <= rect.bottom
  );
}

async function readNativeDroppedFiles(
  paths: string[],
  maxFiles: number,
  maxFileSize: number,
): Promise<File[]> {
  const files: File[] = [];

  for (const filePath of paths.slice(0, maxFiles)) {
    try {
      const metadata = await stat(filePath);
      if (!metadata.isFile || metadata.size > maxFileSize) {
        continue;
      }

      const data = await readFile(filePath);
      if (data.byteLength > maxFileSize) {
        continue;
      }

      const fileName = getFileNameFromPath(filePath);
      const mimeType = inferMimeTypeFromFileName(fileName);
      files.push(
        new File([data], fileName, mimeType ? { type: mimeType } : undefined),
      );
    } catch (error) {
      console.error("Failed to read dropped chat attachment:", error);
    }
  }

  return files;
}

export function useChatAttachmentDrop({
  enabled,
  targetRef,
  onFilesSelected,
  maxFiles = CHAT_ATTACHMENT_MAX_FILES,
  maxFileSize = CHAT_ATTACHMENT_MAX_FILE_SIZE,
}: UseChatAttachmentDropOptions): UseChatAttachmentDropResult {
  const [isDragging, setIsDragging] = useState(false);
  const enabledRef = useRef(enabled);
  const onFilesSelectedRef = useRef(onFilesSelected);
  const maxFilesRef = useRef(maxFiles);
  const maxFileSizeRef = useRef(maxFileSize);
  const nativeDragInsideRef = useRef(false);

  useEffect(() => {
    enabledRef.current = enabled;
    onFilesSelectedRef.current = onFilesSelected;
    maxFilesRef.current = maxFiles;
    maxFileSizeRef.current = maxFileSize;
  }, [enabled, maxFiles, maxFileSize, onFilesSelected]);

  const selectFiles = useCallback((files: File[]) => {
    if (files.length === 0) {
      return;
    }

    void onFilesSelectedRef.current?.(files);
  }, []);

  useEffect(() => {
    if (!enabled) {
      setIsDragging(false);
      nativeDragInsideRef.current = false;
      return;
    }

    let unlisten: (() => void) | undefined;
    let cancelled = false;

    const setupListener = async () => {
      try {
        const webview = getCurrentWebview();
        unlisten = await webview.onDragDropEvent(async (event: NativeDropEvent) => {
          if (!enabledRef.current) {
            return;
          }

          const { payload } = event;
          if (payload.type === "over" || payload.type === "enter") {
            const isInside = isPositionInsideElement(payload.position, targetRef.current);
            nativeDragInsideRef.current = isInside;
            setIsDragging(isInside);
            return;
          }

          if (payload.type === "drop") {
            const isInside = payload.position
              ? isPositionInsideElement(payload.position, targetRef.current)
              : nativeDragInsideRef.current;
            nativeDragInsideRef.current = false;
            setIsDragging(false);

            if (!isInside || !payload.paths || payload.paths.length === 0) {
              return;
            }

            const files = await readNativeDroppedFiles(
              payload.paths,
              maxFilesRef.current,
              maxFileSizeRef.current,
            );
            selectFiles(files);
            return;
          }

          nativeDragInsideRef.current = false;
          setIsDragging(false);
        });

        if (cancelled) {
          unlisten();
        }
      } catch (error) {
        console.error("Failed to set up chat attachment drop listener:", error);
      }
    };

    void setupListener();

    return () => {
      cancelled = true;
      nativeDragInsideRef.current = false;
      unlisten?.();
    };
  }, [enabled, selectFiles, targetRef]);

  const onDragEnter = useCallback((event: DragEvent<HTMLElement>) => {
    if (!dataTransferHasFiles(event.dataTransfer)) {
      return;
    }

    event.preventDefault();
    event.stopPropagation();

    if (enabledRef.current) {
      setIsDragging(true);
    }
  }, []);

  const onDragOver = useCallback((event: DragEvent<HTMLElement>) => {
    if (!dataTransferHasFiles(event.dataTransfer)) {
      return;
    }

    event.preventDefault();
    event.stopPropagation();
    event.dataTransfer.dropEffect = "copy";
  }, []);

  const onDragLeave = useCallback((event: DragEvent<HTMLElement>) => {
    if (!dataTransferHasFiles(event.dataTransfer)) {
      return;
    }

    event.preventDefault();
    event.stopPropagation();

    const relatedTarget = event.relatedTarget as Node | null;
    if (!relatedTarget || !event.currentTarget.contains(relatedTarget)) {
      setIsDragging(false);
    }
  }, []);

  const onDrop = useCallback(
    (event: DragEvent<HTMLElement>) => {
      if (!dataTransferHasFiles(event.dataTransfer)) {
        return;
      }

      event.preventDefault();
      event.stopPropagation();
      setIsDragging(false);

      if (!enabledRef.current) {
        return;
      }

      const validFiles = validateChatAttachmentFiles(event.dataTransfer.files, {
        maxFiles: maxFilesRef.current,
        maxFileSize: maxFileSizeRef.current,
      });
      selectFiles(validFiles);
    },
    [selectFiles],
  );

  return {
    isDragging,
    dropProps: {
      onDragEnter,
      onDragOver,
      onDragLeave,
      onDrop,
    },
  };
}
