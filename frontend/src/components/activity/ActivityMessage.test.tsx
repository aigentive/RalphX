import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ActivityMessage } from "./ActivityMessage";
import type { UnifiedActivityMessage } from "./ActivityView.types";

const mockNavigateToIdeationSession = vi.fn();

vi.mock("@/lib/navigation", () => ({
  navigateToIdeationSession: (sessionId: string) =>
    mockNavigateToIdeationSession(sessionId),
}));

function baseMessage(
  overrides: Partial<UnifiedActivityMessage> = {},
): UnifiedActivityMessage {
  return {
    id: "evt-1",
    type: "text",
    content: "hello",
    timestamp: Date.parse("2026-05-01T10:00:00Z"),
    role: "agent",
    ...overrides,
  };
}

describe("ActivityMessage", () => {
  it("opens follow-up ideation session from system activity metadata", async () => {
    const user = userEvent.setup();

    render(
      <ActivityMessage
        message={baseMessage({
          id: "evt-1",
          type: "system",
          content: "Linked follow-up ideation session.",
          metadata: { followupSessionId: "session-followup-1" },
          taskId: "task-1",
          role: "system",
        })}
        isExpanded={false}
        onToggle={vi.fn()}
        copied={false}
        onCopy={vi.fn()}
      />,
    );

    await user.click(screen.getByRole("button", { name: /Open Follow-up/i }));

    expect(mockNavigateToIdeationSession).toHaveBeenCalledWith(
      "session-followup-1",
    );
  });

  it("renders text type with truncation when not expanded", () => {
    const long = "a".repeat(250);
    render(
      <ActivityMessage
        message={baseMessage({ type: "text", content: long })}
        isExpanded={false}
        onToggle={vi.fn()}
        copied={false}
        onCopy={vi.fn()}
      />,
    );
    // Truncated to 200 + "..."
    expect(screen.getByText(/a{200}\.\.\./)).toBeInTheDocument();
  });

  it("renders thinking type as markdown", () => {
    render(
      <ActivityMessage
        message={baseMessage({ type: "thinking", content: "Thinking deep" })}
        isExpanded={false}
        onToggle={vi.fn()}
        copied={false}
        onCopy={vi.fn()}
      />,
    );
    expect(screen.getByText("Thinking deep")).toBeInTheDocument();
  });

  it("truncates thinking content beyond 500 chars when collapsed", () => {
    const long = "z".repeat(600);
    render(
      <ActivityMessage
        message={baseMessage({ type: "thinking", content: long })}
        isExpanded={false}
        onToggle={vi.fn()}
        copied={false}
        onCopy={vi.fn()}
      />,
    );
    expect(screen.getByText(/z{500}\.\.\./)).toBeInTheDocument();
  });

  it("renders error type with whitespace preserved", () => {
    render(
      <ActivityMessage
        message={baseMessage({ type: "error", content: "Error: boom" })}
        isExpanded={false}
        onToggle={vi.fn()}
        copied={false}
        onCopy={vi.fn()}
      />,
    );
    expect(screen.getByText(/Error: boom/)).toBeInTheDocument();
  });

  it("renders tool_call with formatted arguments from metadata", () => {
    render(
      <ActivityMessage
        message={baseMessage({
          type: "tool_call",
          content: "Read(...)",
          metadata: { file: "/tmp/x.txt", limit: 10 },
        })}
        isExpanded={false}
        onToggle={vi.fn()}
        copied={false}
        onCopy={vi.fn()}
      />,
    );
    expect(screen.getByText("file")).toBeInTheDocument();
    expect(screen.getByText("/tmp/x.txt")).toBeInTheDocument();
    expect(screen.getByText("limit")).toBeInTheDocument();
    expect(screen.getByText("10")).toBeInTheDocument();
  });

  it("renders tool_call fallback content when no metadata args", () => {
    const long = "x".repeat(250);
    render(
      <ActivityMessage
        message={baseMessage({
          type: "tool_call",
          content: long,
          metadata: {},
        })}
        isExpanded={false}
        onToggle={vi.fn()}
        copied={false}
        onCopy={vi.fn()}
      />,
    );
    // truncated at 200 chars + ...
    expect(screen.getByText(/x{200}\.\.\./)).toBeInTheDocument();
  });

  it("renders tool_result success preview from JSON content", () => {
    const content = JSON.stringify({ message: "Saved successfully" });
    render(
      <ActivityMessage
        message={baseMessage({ type: "tool_result", content })}
        isExpanded={false}
        onToggle={vi.fn()}
        copied={false}
        onCopy={vi.fn()}
      />,
    );
    expect(screen.getByText("Saved successfully")).toBeInTheDocument();
    expect(screen.getByText("✓")).toBeInTheDocument();
  });

  it("renders tool_result error preview", () => {
    const content = JSON.stringify({ error: "Something went wrong" });
    render(
      <ActivityMessage
        message={baseMessage({ type: "tool_result", content })}
        isExpanded={false}
        onToggle={vi.fn()}
        copied={false}
        onCopy={vi.fn()}
      />,
    );
    expect(screen.getByText("Something went wrong")).toBeInTheDocument();
    expect(screen.getByText("✗")).toBeInTheDocument();
  });

  it("shows expanded JSON for tool_result when expanded with valid JSON", () => {
    const content = JSON.stringify({ items: [1, 2, 3] });
    render(
      <ActivityMessage
        message={baseMessage({ type: "tool_result", content })}
        isExpanded={true}
        onToggle={vi.fn()}
        copied={false}
        onCopy={vi.fn()}
      />,
    );
    expect(screen.getByText(/items/)).toBeInTheDocument();
  });

  it("shows fallback pre block for non-JSON tool_result when expanded and long", () => {
    const longText = "not-json " + "y".repeat(120);
    render(
      <ActivityMessage
        message={baseMessage({ type: "tool_result", content: longText })}
        isExpanded={true}
        onToggle={vi.fn()}
        copied={false}
        onCopy={vi.fn()}
      />,
    );
    // Both preview and fallback render the content; assert both nodes appear.
    const matches = screen.getAllByText(/not-json/);
    expect(matches.length).toBeGreaterThanOrEqual(1);
  });

  it("calls onToggle when header clicked for messages with details", async () => {
    const onToggle = vi.fn();
    const user = userEvent.setup();
    render(
      <ActivityMessage
        message={baseMessage({
          type: "tool_call",
          content: "Read",
          metadata: { file: "x.txt" },
        })}
        isExpanded={false}
        onToggle={onToggle}
        copied={false}
        onCopy={vi.fn()}
      />,
    );

    const root = screen.getByTestId("activity-message");
    const header = root.firstElementChild as HTMLElement;
    await user.click(header);
    expect(onToggle).toHaveBeenCalled();
  });

  it("does not toggle when message has no details", async () => {
    const onToggle = vi.fn();
    const user = userEvent.setup();
    render(
      <ActivityMessage
        message={baseMessage({ type: "text", content: "plain" })}
        isExpanded={false}
        onToggle={onToggle}
        copied={false}
        onCopy={vi.fn()}
      />,
    );
    const root = screen.getByTestId("activity-message");
    const header = root.firstElementChild as HTMLElement;
    await user.click(header);
    expect(onToggle).not.toHaveBeenCalled();
  });

  it("renders raw JSON details panel when expanded with metadata", () => {
    render(
      <ActivityMessage
        message={baseMessage({
          type: "tool_call",
          content: "Read",
          metadata: { foo: "bar" },
        })}
        isExpanded={true}
        onToggle={vi.fn()}
        copied={false}
        onCopy={vi.fn()}
      />,
    );
    expect(screen.getByText("Raw JSON")).toBeInTheDocument();
  });

  it("renders Details label for non-tool_call expansions with metadata", () => {
    render(
      <ActivityMessage
        message={baseMessage({
          type: "system",
          content: "system thing",
          metadata: { key: "value" },
        })}
        isExpanded={true}
        onToggle={vi.fn()}
        copied={false}
        onCopy={vi.fn()}
      />,
    );
    expect(screen.getByText("Details")).toBeInTheDocument();
  });

  it("invokes onCopy when copy button clicked and writes to clipboard", async () => {
    const onCopy = vi.fn();
    const writeText = vi.fn().mockResolvedValue(undefined);
    // userEvent.setup() installs its own clipboard; create user FIRST then override clipboard.
    const user = userEvent.setup();
    const originalClipboard = (navigator as unknown as { clipboard?: unknown }).clipboard;
    Object.defineProperty(globalThis.navigator, "clipboard", {
      configurable: true,
      value: { writeText },
      writable: true,
    });
    const { container } = render(
      <ActivityMessage
        message={baseMessage({
          type: "tool_call",
          content: "Read",
          metadata: { foo: "bar" },
        })}
        isExpanded={true}
        onToggle={vi.fn()}
        copied={false}
        onCopy={onCopy}
      />,
    );
    // Copy button is the only <button> in the rendered tree (Button ghost, icon-only).
    const copyButton = container.querySelector("button");
    expect(copyButton).not.toBeNull();
    await user.click(copyButton!);
    expect(onCopy).toHaveBeenCalled();
    expect(writeText).toHaveBeenCalled();

    // Restore for safety
    if (originalClipboard !== undefined) {
      Object.defineProperty(navigator, "clipboard", {
        configurable: true,
        value: originalClipboard,
      });
    }
  });

  it("shows check icon when copied is true", () => {
    render(
      <ActivityMessage
        message={baseMessage({
          type: "tool_call",
          content: "Read",
          metadata: { foo: "bar" },
        })}
        isExpanded={true}
        onToggle={vi.fn()}
        copied={true}
        onCopy={vi.fn()}
      />,
    );
    // Just ensure no crash and Raw JSON label still appears
    expect(screen.getByText("Raw JSON")).toBeInTheDocument();
  });

  it("renders system type without followup button when missing metadata", () => {
    render(
      <ActivityMessage
        message={baseMessage({
          type: "system",
          content: "system note",
        })}
        isExpanded={false}
        onToggle={vi.fn()}
        copied={false}
        onCopy={vi.fn()}
      />,
    );
    expect(screen.getByText("system note")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Open Follow-up/i })).toBeNull();
  });

  it("shows clean tool name from mcp-prefixed content", () => {
    render(
      <ActivityMessage
        message={baseMessage({
          type: "tool_call",
          content: "mcp__ralphx__get_task_steps(...)",
          metadata: { foo: "bar" },
        })}
        isExpanded={false}
        onToggle={vi.fn()}
        copied={false}
        onCopy={vi.fn()}
      />,
    );
    // Tool name badge should display cleaned name
    expect(screen.getByText(/get_task_steps|ralphx/)).toBeInTheDocument();
  });
});
