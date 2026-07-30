/**
 * TextBubble Component Tests
 *
 * Tests for the text bubble component with:
 * - Markdown rendering for both user and assistant messages
 * - User vs assistant styling
 * - Copy button functionality
 */

import { act, render, screen } from "@testing-library/react";
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

    it("flushes only the latest streaming markdown update after the throttle window", async () => {
      const { rerender } = render(
        <TextBubble text="**first**" isUser={false} isStreaming />,
      );
      const markdown = (await screen.findByText("first")).closest("p");
      expect(markdown).not.toBeNull();
      vi.useFakeTimers();

      rerender(<TextBubble text="**intermediate**" isUser={false} isStreaming />);
      rerender(<TextBubble text="**latest**" isUser={false} isStreaming />);

      expect(screen.queryByText("intermediate")).not.toBeInTheDocument();
      expect(screen.getByText("first")).toBeInTheDocument();
      act(() => {
        vi.advanceTimersByTime(200);
      });

      expect(screen.getByText("latest")).toBeInTheDocument();
      expect(screen.queryByText("intermediate")).not.toBeInTheDocument();
      expect(screen.getByText("latest").closest("p")).toBe(markdown);
      vi.useRealTimers();
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

    it("renders a fenced code block streamed one character at a time after the flush window", async () => {
      vi.useFakeTimers();
      try {
        const fencedCode = `\`\`\`ts
const answer = 42;
\`\`\``;
        const { rerender } = render(
          <TextBubble text="" isUser={false} isStreaming />,
        );

        for (let index = 1; index <= fencedCode.length; index += 1) {
          rerender(<TextBubble text={fencedCode.slice(0, index)} isUser={false} isStreaming />);
        }

        expect(() => {
          act(() => {
            vi.advanceTimersByTime(200);
          });
        }).not.toThrow();
        const bubble = screen.getByTestId("text-bubble-assistant");
        expect(bubble).not.toHaveTextContent("```");
        expect(bubble).toHaveTextContent("const answer = 42;");
      } finally {
        vi.useRealTimers();
      }
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

    it("disables the inner prose max-width inside user bubbles so short messages don't wrap mid-word", () => {
      // Regression: markdown <p> re-applied maxWidth 85% against the shrink-to-fit
      // bubble width, collapsing "I switched" into "I / switche / d".
      const { container } = render(<TextBubble text="I switched" isUser={true} />);
      const bubble = container.firstChild as HTMLElement;
      expect(bubble.getAttribute("style")).toContain("--chat-prose-max-width: none");
    });

    it("keeps the prose max-width fallback active for assistant text", () => {
      const { container } = render(<TextBubble text="Hello" isUser={false} />);
      const bubble = container.firstChild as HTMLElement;
      expect(bubble.getAttribute("style")).not.toContain("--chat-prose-max-width");
    });

    it("caps markdown blocks through the overridable prose max-width variable", async () => {
      render(<TextBubble text="# Prose heading" isUser={false} />);
      const heading = await screen.findByRole("heading", { name: "Prose heading" });
      expect(heading.getAttribute("style")).toContain(
        "max-width: var(--chat-prose-max-width, min(85%, 620px))",
      );
    });
  });

  describe("copy control ownership", () => {
    it("does not render an inline copy button inside the text bubble", () => {
      render(<TextBubble text="Hello" isUser={true} />);
      expect(screen.queryByLabelText("Copy message")).not.toBeInTheDocument();
    });
  });
});
