/**
 * MessageAttachments tests
 */

import { describe, it, expect, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import { MessageAttachments } from "./MessageAttachments";

describe("MessageAttachments", () => {
  const mockAttachments = [
    {
      id: "att-1",
      fileName: "document.txt",
      fileSize: 1024,
      mimeType: "text/plain",
      filePath: "/path/to/document.txt",
    },
    {
      id: "att-2",
      fileName: "screenshot.png",
      fileSize: 2048000,
      mimeType: "image/png",
      filePath: "/path/to/screenshot.png",
    },
    {
      id: "att-3",
      fileName: "report.pdf",
      fileSize: 512000,
      mimeType: "application/pdf",
      filePath: "/path/to/report.pdf",
    },
  ];

  it("should render nothing if attachments array is empty", () => {
    const { container } = render(<MessageAttachments attachments={[]} />);
    expect(container.firstChild).toBeNull();
  });

  it("shows host attachment availability instead of an empty claim", () => {
    render(<MessageAttachments attachments={[]} availability="unavailable" />);

    expect(screen.getByText("Attachments are on the host.")).toBeInTheDocument();
    expect(screen.queryByTestId("attachment-chip")).not.toBeInTheDocument();
  });

  it("should render image previews and file chips for mixed attachments", () => {
    render(<MessageAttachments attachments={mockAttachments} />);

    expect(screen.getByText("document.txt")).toBeInTheDocument();
    expect(screen.getByText("screenshot.png")).toBeInTheDocument();
    expect(screen.getByText("report.pdf")).toBeInTheDocument();
    expect(screen.getByTestId("attachment-image-grid")).toBeInTheDocument();
    expect(screen.getAllByTestId("attachment-image-tile")).toHaveLength(1);
    expect(screen.getAllByTestId("attachment-chip")).toHaveLength(2);
  });

  it("should format file sizes correctly", () => {
    render(<MessageAttachments attachments={mockAttachments} />);

    // 1024 bytes = 1.0 KB
    expect(screen.getByText("1.0 KB")).toBeInTheDocument();
    // 2048000 bytes = 2.0 MB
    expect(screen.getByText("2.0 MB")).toBeInTheDocument();
    // 512000 bytes = 500.0 KB
    expect(screen.getByText("500.0 KB")).toBeInTheDocument();
  });

  it("should display correct icons for different file types", () => {
    render(<MessageAttachments attachments={mockAttachments} />);

    const chips = screen.getAllByTestId("attachment-chip");
    expect(chips).toHaveLength(2);
    expect(screen.getAllByTestId("attachment-image-tile")).toHaveLength(1);
  });

  it("should render image attachments in a preview grid", () => {
    render(<MessageAttachments attachments={[mockAttachments[1]]} />);

    expect(screen.getByTestId("attachment-image-grid")).toBeInTheDocument();
    const preview = screen.getByTestId("attachment-image-preview");
    expect(preview).toHaveAttribute("src", "/path/to/screenshot.png");
  });

  it("should render optimistic image attachments from frontend preview URLs", () => {
    render(
      <MessageAttachments
        attachments={[
          {
            id: "optimistic-image",
            fileName: "local-image.png",
            fileSize: 42,
            mimeType: "image/png",
            previewUrl: "blob:local-image-preview",
          },
        ]}
      />
    );

    expect(screen.getByTestId("attachment-image-grid")).toBeInTheDocument();
    expect(screen.getByTestId("attachment-image-preview")).toHaveAttribute(
      "src",
      "blob:local-image-preview"
    );
    expect(screen.queryByTestId("attachment-chip")).not.toBeInTheDocument();
  });

  it("should render multiple images in the preview grid", () => {
    const images = [
      mockAttachments[1],
      {
        id: "att-4",
        fileName: "diagram.jpg",
        fileSize: 128000,
        mimeType: "image/jpeg",
        filePath: "/path/to/diagram.jpg",
      },
      {
        id: "att-5",
        fileName: "mockup.webp",
        fileSize: 256000,
        mimeType: "image/webp",
        filePath: "/path/to/mockup.webp",
      },
    ];

    render(<MessageAttachments attachments={images} />);

    expect(screen.getByTestId("attachment-image-grid")).toHaveClass("grid-cols-2");
    expect(screen.getAllByTestId("attachment-image-tile")).toHaveLength(3);
    expect(screen.getAllByTestId("attachment-image-preview")).toHaveLength(3);
  });

  it("should open a large image preview when an image tile is clicked", () => {
    render(<MessageAttachments attachments={[mockAttachments[1]]} />);

    fireEvent.click(screen.getByTestId("attachment-image-tile"));

    expect(screen.getByTestId("attachment-image-dialog")).toBeInTheDocument();
    expect(screen.getByTestId("attachment-image-large")).toHaveAttribute(
      "src",
      "/path/to/screenshot.png"
    );
    expect(screen.getAllByText("screenshot.png")).toHaveLength(2);
  });

  it("should fall back to a chip when image preview loading fails", () => {
    render(<MessageAttachments attachments={[mockAttachments[1]]} />);

    fireEvent.error(screen.getByTestId("attachment-image-preview"));

    expect(screen.queryByTestId("attachment-image-grid")).not.toBeInTheDocument();
    expect(screen.getByTestId("attachment-chip")).toBeInTheDocument();
    expect(screen.getByText("screenshot.png")).toBeInTheDocument();
  });

  it("should close the large preview when the selected image fails to load", () => {
    render(<MessageAttachments attachments={[mockAttachments[1]]} />);

    fireEvent.click(screen.getByTestId("attachment-image-tile"));
    fireEvent.error(screen.getByTestId("attachment-image-large"));

    expect(screen.queryByTestId("attachment-image-dialog")).not.toBeInTheDocument();
    expect(screen.getByTestId("attachment-chip")).toBeInTheDocument();
  });

  it("should truncate long file names", () => {
    const longName = [
      {
        id: "att-long",
        fileName: "very_long_filename_that_should_be_truncated_to_fit_in_the_chip.txt",
        fileSize: 100,
        mimeType: "text/plain",
        filePath: "/path/to/file.txt",
      },
    ];

    const { container } = render(<MessageAttachments attachments={longName} />);

    // Check that text overflow is set to ellipsis (the span element)
    const fileNameElement = container.querySelector('span[title="very_long_filename_that_should_be_truncated_to_fit_in_the_chip.txt"]');
    expect(fileNameElement).toBeInTheDocument();
    // Verify max-width class is present for truncation
    expect(fileNameElement).toHaveClass("max-w-[180px]");
  });

  it("should handle files with no MIME type", () => {
    const noMimeType = [
      {
        id: "att-no-mime",
        fileName: "unknown.dat",
        fileSize: 500,
        filePath: "/path/to/unknown.dat",
      },
    ];

    render(<MessageAttachments attachments={noMimeType} />);
    expect(screen.getByText("unknown.dat")).toBeInTheDocument();
  });

  it("should format very small files (< 1024 bytes)", () => {
    const smallFile = [
      {
        id: "att-small",
        fileName: "tiny.txt",
        fileSize: 42,
        mimeType: "text/plain",
        filePath: "/path/to/tiny.txt",
      },
    ];

    render(<MessageAttachments attachments={smallFile} />);
    expect(screen.getByText("42 B")).toBeInTheDocument();
  });

  it("should render code file icons for common code extensions", () => {
    const codeFile = [
      {
        id: "att-code",
        fileName: "script.ts",
        fileSize: 1000,
        mimeType: "application/typescript",
        filePath: "/path/to/script.ts",
      },
    ];

    render(<MessageAttachments attachments={codeFile} />);
    expect(screen.getByText("script.ts")).toBeInTheDocument();
  });

  it("should handle onClick callback when provided", () => {
    const onClick = vi.fn();
    const attachments = [mockAttachments[0]];

    render(<MessageAttachments attachments={attachments} onClick={onClick} />);

    const chip = screen.getByTestId("attachment-chip");
    chip.click();

    expect(onClick).toHaveBeenCalledWith("att-1", "/path/to/document.txt");
  });

  it("should apply hover styles to chips", () => {
    render(<MessageAttachments attachments={[mockAttachments[0]]} />);

    const chip = screen.getByTestId("attachment-chip");
    expect(chip).toHaveStyle({ background: "var(--bg-elevated)" });

    fireEvent.mouseEnter(chip);
    expect(chip).toHaveStyle({ background: "var(--bg-hover)" });

    fireEvent.mouseLeave(chip);
    expect(chip).toHaveStyle({ background: "var(--bg-elevated)" });
  });

  it("should render in compact horizontal layout", () => {
    const { container } = render(<MessageAttachments attachments={mockAttachments} />);

    const wrapper = container.firstChild as HTMLElement;
    expect(wrapper).toHaveClass("space-y-2");
    expect(screen.getByTestId("attachment-image-grid")).toHaveClass("grid");
    expect(screen.getAllByTestId("attachment-chip")).toHaveLength(2);
  });
});
