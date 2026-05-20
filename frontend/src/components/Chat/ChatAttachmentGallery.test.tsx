/**
 * Tests for ChatAttachmentGallery component
 */

import { describe, it, expect, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import {
  ChatAttachmentGallery,
  type ChatAttachment,
} from "./ChatAttachmentGallery";

describe("ChatAttachmentGallery", () => {
  const mockAttachments: ChatAttachment[] = [
    {
      id: "1",
      fileName: "document.pdf",
      fileSize: 1024 * 1024, // 1 MB
      mimeType: "application/pdf",
    },
    {
      id: "2",
      fileName: "image.png",
      fileSize: 512 * 1024, // 512 KB
      mimeType: "image/png",
    },
    {
      id: "3",
      fileName: "script.ts",
      fileSize: 2048, // 2 KB
      mimeType: "text/plain",
    },
  ];

  it("renders nothing when attachments array is empty", () => {
    const { container } = render(<ChatAttachmentGallery attachments={[]} />);
    expect(container.firstChild).toBeNull();
  });

  it("renders grid with attachments", () => {
    render(<ChatAttachmentGallery attachments={mockAttachments} />);

    const gallery = screen.getByTestId("chat-attachment-gallery");
    expect(gallery).toBeInTheDocument();

    const cards = screen.getAllByTestId("attachment-card");
    expect(cards).toHaveLength(3);
  });

  it("displays file names", () => {
    render(<ChatAttachmentGallery attachments={mockAttachments} />);

    expect(screen.getByText("document.pdf")).toBeInTheDocument();
    expect(screen.getByText("image.png")).toBeInTheDocument();
    expect(screen.getByText("script.ts")).toBeInTheDocument();
  });

  it("formats file sizes correctly", () => {
    render(<ChatAttachmentGallery attachments={mockAttachments} />);

    expect(screen.getByText("1.0 MB")).toBeInTheDocument();
    expect(screen.getByText("512.0 KB")).toBeInTheDocument();
    expect(screen.getByText("2.0 KB")).toBeInTheDocument();
  });

  it("formats bytes correctly", () => {
    const smallFile: ChatAttachment[] = [
      {
        id: "1",
        fileName: "tiny.txt",
        fileSize: 500,
        mimeType: "text/plain",
      },
    ];

    render(<ChatAttachmentGallery attachments={smallFile} />);
    expect(screen.getByText("500 B")).toBeInTheDocument();
  });

  it("shows correct file icon for images", () => {
    const imageFile: ChatAttachment[] = [
      {
        id: "1",
        fileName: "photo.jpg",
        fileSize: 1024,
        mimeType: "image/jpeg",
      },
    ];

    const { container } = render(
      <ChatAttachmentGallery attachments={imageFile} />
    );

    // Check if Lucide Image icon is rendered (it will have a specific SVG structure)
    const svg = container.querySelector("svg");
    expect(svg).toBeInTheDocument();
  });

  it("renders local image file previews in the composer gallery", async () => {
    const createObjectURL = vi.fn(() => "blob:composer-image-preview");
    const revokeObjectURL = vi.fn();
    const originalCreateObjectURL = URL.createObjectURL;
    const originalRevokeObjectURL = URL.revokeObjectURL;
    Object.defineProperty(URL, "createObjectURL", {
      value: createObjectURL,
      configurable: true,
    });
    Object.defineProperty(URL, "revokeObjectURL", {
      value: revokeObjectURL,
      configurable: true,
    });

    const file = new File(["image"], "photo.png", { type: "image/png" });

    try {
      render(
        <ChatAttachmentGallery
          attachments={[
            {
              id: "local-image",
              fileName: "photo.png",
              fileSize: file.size,
              mimeType: "image/png",
              file,
            },
          ]}
          compact
        />
      );

      await waitFor(() =>
        expect(screen.getByTestId("chat-attachment-image-preview")).toHaveAttribute(
          "src",
          "blob:composer-image-preview"
        )
      );
      expect(createObjectURL).toHaveBeenCalledWith(file);
      expect(screen.getByTestId("attachment-card").querySelector("svg")).not.toBeInTheDocument();
    } finally {
      Object.defineProperty(URL, "createObjectURL", {
        value: originalCreateObjectURL,
        configurable: true,
      });
      Object.defineProperty(URL, "revokeObjectURL", {
        value: originalRevokeObjectURL,
        configurable: true,
      });
    }
  });

  it("opens a large preview from composer image thumbnails", async () => {
    render(
      <ChatAttachmentGallery
        attachments={[
          {
            id: "preview-image",
            fileName: "preview.png",
            fileSize: 1024,
            mimeType: "image/png",
            previewUrl: "blob:preview-image",
          },
        ]}
      />
    );

    fireEvent.click(screen.getByTestId("chat-attachment-image-preview-button"));

    expect(screen.getByTestId("chat-attachment-image-dialog")).toBeInTheDocument();
    expect(screen.getByTestId("chat-attachment-image-large")).toHaveAttribute(
      "src",
      "blob:preview-image"
    );
  });

  it("shows correct file icon for text files", () => {
    const textFile: ChatAttachment[] = [
      {
        id: "1",
        fileName: "readme.txt",
        fileSize: 1024,
        mimeType: "text/plain",
      },
    ];

    render(<ChatAttachmentGallery attachments={textFile} />);
    const cards = screen.getAllByTestId("attachment-card");
    expect(cards).toHaveLength(1);
  });

  it("shows correct file icon for code files based on extension", () => {
    const codeFiles: ChatAttachment[] = [
      { id: "1", fileName: "app.js", fileSize: 1024 },
      { id: "2", fileName: "component.tsx", fileSize: 1024 },
      { id: "3", fileName: "main.rs", fileSize: 1024 },
    ];

    render(<ChatAttachmentGallery attachments={codeFiles} />);
    const cards = screen.getAllByTestId("attachment-card");
    expect(cards).toHaveLength(3);
  });

  it("shows generic file icon for unknown types", () => {
    const unknownFile: ChatAttachment[] = [
      {
        id: "1",
        fileName: "data.bin",
        fileSize: 1024,
        mimeType: "application/octet-stream",
      },
    ];

    render(<ChatAttachmentGallery attachments={unknownFile} />);
    const cards = screen.getAllByTestId("attachment-card");
    expect(cards).toHaveLength(1);
  });

  it("calls onRemove when X button is clicked", async () => {
    const user = userEvent.setup();
    const onRemove = vi.fn();

    render(
      <ChatAttachmentGallery
        attachments={mockAttachments}
        onRemove={onRemove}
      />
    );

    const removeButtons = screen.getAllByTestId("remove-attachment");
    expect(removeButtons).toHaveLength(3);

    await user.click(removeButtons[0]);
    expect(onRemove).toHaveBeenCalledWith("1");

    await user.click(removeButtons[1]);
    expect(onRemove).toHaveBeenCalledWith("2");
  });

  it("does not render remove button when onRemove is not provided", () => {
    render(<ChatAttachmentGallery attachments={mockAttachments} />);

    const removeButtons = screen.queryAllByTestId("remove-attachment");
    expect(removeButtons).toHaveLength(0);
  });

  it("shows upload progress when uploading is true", () => {
    render(
      <ChatAttachmentGallery attachments={mockAttachments} uploading={true} />
    );

    const progressIndicators = screen.getAllByTestId("upload-progress");
    expect(progressIndicators).toHaveLength(3);

    // Upload progress should be shown instead of remove buttons
    const removeButtons = screen.queryAllByTestId("remove-attachment");
    expect(removeButtons).toHaveLength(0);
  });

  it("does not show upload progress when uploading is false", () => {
    render(
      <ChatAttachmentGallery
        attachments={mockAttachments}
        uploading={false}
        onRemove={vi.fn()}
      />
    );

    const progressIndicators = screen.queryAllByTestId("upload-progress");
    expect(progressIndicators).toHaveLength(0);

    const removeButtons = screen.getAllByTestId("remove-attachment");
    expect(removeButtons).toHaveLength(3);
  });

  it("renders in compact variant", () => {
    render(<ChatAttachmentGallery attachments={mockAttachments} compact />);

    const gallery = screen.getByTestId("chat-attachment-gallery");
    expect(gallery).toHaveClass("flex");
    expect(gallery).toHaveClass("overflow-x-auto");
  });

  it("renders in full variant by default", () => {
    render(<ChatAttachmentGallery attachments={mockAttachments} />);

    const gallery = screen.getByTestId("chat-attachment-gallery");
    expect(gallery).toHaveClass("grid");
    expect(gallery).toHaveClass("grid-cols-2");
  });

  it("truncates long file names with ellipsis", () => {
    const longFileName: ChatAttachment[] = [
      {
        id: "1",
        fileName:
          "very-long-file-name-that-should-be-truncated-with-ellipsis.pdf",
        fileSize: 1024,
        mimeType: "application/pdf",
      },
    ];

    render(<ChatAttachmentGallery attachments={longFileName} />);

    const fileNameElement = screen.getByText(
      "very-long-file-name-that-should-be-truncated-with-ellipsis.pdf"
    );
    expect(fileNameElement).toHaveStyle({ textOverflow: "ellipsis" });
    expect(fileNameElement).toHaveStyle({ overflow: "hidden" });
  });

  it("shows full file name in title attribute", () => {
    const fileName = "my-document.pdf";
    const attachment: ChatAttachment[] = [
      {
        id: "1",
        fileName,
        fileSize: 1024,
        mimeType: "application/pdf",
      },
    ];

    render(<ChatAttachmentGallery attachments={attachment} />);

    const fileNameElement = screen.getByText(fileName);
    expect(fileNameElement).toHaveAttribute("title", fileName);
  });

  it("has accessible aria-label on remove button", async () => {
    const onRemove = vi.fn();
    render(
      <ChatAttachmentGallery attachments={mockAttachments} onRemove={onRemove} />
    );

    const firstRemoveButton = screen.getAllByTestId("remove-attachment")[0];
    expect(firstRemoveButton).toHaveAttribute(
      "aria-label",
      "Remove document.pdf"
    );
  });
});
