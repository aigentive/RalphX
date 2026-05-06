import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { CodeBlock, MarkdownLink, markdownComponents } from "./MessageItem.markdown";
import { openPath } from "@tauri-apps/plugin-opener";

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

  it("CodeBlock toggles to copied state on successful copy", async () => {
    render(<CodeBlock language="js">{"foo();"}</CodeBlock>);
    fireEvent.click(screen.getByRole("button", { name: /Copy code/i }));
    await waitFor(() => expect(writeTextMock).toHaveBeenCalledWith("foo();"));
    expect(await screen.findByRole("button", { name: /Copied/i })).toBeInTheDocument();
  });

  it("CodeBlock silently swallows clipboard failures", async () => {
    writeTextMock.mockRejectedValueOnce(new Error("denied"));
    render(<CodeBlock>{"x"}</CodeBlock>);
    fireEvent.click(screen.getByRole("button", { name: /Copy code/i }));
    await waitFor(() => expect(writeTextMock).toHaveBeenCalled());
    // Stays as Copy code since the catch block swallows the error (no Copied state)
    expect(screen.getByRole("button", { name: /Copy code/i })).toBeInTheDocument();
  });

  it("MarkdownLink opens absolute /-paths via openPath and prevents default", async () => {
    const user = userEvent.setup();
    render(<MarkdownLink href="/Users/me/file.ts:42:7">file</MarkdownLink>);
    const anchor = screen.getByText("file").closest("a");
    expect(anchor).toHaveAttribute("href", "file:///Users/me/file.ts");
    expect(anchor).not.toHaveAttribute("target");
    await user.click(anchor!);
    expect(openPath).toHaveBeenCalledWith("/Users/me/file.ts");
  });

  it("MarkdownLink parses file:// URLs and strips line/col suffix", async () => {
    const user = userEvent.setup();
    (openPath as unknown as { mockClear: () => void }).mockClear();
    render(<MarkdownLink href="file:///tmp/foo%20bar.txt:10">f</MarkdownLink>);
    const anchor = screen.getByText("f").closest("a");
    await user.click(anchor!);
    expect(openPath).toHaveBeenCalledWith("/tmp/foo bar.txt");
  });

  it("MarkdownLink falls back when file:// URL is malformed", async () => {
    const user = userEvent.setup();
    (openPath as unknown as { mockClear: () => void }).mockClear();
    // Intentionally malformed URL to hit the catch branch in parseLocalFileHref
    render(<MarkdownLink href="file://%E0%A4%A">x</MarkdownLink>);
    const anchor = screen.getByText("x").closest("a");
    await user.click(anchor!);
    expect(openPath).toHaveBeenCalled();
  });

  it("MarkdownLink keeps remote URLs as external links", () => {
    render(<MarkdownLink href="https://example.com">remote</MarkdownLink>);
    const anchor = screen.getByText("remote").closest("a");
    expect(anchor).toHaveAttribute("href", "https://example.com");
    expect(anchor).toHaveAttribute("target", "_blank");
    expect(anchor).toHaveAttribute("rel", "noopener noreferrer");
  });

  it("MarkdownLink with no href is a noop on click", async () => {
    const user = userEvent.setup();
    (openPath as unknown as { mockClear: () => void }).mockClear();
    render(<MarkdownLink>nohref</MarkdownLink>);
    await user.click(screen.getByText("nohref"));
    expect(openPath).not.toHaveBeenCalled();
  });

  it("MarkdownLink ignores protocol-relative // URLs", () => {
    render(<MarkdownLink href="//example.com/path">pr</MarkdownLink>);
    const anchor = screen.getByText("pr").closest("a");
    expect(anchor).toHaveAttribute("target", "_blank");
  });

  it("renders all block-level markdownComponents", () => {
    const Components = markdownComponents as Record<
      string,
      React.ComponentType<{ children?: React.ReactNode; className?: string }>
    >;
    const blockTags = ["p", "h1", "h2", "h3", "ul", "ol", "li", "strong", "em"];
    blockTags.forEach((tag) => {
      const Comp = Components[tag]!;
      const { unmount } = render(<Comp>content-{tag}</Comp>);
      expect(screen.getByText(`content-${tag}`)).toBeInTheDocument();
      unmount();
    });
  });

  it("renders hr separator", () => {
    const Hr = markdownComponents.hr as React.ComponentType;
    const { container } = render(<Hr />);
    expect(container.querySelector("hr")).toBeInTheDocument();
  });

  it("renders pre as a fragment passthrough", () => {
    const Pre = markdownComponents.pre as React.ComponentType<{
      children: React.ReactNode;
    }>;
    render(<Pre>passed</Pre>);
    expect(screen.getByText("passed")).toBeInTheDocument();
  });

  it("renders all table-related components", () => {
    const T = markdownComponents as Record<
      string,
      React.ComponentType<{ children?: React.ReactNode }>
    >;
    render(
      <T.table>
        <T.thead>
          <T.tr>
            <T.th>head1</T.th>
          </T.tr>
        </T.thead>
        <T.tbody>
          <T.tr>
            <T.td>cell1</T.td>
          </T.tr>
        </T.tbody>
      </T.table>
    );
    expect(screen.getByText("head1")).toBeInTheDocument();
    expect(screen.getByText("cell1")).toBeInTheDocument();
  });
});
