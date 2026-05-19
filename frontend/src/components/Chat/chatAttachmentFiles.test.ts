import { describe, expect, it } from "vitest";

import {
  CHAT_ATTACHMENT_MAX_FILE_SIZE,
  getFileNameFromPath,
  inferMimeTypeFromFileName,
  validateChatAttachmentFiles,
} from "./chatAttachmentFiles";

describe("chatAttachmentFiles", () => {
  describe("validateChatAttachmentFiles", () => {
    it("returns no files for empty input", () => {
      expect(validateChatAttachmentFiles(null)).toEqual([]);
      expect(validateChatAttachmentFiles([])).toEqual([]);
    });

    it("filters oversized files and applies max file count", () => {
      const smallA = new File(["a"], "a.txt", { type: "text/plain" });
      const smallB = new File(["b"], "b.txt", { type: "text/plain" });
      const smallC = new File(["c"], "c.txt", { type: "text/plain" });
      const large = new File(["large"], "large.txt", { type: "text/plain" });
      Object.defineProperty(large, "size", {
        value: CHAT_ATTACHMENT_MAX_FILE_SIZE + 1,
      });

      expect(
        validateChatAttachmentFiles([smallA, large, smallB, smallC], {
          maxFiles: 2,
        }),
      ).toEqual([smallA, smallB]);
    });
  });

  describe("getFileNameFromPath", () => {
    it("extracts file names from POSIX and Windows paths", () => {
      expect(getFileNameFromPath("/Users/dev/Desktop/note.md")).toBe("note.md");
      expect(getFileNameFromPath("C:\\Users\\dev\\Desktop\\note.md")).toBe("note.md");
    });

    it("falls back when the path has no filename segment", () => {
      expect(getFileNameFromPath("/")).toBe("attachment");
      expect(getFileNameFromPath("")).toBe("attachment");
    });
  });

  describe("inferMimeTypeFromFileName", () => {
    it("infers supported mime types by extension", () => {
      expect(inferMimeTypeFromFileName("note.txt")).toBe("text/plain");
      expect(inferMimeTypeFromFileName("plan.md")).toBe("text/markdown");
      expect(inferMimeTypeFromFileName("plan.markdown")).toBe("text/markdown");
      expect(inferMimeTypeFromFileName("data.json")).toBe("application/json");
      expect(inferMimeTypeFromFileName("doc.pdf")).toBe("application/pdf");
      expect(inferMimeTypeFromFileName("image.png")).toBe("image/png");
      expect(inferMimeTypeFromFileName("photo.jpg")).toBe("image/jpeg");
      expect(inferMimeTypeFromFileName("photo.jpeg")).toBe("image/jpeg");
      expect(inferMimeTypeFromFileName("animation.gif")).toBe("image/gif");
      expect(inferMimeTypeFromFileName("screen.webp")).toBe("image/webp");
      expect(inferMimeTypeFromFileName("icon.svg")).toBe("image/svg+xml");
    });

    it("treats source files as text and unknown files as unspecified", () => {
      for (const extension of ["js", "jsx", "ts", "tsx", "py", "rs", "go", "java", "cpp", "c", "h"]) {
        expect(inferMimeTypeFromFileName(`source.${extension}`)).toBe("text/plain");
      }
      expect(inferMimeTypeFromFileName("archive.zip")).toBe("");
      expect(inferMimeTypeFromFileName("README")).toBe("");
    });
  });
});
