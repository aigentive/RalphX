/**
 * ChatAttachmentPicker Component Tests
 *
 * Tests for the attachment picker component with:
 * - Renders button with paperclip icon
 * - Opens file input on click
 * - Filters file types
 * - Validates file size
 * - Calls onFilesSelected with valid files
 * - Disables when disabled prop is true
 */

import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { ChatAttachmentPicker } from "./ChatAttachmentPicker";

describe("ChatAttachmentPicker", () => {
  const defaultProps = {
    onFilesSelected: vi.fn(),
  };

  beforeEach(() => {
    vi.clearAllMocks();
  });

  // ============================================================================
  // Rendering Tests
  // ============================================================================

  describe("rendering", () => {
    it("renders the attachment button", () => {
      render(<ChatAttachmentPicker {...defaultProps} />);
      expect(screen.getByTestId("attachment-picker-button")).toBeInTheDocument();
    });

    it("renders with paperclip icon", () => {
      render(<ChatAttachmentPicker {...defaultProps} />);
      const button = screen.getByTestId("attachment-picker-button");
      // Lucide-react renders SVG icons
      expect(button.querySelector("svg")).toBeInTheDocument();
    });

    it("renders hidden file input", () => {
      render(<ChatAttachmentPicker {...defaultProps} />);
      const fileInput = screen.getByTestId("attachment-file-input");
      expect(fileInput).toBeInTheDocument();
      expect(fileInput).toHaveClass("hidden");
    });

    it("has accessible label", () => {
      render(<ChatAttachmentPicker {...defaultProps} />);
      expect(screen.getByLabelText("Attach files")).toBeInTheDocument();
    });
  });

  // ============================================================================
  // File Input Trigger Tests
  // ============================================================================

  describe("file input trigger", () => {
    it("opens file input when button is clicked", async () => {
      const user = userEvent.setup();
      render(<ChatAttachmentPicker {...defaultProps} />);

      const fileInput = screen.getByTestId("attachment-file-input") as HTMLInputElement;
      const clickSpy = vi.spyOn(fileInput, "click");

      await user.click(screen.getByTestId("attachment-picker-button"));

      expect(clickSpy).toHaveBeenCalled();
    });

    it("does not open file input when disabled", async () => {
      const user = userEvent.setup();
      render(<ChatAttachmentPicker {...defaultProps} disabled={true} />);

      const fileInput = screen.getByTestId("attachment-file-input") as HTMLInputElement;
      const clickSpy = vi.spyOn(fileInput, "click");

      await user.click(screen.getByTestId("attachment-picker-button"));

      expect(clickSpy).not.toHaveBeenCalled();
    });
  });

  // ============================================================================
  // File Type Filtering Tests
  // ============================================================================

  describe("file type filtering", () => {
    it("accepts text files", () => {
      render(<ChatAttachmentPicker {...defaultProps} />);
      const fileInput = screen.getByTestId("attachment-file-input") as HTMLInputElement;
      const acceptAttr = fileInput.accept;

      expect(acceptAttr).toContain("text/*");
      expect(acceptAttr).toContain(".txt");
      expect(acceptAttr).toContain(".md");
    });

    it("accepts image files", () => {
      render(<ChatAttachmentPicker {...defaultProps} />);
      const fileInput = screen.getByTestId("attachment-file-input") as HTMLInputElement;

      expect(fileInput.accept).toContain("image/*");
    });

    it("accepts PDF files", () => {
      render(<ChatAttachmentPicker {...defaultProps} />);
      const fileInput = screen.getByTestId("attachment-file-input") as HTMLInputElement;

      expect(fileInput.accept).toContain("application/pdf");
    });

    it("accepts code files", () => {
      render(<ChatAttachmentPicker {...defaultProps} />);
      const fileInput = screen.getByTestId("attachment-file-input") as HTMLInputElement;
      const acceptAttr = fileInput.accept;

      expect(acceptAttr).toContain(".js");
      expect(acceptAttr).toContain(".ts");
      expect(acceptAttr).toContain(".tsx");
      expect(acceptAttr).toContain(".py");
      expect(acceptAttr).toContain(".rs");
    });

    it("allows multiple file selection", () => {
      render(<ChatAttachmentPicker {...defaultProps} />);
      const fileInput = screen.getByTestId("attachment-file-input") as HTMLInputElement;

      expect(fileInput).toHaveAttribute("multiple");
    });
  });

  // ============================================================================
  // File Size Validation Tests
  // ============================================================================

  describe("file size validation", () => {
    it("accepts files within size limit", async () => {
      render(<ChatAttachmentPicker {...defaultProps} />);

      const fileInput = screen.getByTestId("attachment-file-input") as HTMLInputElement;
      const validFile = new File(["test content"], "test.txt", {
        type: "text/plain",
      });

      // Default max size is 10MB, this file is tiny
      fireEvent.change(fileInput, { target: { files: [validFile] } });

      await waitFor(() => {
        expect(defaultProps.onFilesSelected).toHaveBeenCalledWith([validFile]);
      });
    });

    it("rejects files exceeding size limit", async () => {
      const maxFileSize = 1024; // 1KB
      render(
        <ChatAttachmentPicker {...defaultProps} maxFileSize={maxFileSize} />
      );

      const fileInput = screen.getByTestId("attachment-file-input") as HTMLInputElement;

      // Create a file larger than 1KB
      const largeContent = "x".repeat(2048); // 2KB
      const largeFile = new File([largeContent], "large.txt", {
        type: "text/plain",
      });

      // Mock file size
      Object.defineProperty(largeFile, "size", { value: 2048 });

      fireEvent.change(fileInput, { target: { files: [largeFile] } });

      // Should not call onFilesSelected because file is too large
      await waitFor(() => {
        expect(defaultProps.onFilesSelected).not.toHaveBeenCalled();
      });
    });

    it("filters out oversized files but keeps valid ones", async () => {
      const maxFileSize = 1024; // 1KB
      render(
        <ChatAttachmentPicker {...defaultProps} maxFileSize={maxFileSize} />
      );

      const fileInput = screen.getByTestId("attachment-file-input") as HTMLInputElement;

      const validFile = new File(["small"], "small.txt", {
        type: "text/plain",
      });
      const largeFile = new File(["x".repeat(2048)], "large.txt", {
        type: "text/plain",
      });

      Object.defineProperty(validFile, "size", { value: 100 });
      Object.defineProperty(largeFile, "size", { value: 2048 });

      fireEvent.change(fileInput, { target: { files: [validFile, largeFile] } });

      await waitFor(() => {
        // Only the valid file should be passed
        expect(defaultProps.onFilesSelected).toHaveBeenCalledWith([validFile]);
      });
    });
  });

  // ============================================================================
  // Max Files Limit Tests
  // ============================================================================

  describe("max files limit", () => {
    it("respects default max files limit (5)", async () => {
      render(<ChatAttachmentPicker {...defaultProps} />);

      const fileInput = screen.getByTestId("attachment-file-input") as HTMLInputElement;
      const files = Array.from({ length: 7 }, (_, i) =>
        new File([`content ${i}`], `file${i}.txt`, { type: "text/plain" })
      );

      fireEvent.change(fileInput, { target: { files } });

      await waitFor(() => {
        // Should only include first 5 files
        expect(defaultProps.onFilesSelected).toHaveBeenCalledWith(
          files.slice(0, 5)
        );
      });
    });

    it("respects custom max files limit", async () => {
      const maxFiles = 2;
      render(<ChatAttachmentPicker {...defaultProps} maxFiles={maxFiles} />);

      const fileInput = screen.getByTestId("attachment-file-input") as HTMLInputElement;
      const files = Array.from({ length: 5 }, (_, i) =>
        new File([`content ${i}`], `file${i}.txt`, { type: "text/plain" })
      );

      fireEvent.change(fileInput, { target: { files } });

      await waitFor(() => {
        // Should only include first 2 files
        expect(defaultProps.onFilesSelected).toHaveBeenCalledWith(
          files.slice(0, 2)
        );
      });
    });

    it("accepts all files when count is within limit", async () => {
      render(<ChatAttachmentPicker {...defaultProps} />);

      const fileInput = screen.getByTestId("attachment-file-input") as HTMLInputElement;
      const files = Array.from({ length: 3 }, (_, i) =>
        new File([`content ${i}`], `file${i}.txt`, { type: "text/plain" })
      );

      fireEvent.change(fileInput, { target: { files } });

      await waitFor(() => {
        expect(defaultProps.onFilesSelected).toHaveBeenCalledWith(files);
      });
    });
  });

  // ============================================================================
  // Disabled State Tests
  // ============================================================================

  describe("disabled state", () => {
    it("disables button when disabled prop is true", () => {
      render(<ChatAttachmentPicker {...defaultProps} disabled={true} />);
      expect(screen.getByTestId("attachment-picker-button")).toBeDisabled();
    });

    it("enables button when disabled prop is false", () => {
      render(<ChatAttachmentPicker {...defaultProps} disabled={false} />);
      expect(screen.getByTestId("attachment-picker-button")).not.toBeDisabled();
    });

    it("applies disabled styling when disabled", () => {
      render(<ChatAttachmentPicker {...defaultProps} disabled={true} />);
      const button = screen.getByTestId("attachment-picker-button");

      // Neutral muted chrome — disabled attribute + shared CSS opacity-40 class
      expect(button).toBeDisabled();
      expect(button.className).toContain("disabled:opacity-40");
    });

    it("applies enabled styling when not disabled", () => {
      render(<ChatAttachmentPicker {...defaultProps} disabled={false} />);
      const button = screen.getByTestId("attachment-picker-button");

      // Muted gray chrome (matches Send button's disabled baseline)
      expect(button).toHaveStyle({
        background: "color-mix(in srgb, var(--text-primary) 8%, transparent)",
      });
    });
  });

  // ============================================================================
  // Callback Tests
  // ============================================================================

  describe("callback behavior", () => {
    it("calls onFilesSelected with valid files", async () => {
      render(<ChatAttachmentPicker {...defaultProps} />);

      const fileInput = screen.getByTestId("attachment-file-input") as HTMLInputElement;
      const file1 = new File(["content 1"], "file1.txt", { type: "text/plain" });
      const file2 = new File(["content 2"], "file2.md", { type: "text/markdown" });

      fireEvent.change(fileInput, { target: { files: [file1, file2] } });

      await waitFor(() => {
        expect(defaultProps.onFilesSelected).toHaveBeenCalledWith([file1, file2]);
      });
    });

    it("does not call onFilesSelected when no files selected", async () => {
      render(<ChatAttachmentPicker {...defaultProps} />);

      const fileInput = screen.getByTestId("attachment-file-input") as HTMLInputElement;

      fireEvent.change(fileInput, { target: { files: [] } });

      await waitFor(() => {
        expect(defaultProps.onFilesSelected).not.toHaveBeenCalled();
      });
    });

    it("does not call onFilesSelected when all files are invalid", async () => {
      const maxFileSize = 100; // 100 bytes
      render(
        <ChatAttachmentPicker {...defaultProps} maxFileSize={maxFileSize} />
      );

      const fileInput = screen.getByTestId("attachment-file-input") as HTMLInputElement;
      const largeFile = new File(["x".repeat(200)], "large.txt", {
        type: "text/plain",
      });
      Object.defineProperty(largeFile, "size", { value: 200 });

      fireEvent.change(fileInput, { target: { files: [largeFile] } });

      await waitFor(() => {
        expect(defaultProps.onFilesSelected).not.toHaveBeenCalled();
      });
    });

    it("resets file input after selection", async () => {
      render(<ChatAttachmentPicker {...defaultProps} />);

      const fileInput = screen.getByTestId("attachment-file-input") as HTMLInputElement;
      const file = new File(["content"], "file.txt", { type: "text/plain" });

      fireEvent.change(fileInput, { target: { files: [file] } });

      await waitFor(() => {
        expect(fileInput.value).toBe("");
      });
    });
  });

  // ============================================================================
  // Styling Tests
  // ============================================================================

  describe("styling", () => {
    it("applies muted neutral chrome to enabled button", () => {
      render(<ChatAttachmentPicker {...defaultProps} />);
      const button = screen.getByTestId("attachment-picker-button");

      // Neutral control — not a primary CTA (per 2026-04-19 UX pass)
      expect(button).toHaveStyle({
        background: "color-mix(in srgb, var(--text-primary) 8%, transparent)",
      });
      expect(button).toHaveStyle({ color: "var(--text-muted)" });
    });

    it("has compact size to fit in chat footer", () => {
      render(<ChatAttachmentPicker {...defaultProps} />);
      const button = screen.getByTestId("attachment-picker-button");

      // Should have fixed dimensions matching ChatInput send button
      expect(button).toHaveClass("w-[38px]");
      expect(button).toHaveClass("h-[38px]");
    });

    it("has rounded corners", () => {
      render(<ChatAttachmentPicker {...defaultProps} />);
      const button = screen.getByTestId("attachment-picker-button");

      expect(button).toHaveClass("rounded-lg");
    });

    it("has flat styling (no box shadow)", () => {
      render(<ChatAttachmentPicker {...defaultProps} />);
      const button = screen.getByTestId("attachment-picker-button");

      expect(button).toHaveStyle({ boxShadow: "none" });
    });
  });

  // ============================================================================
  // Accessibility Tests
  // ============================================================================

  describe("accessibility", () => {
    it("button has proper aria-label", () => {
      render(<ChatAttachmentPicker {...defaultProps} />);
      expect(screen.getByLabelText("Attach files")).toBeInTheDocument();
    });

    it("file input is hidden from screen readers", () => {
      render(<ChatAttachmentPicker {...defaultProps} />);
      const fileInput = screen.getByTestId("attachment-file-input");

      expect(fileInput).toHaveAttribute("aria-hidden", "true");
      expect(fileInput).toHaveAttribute("tabIndex", "-1");
    });
  });

  // ============================================================================
  // Drag and Drop Tests
  // ============================================================================

  describe("drag and drop", () => {
    // Helper to create mock drag event with files
    const createDragEvent = (files: File[]) => {
      const dataTransfer = {
        files,
        items: files.map((file) => ({
          kind: "file" as const,
          type: file.type,
          getAsFile: () => file,
        })),
        types: ["Files"],
        getData: () => "",
        setData: () => {},
        clearData: () => {},
        setDragImage: () => {},
        effectAllowed: "all" as DataTransfer["effectAllowed"],
        dropEffect: "none" as DataTransfer["dropEffect"],
      };

      return { dataTransfer } as unknown as React.DragEvent<HTMLDivElement>;
    };

    it("renders drop zone wrapper", () => {
      render(<ChatAttachmentPicker {...defaultProps} />);
      expect(screen.getByTestId("attachment-drop-zone")).toBeInTheDocument();
    });

    it("does not show overlay initially", () => {
      render(<ChatAttachmentPicker {...defaultProps} />);
      expect(screen.queryByTestId("attachment-drop-overlay")).not.toBeInTheDocument();
    });

    it("shows overlay on drag enter", () => {
      render(<ChatAttachmentPicker {...defaultProps} />);

      const dropZone = screen.getByTestId("attachment-drop-zone");
      const file = new File(["content"], "test.txt", { type: "text/plain" });
      const dragEvent = createDragEvent([file]);

      fireEvent.dragEnter(dropZone, dragEvent);

      expect(screen.getByTestId("attachment-drop-overlay")).toBeInTheDocument();
    });

    it("hides overlay on drag leave", () => {
      render(<ChatAttachmentPicker {...defaultProps} />);

      const dropZone = screen.getByTestId("attachment-drop-zone");
      const file = new File(["content"], "test.txt", { type: "text/plain" });
      const dragEvent = createDragEvent([file]);

      fireEvent.dragEnter(dropZone, dragEvent);
      expect(screen.getByTestId("attachment-drop-overlay")).toBeInTheDocument();

      // Create drag leave event with relatedTarget outside drop zone
      const dragLeaveEvent = {
        ...dragEvent,
        relatedTarget: null,
      };

      fireEvent.dragLeave(dropZone, dragLeaveEvent);

      expect(screen.queryByTestId("attachment-drop-overlay")).not.toBeInTheDocument();
    });

    it("handles drag enter event", () => {
      render(<ChatAttachmentPicker {...defaultProps} />);

      const dropZone = screen.getByTestId("attachment-drop-zone");
      const file = new File(["content"], "test.txt", { type: "text/plain" });
      const dragEvent = createDragEvent([file]);

      fireEvent.dragEnter(dropZone, dragEvent);

      // Verify that drag enter is handled by checking overlay appears
      expect(screen.getByTestId("attachment-drop-overlay")).toBeInTheDocument();
    });

    it("handles drag over event without errors", () => {
      render(<ChatAttachmentPicker {...defaultProps} />);

      const dropZone = screen.getByTestId("attachment-drop-zone");
      const file = new File(["content"], "test.txt", { type: "text/plain" });
      const dragEvent = createDragEvent([file]);

      // First dragEnter to show overlay
      fireEvent.dragEnter(dropZone, dragEvent);

      // Then dragOver should not cause errors and overlay should remain
      fireEvent.dragOver(dropZone, dragEvent);

      expect(screen.getByTestId("attachment-drop-overlay")).toBeInTheDocument();
    });

    it("handles drop event and calls onFilesSelected", async () => {
      render(<ChatAttachmentPicker {...defaultProps} />);

      const dropZone = screen.getByTestId("attachment-drop-zone");
      const file1 = new File(["content 1"], "file1.txt", { type: "text/plain" });
      const file2 = new File(["content 2"], "file2.md", { type: "text/markdown" });
      const dropEvent = createDragEvent([file1, file2]);

      fireEvent.drop(dropZone, dropEvent);

      await waitFor(() => {
        expect(defaultProps.onFilesSelected).toHaveBeenCalledWith([file1, file2]);
      });
    });

    it("hides overlay after drop", async () => {
      render(<ChatAttachmentPicker {...defaultProps} />);

      const dropZone = screen.getByTestId("attachment-drop-zone");
      const file = new File(["content"], "test.txt", { type: "text/plain" });
      const dragEvent = createDragEvent([file]);

      // Show overlay
      fireEvent.dragEnter(dropZone, dragEvent);
      expect(screen.getByTestId("attachment-drop-overlay")).toBeInTheDocument();

      // Drop
      fireEvent.drop(dropZone, dragEvent);

      await waitFor(() => {
        expect(screen.queryByTestId("attachment-drop-overlay")).not.toBeInTheDocument();
      });
    });

    it("validates dropped files (size limit)", async () => {
      const maxFileSize = 1024; // 1KB
      render(
        <ChatAttachmentPicker {...defaultProps} maxFileSize={maxFileSize} />
      );

      const dropZone = screen.getByTestId("attachment-drop-zone");
      const validFile = new File(["small"], "small.txt", { type: "text/plain" });
      const largeFile = new File(["x".repeat(2048)], "large.txt", {
        type: "text/plain",
      });

      Object.defineProperty(validFile, "size", { value: 100 });
      Object.defineProperty(largeFile, "size", { value: 2048 });

      const dropEvent = createDragEvent([validFile, largeFile]);

      fireEvent.drop(dropZone, dropEvent);

      await waitFor(() => {
        // Only valid file should be passed
        expect(defaultProps.onFilesSelected).toHaveBeenCalledWith([validFile]);
      });
    });

    it("validates dropped files (max count)", async () => {
      const maxFiles = 2;
      render(<ChatAttachmentPicker {...defaultProps} maxFiles={maxFiles} />);

      const dropZone = screen.getByTestId("attachment-drop-zone");
      const files = Array.from({ length: 5 }, (_, i) =>
        new File([`content ${i}`], `file${i}.txt`, { type: "text/plain" })
      );
      const dropEvent = createDragEvent(files);

      fireEvent.drop(dropZone, dropEvent);

      await waitFor(() => {
        // Only first 2 files should be passed
        expect(defaultProps.onFilesSelected).toHaveBeenCalledWith(
          files.slice(0, 2)
        );
      });
    });

    it("does not handle drag events when disabled", () => {
      render(<ChatAttachmentPicker {...defaultProps} disabled={true} />);

      const dropZone = screen.getByTestId("attachment-drop-zone");
      const file = new File(["content"], "test.txt", { type: "text/plain" });
      const dragEvent = createDragEvent([file]);

      fireEvent.dragEnter(dropZone, dragEvent);

      // Overlay should not be shown
      expect(screen.queryByTestId("attachment-drop-overlay")).not.toBeInTheDocument();
    });

    it("does not call onFilesSelected on drop when disabled", async () => {
      render(<ChatAttachmentPicker {...defaultProps} disabled={true} />);

      const dropZone = screen.getByTestId("attachment-drop-zone");
      const file = new File(["content"], "test.txt", { type: "text/plain" });
      const dropEvent = createDragEvent([file]);

      fireEvent.drop(dropZone, dropEvent);

      await waitFor(() => {
        expect(defaultProps.onFilesSelected).not.toHaveBeenCalled();
      });
    });

    it("does not call onFilesSelected when no valid files dropped", async () => {
      const maxFileSize = 100; // 100 bytes
      render(
        <ChatAttachmentPicker {...defaultProps} maxFileSize={maxFileSize} />
      );

      const dropZone = screen.getByTestId("attachment-drop-zone");
      const largeFile = new File(["x".repeat(200)], "large.txt", {
        type: "text/plain",
      });
      Object.defineProperty(largeFile, "size", { value: 200 });

      const dropEvent = createDragEvent([largeFile]);

      fireEvent.drop(dropZone, dropEvent);

      await waitFor(() => {
        expect(defaultProps.onFilesSelected).not.toHaveBeenCalled();
      });
    });

    it("overlay has correct styling", () => {
      render(<ChatAttachmentPicker {...defaultProps} />);

      const dropZone = screen.getByTestId("attachment-drop-zone");
      const file = new File(["content"], "test.txt", { type: "text/plain" });
      const dragEvent = createDragEvent([file]);

      fireEvent.dragEnter(dropZone, dragEvent);

      const overlay = screen.getByTestId("attachment-drop-overlay");

      // Check background (design token — color-mix preserved by jsdom)
      expect(overlay).toHaveStyle({
        background: "color-mix(in srgb, var(--accent-primary) 10%, transparent)",
      });

      // Check that it has a dashed border (jsdom can't resolve CSS var in shorthand,
      // so read inline style directly)
      expect(overlay.style.border).toContain("dashed");

      // Check text content
      expect(overlay).toHaveTextContent("Drop files here");
    });

    it("overlay has correct text styling", () => {
      render(<ChatAttachmentPicker {...defaultProps} />);

      const dropZone = screen.getByTestId("attachment-drop-zone");
      const file = new File(["content"], "test.txt", { type: "text/plain" });
      const dragEvent = createDragEvent([file]);

      fireEvent.dragEnter(dropZone, dragEvent);

      const overlay = screen.getByTestId("attachment-drop-overlay");
      const text = overlay.querySelector("span");

      expect(text).toHaveClass("text-[0.8125rem]");
      expect(text).toHaveClass("font-medium");
      expect(text).toHaveStyle({ color: "var(--accent-primary)" });
    });

    it("does not interfere with click-to-upload", async () => {
      const user = userEvent.setup();
      render(<ChatAttachmentPicker {...defaultProps} />);

      const fileInput = screen.getByTestId("attachment-file-input") as HTMLInputElement;
      const clickSpy = vi.spyOn(fileInput, "click");

      // Simulate drag events first
      const dropZone = screen.getByTestId("attachment-drop-zone");
      const file = new File(["content"], "test.txt", { type: "text/plain" });
      const dragEvent = createDragEvent([file]);

      fireEvent.dragEnter(dropZone, dragEvent);
      fireEvent.drop(dropZone, dragEvent);

      // Click should still work
      await user.click(screen.getByTestId("attachment-picker-button"));

      expect(clickSpy).toHaveBeenCalled();
    });
  });
});
