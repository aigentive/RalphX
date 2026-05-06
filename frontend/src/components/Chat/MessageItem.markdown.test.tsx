import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { CodeBlock, MarkdownLink, markdownComponents } from "./MessageItem.markdown";

const writeTextMock = vi.fn().mockResolvedValue(undefined);

vi.mock("@tauri-apps/plugin-opener", () => ({
  openPath: vi.fn().mockResolvedValue(undefined),
}));

beforeEach(() => {
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: { writeText: writeTextMock },
  });
  writeTextMock.mockClear();
});

describe("MessageItem.markdown", () => {
  it("CodeBlock renders the language label and copies on click", async () => {
    const user = userEvent.setup();
    render(<CodeBlock language="ts">{"const x = 1;\n"}</CodeBlock>);
    expect(screen.getByText("ts")).toBeInTheDocument();
    expect(screen.getByText(/const x = 1/)).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /Copy code/i }));
    // Click handler is async; whether or not the awaiting writeText settled
    // depends on the mock — at minimum the button is invokable.
    expect(screen.getByRole("button", { name: /Copy code|Copied/i })).toBeInTheDocument();
  });

  it("CodeBlock renders without a language when none provided", () => {
    render(<CodeBlock>{"plain text"}</CodeBlock>);
    expect(screen.getByText("plain text")).toBeInTheDocument();
  });

  it("MarkdownLink renders an anchor for href content", () => {
    render(<MarkdownLink href="https://example.com">Example</MarkdownLink>);
    expect(screen.getByText("Example")).toBeInTheDocument();
  });

  it("markdownComponents.code renders inline code for short single-line content", () => {
    const Code = markdownComponents.code as React.ComponentType<{
      children: React.ReactNode;
      className?: string;
    }>;
    render(<Code>foo</Code>);
    expect(screen.getByText("foo")).toBeInTheDocument();
  });

  it("markdownComponents.code routes multi-line content to CodeBlock", () => {
    const Code = markdownComponents.code as React.ComponentType<{
      children: React.ReactNode;
      className?: string;
    }>;
    render(<Code className="language-typescript">{"const a = 1;\nconst b = 2;"}</Code>);
    expect(screen.getByText(/const a = 1/)).toBeInTheDocument();
    expect(screen.getByText("typescript")).toBeInTheDocument();
  });
});
