export const CHAT_ATTACHMENT_MAX_FILES = 5;
export const CHAT_ATTACHMENT_MAX_FILE_SIZE = 10 * 1024 * 1024;

export const CHAT_ATTACHMENT_ACCEPTED_TYPES = [
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

interface ValidateChatAttachmentFilesOptions {
  maxFiles?: number;
  maxFileSize?: number;
}

export function validateChatAttachmentFiles(
  fileList: FileList | File[] | null,
  {
    maxFiles = CHAT_ATTACHMENT_MAX_FILES,
    maxFileSize = CHAT_ATTACHMENT_MAX_FILE_SIZE,
  }: ValidateChatAttachmentFilesOptions = {},
): File[] {
  if (!fileList || fileList.length === 0) {
    return [];
  }

  return Array.from(fileList)
    .filter((file) => file.size <= maxFileSize)
    .slice(0, maxFiles);
}

export function getFileNameFromPath(filePath: string): string {
  return filePath.split(/[\\/]/).pop() || "attachment";
}

export function inferMimeTypeFromFileName(fileName: string): string {
  const extension = fileName.split(".").pop()?.toLowerCase() ?? "";

  switch (extension) {
    case "txt":
      return "text/plain";
    case "md":
    case "markdown":
      return "text/markdown";
    case "json":
      return "application/json";
    case "pdf":
      return "application/pdf";
    case "png":
      return "image/png";
    case "jpg":
    case "jpeg":
      return "image/jpeg";
    case "gif":
      return "image/gif";
    case "webp":
      return "image/webp";
    case "svg":
      return "image/svg+xml";
    case "js":
    case "jsx":
    case "ts":
    case "tsx":
    case "py":
    case "rs":
    case "go":
    case "java":
    case "cpp":
    case "c":
    case "h":
      return "text/plain";
    default:
      return "";
  }
}
