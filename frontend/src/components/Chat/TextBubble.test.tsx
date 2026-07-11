/**
 * TextBubble Component Tests
 *
 * Tests for the text bubble component with:
 * - Markdown rendering for both user and assistant messages
 * - User vs assistant styling
 * - Copy button functionality
 */

import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ComponentProps } from "react";
import { describe, it, expect, vi } from "vitest";
import { TextBubble } from "./TextBubble";
import { openPath } from "@tauri-apps/plugin-opener";

const markdownRenderProbe = vi.hoisted(() => vi.fn());

vi.mock("react-markdown", async (importOriginal) => {
  const actual = await importOriginal<typeof import("react-markdown")>();

  return {
    ...actual,
    default: function CountedReactMarkdown(props: ComponentProps<typeof actual.default>) {
      markdownRenderProbe();
      return <actual.default {...props} />;
    },
  };
});

vi.mock("@tauri-apps/plugin-opener", () => ({
  openPath: vi.fn().mockResolvedValue(undefined),
  revealItemInDir: vi.fn().mockResolvedValue(undefined),
}));

describe("TextBubble", () => {

  // ============================================================================
  // Rendering Tests
  // ============================================================================

  describe("rendering", () => {
    it("renders plain text for user messages", () => {
      render(<TextBubble text="Hello world" isUser={true} />);
      expect(screen.getByText("Hello world")).toBeInTheDocument();
    });

    it("renders plain text for assistant messages", () => {
      render(<TextBubble text="Hello world" isUser={false} />);
      expect(screen.getByText("Hello world")).toBeInTheDocument();
    });

  });

  // ============================================================================
  // Markdown Rendering Tests
  // ============================================================================

  describe("markdown rendering", () => {
    it("paints plain text before markdown hydration", async () => {
      render(<TextBubble text="# Heading 1" isUser={false} />);

      expect(screen.getByText("# Heading 1")).toBeInTheDocument();
      expect(await screen.findByRole("heading", { level: 1 })).toHaveTextContent("Heading 1");
    });

    it("renders headings in user messages", async () => {
      render(<TextBubble text="# Heading 1" isUser={true} />);
      expect(await screen.findByRole("heading", { level: 1 })).toHaveTextContent("Heading 1");
    });

    it("renders headings in assistant messages", async () => {
      render(<TextBubble text="# Heading 1" isUser={false} />);
      expect(await screen.findByRole("heading", { level: 1 })).toHaveTextContent("Heading 1");
    });

    it("renders lists in user messages", async () => {
      const text = "- Item 1\n- Item 2\n- Item 3";
      render(<TextBubble text={text} isUser={true} />);
      expect(await screen.findByText("Item 1")).toBeInTheDocument();
      expect(screen.getByText("Item 2")).toBeInTheDocument();
      expect(screen.getByText("Item 3")).toBeInTheDocument();
    });

    it("renders lists in assistant messages", async () => {
      const text = "- Item 1\n- Item 2\n- Item 3";
      render(<TextBubble text={text} isUser={false} />);
      expect(await screen.findByText("Item 1")).toBeInTheDocument();
      expect(screen.getByText("Item 2")).toBeInTheDocument();
      expect(screen.getByText("Item 3")).toBeInTheDocument();
    });

    it("renders inline code in user messages", async () => {
      render(<TextBubble text="Use `const` for constants" isUser={true} />);
      expect(await screen.findByText("const")).toBeInTheDocument();
    });

    it("renders inline code in assistant messages", async () => {
      render(<TextBubble text="Use `const` for constants" isUser={false} />);
      expect(await screen.findByText("const")).toBeInTheDocument();
    });

    it("renders code blocks in user messages", async () => {
      const text = "```javascript\nconst x = 1;\n```";
      render(<TextBubble text={text} isUser={true} />);
      expect(await screen.findByText("const x = 1;")).toBeInTheDocument();
    });

    it("renders code blocks in assistant messages", async () => {
      const text = "```javascript\nconst x = 1;\n```";
      render(<TextBubble text={text} isUser={false} />);
      expect(await screen.findByText("const x = 1;")).toBeInTheDocument();
    });

    it("renders bold text in user messages", async () => {
      render(<TextBubble text="This is **bold** text" isUser={true} />);
      expect(await screen.findByText("bold")).toBeInTheDocument();
    });

    it("renders bold text in assistant messages", async () => {
      render(<TextBubble text="This is **bold** text" isUser={false} />);
      expect(await screen.findByText("bold")).toBeInTheDocument();
    });

    it("hydrates the markdown renderer only once", async () => {
      const { rerender } = render(
        <TextBubble text="# First heading" isUser={false} isStreaming />,
      );

      const markdown = await screen.findByRole("heading", { level: 1 });
      rerender(<TextBubble text="# Second heading" isUser={false} isStreaming />);

      expect(screen.getByRole("heading", { level: 1 })).toBe(markdown);
      expect(screen.queryByText("# Second heading")).not.toBeInTheDocument();
    });

    it("keeps the markdown subtree mounted across rapid streaming chunks", async () => {
      const { rerender } = render(
        <TextBubble text="**first**" isUser={false} isStreaming />,
      );
      const markdown = (await screen.findByText("first")).closest("p");
      expect(markdown).not.toBeNull();

      rerender(<TextBubble text="**second**" isUser={false} isStreaming />);
      rerender(<TextBubble text="**third**" isUser={false} isStreaming />);

      expect(screen.getByText("first").closest("p")).toBe(markdown);
      expect(screen.queryByText("third")).not.toBeInTheDocument();
    });

    it("does not re-render markdown during rapid updates inside a throttle window", async () => {
      markdownRenderProbe.mockClear();
      const { rerender } = render(
        <TextBubble text="**first**" isUser={false} isStreaming />,
      );
      await screen.findByText("first");

      rerender(<TextBubble text="**second**" isUser={false} isStreaming />);
      rerender(<TextBubble text="**third**" isUser={false} isStreaming />);

      expect(markdownRenderProbe).toHaveBeenCalledTimes(1);
    });

    it("flushes the latest throttled streaming text immediately when finalizing", async () => {
      const { rerender } = render(
        <TextBubble text="**first**" isUser={false} isStreaming />,
      );
      await screen.findByText("first");

      rerender(<TextBubble text="latest streamed text" isUser={false} isStreaming />);
      rerender(<TextBubble text="latest streamed text" isUser={false} isStreaming={false} />);

      expect(screen.getByText("latest streamed text")).toBeInTheDocument();
    });

    it("opens absolute local file links with the system opener instead of navigating the webview", async () => {
      const user = userEvent.setup();
      render(
        <TextBubble
          text="[agent-models.ts](/tmp/ralphx-worktree/frontend/src/lib/agent-models.ts:1)"
          isUser={false}
        />
      );

      const link = await screen.findByRole("link", { name: "agent-models.ts" });
      expect(link).toHaveAttribute(
        "href",
        "file:///tmp/ralphx-worktree/frontend/src/lib/agent-models.ts",
      );

      await user.click(link);

      expect(openPath).toHaveBeenCalledWith(
        "/tmp/ralphx-worktree/frontend/src/lib/agent-models.ts",
      );
    });
  });

  // ============================================================================
  // Styling Tests
  // ============================================================================

  describe("styling", () => {
    it("applies token-backed user styling", () => {
      const { container } = render(<TextBubble text="Hello" isUser={true} />);
      const bubble = container.firstChild as HTMLElement;
      expect(bubble).toHaveStyle({
        background: "var(--chat-user-bubble-bg)",
        color: "var(--chat-user-bubble-text)",
      });
      expect(bubble.getAttribute("style")).toContain("border-color: var(--chat-user-bubble-border)");
      expect(bubble.getAttribute("style")).toContain("border-style: solid");
      expect(bubble.getAttribute("style")).toContain("border-width: 1px");
    });

    it("renders assistant text without a filled bubble background", () => {
      const { container } = render(<TextBubble text="Hello" isUser={false} />);
      const bubble = container.firstChild as HTMLElement;
      expect(bubble).toHaveStyle({ background: "transparent" });
    });

    it("applies rounded corners to user bubbles", () => {
      const { container } = render(<TextBubble text="Hello" isUser={true} />);
      const bubble = container.firstChild as HTMLElement;
      expect(bubble).toHaveClass("rounded-xl");
    });

    it("keeps user bubble padding", () => {
      const { container } = render(<TextBubble text="Hello" isUser={true} />);
      const bubble = container.firstChild as HTMLElement;
      expect(bubble).toHaveClass("px-3");
      expect(bubble).toHaveClass("py-2");
    });

    it("removes bubble padding and rounding for assistant text", () => {
      const { container } = render(<TextBubble text="Hello" isUser={false} />);
      const bubble = container.firstChild as HTMLElement;
      expect(bubble).toHaveClass("px-0");
      expect(bubble).toHaveClass("py-0");
      expect(bubble).toHaveClass("rounded-none");
    });

    it("uses a container-aware max width instead of a fixed bubble cap", () => {
      const { container } = render(<TextBubble text="Hello" isUser={true} />);
      const bubble = container.firstChild as HTMLElement;
      expect(bubble).toHaveStyle({ maxWidth: "min(85%, 620px)" });
    });
  });

  describe("copy control ownership", () => {
    it("does not render an inline copy button inside the text bubble", () => {
      render(<TextBubble text="Hello" isUser={true} />);
      expect(screen.queryByLabelText("Copy message")).not.toBeInTheDocument();
    });
  });
});
