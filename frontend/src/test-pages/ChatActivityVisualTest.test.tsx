import { render, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ChatActivityVisualTestPage } from "./ChatActivityVisualTest";
import type { ContentBlockItem } from "@/components/Chat/MessageItem";

vi.mock("@/components/Chat/MessageItem", () => ({
  MessageItem: ({
    contentBlocks,
    providerHarness,
    logicalModel,
  }: {
    contentBlocks?: ContentBlockItem[] | null;
    providerHarness?: string | null;
    logicalModel?: string | null;
  }) => (
    <div data-testid={`message-item-${providerHarness ?? "unknown"}`}>
      <span>{logicalModel}</span>
      {(contentBlocks ?? []).map((block) => (
        <span key={block.id}>{block.name}</span>
      ))}
    </div>
  ),
}));

describe("ChatActivityVisualTestPage", () => {
  it("renders the universal chat-context matrix for Claude and Codex fixtures", async () => {
    window.history.replaceState({}, "", "/?test=chat-activity&theme=light");

    render(<ChatActivityVisualTestPage />);

    await waitFor(() => {
      expect(document.documentElement).toHaveAttribute("data-theme", "light");
    });
    expect(screen.getByRole("heading", {
      name: "Activity summaries and delegated tasks",
    })).toBeInTheDocument();

    const contexts = screen.getByLabelText("Supported chat contexts");
    [
      "Ideation",
      "Project",
      "Task",
      "Execution",
      "Review",
      "Merge",
      "Branch update",
      "Delegation",
    ].forEach((context) => {
      expect(within(contexts).getByText(context)).toBeInTheDocument();
    });

    const claude = screen.getByTestId("message-item-claude");
    expect(within(claude).getByText("claude-sonnet-4-6")).toBeInTheDocument();
    expect(within(claude).getByText("Write")).toBeInTheDocument();
    expect(within(claude).getAllByText("mcp__ralphx__delegate_start")).toHaveLength(2);
    expect(within(claude).getAllByText("mcp__ralphx__delegate_wait")).toHaveLength(2);

    const codex = screen.getByTestId("message-item-codex");
    expect(within(codex).getByText("gpt-5.5")).toBeInTheDocument();
    expect(within(codex).getByText("write")).toBeInTheDocument();
    expect(within(codex).getAllByText("ralphx::delegate_start")).toHaveLength(2);
    expect(within(codex).getAllByText("ralphx::delegate_wait")).toHaveLength(2);

    expect(screen.getByRole("button", {
      name: "Agent thinking… Expand thinking details.",
    })).toBeInTheDocument();
    expect(screen.getByRole("button", {
      name: "Agent thinking… · ~2,000 tokens Expand thinking details.",
    })).toBeInTheDocument();
    expect(screen.getByRole("button", {
      name: "Agent thought for 2s Expand thinking details.",
    })).toBeInTheDocument();
    expect(screen.getByRole("button", {
      name: "Agent thought for 6s · 3 steps Expand thinking details.",
    })).toBeInTheDocument();
  });
});
