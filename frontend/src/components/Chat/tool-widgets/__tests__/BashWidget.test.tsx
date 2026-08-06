import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { render, screen, act, renderHook } from "@testing-library/react";
import { BashWidget } from "../BashWidget";
import { ToolCallStoreKeyContext } from "../ToolCallStoreKeyContext";
import { useChatStore } from "@/stores/chatStore";
import { useDelegateCardExpansion } from "../../useDelegateCardExpansion";
import type { ToolCall } from "../shared.constants";

const STORE_KEY = "task_execution:test-task-123";

function makeToolCall(overrides: Partial<ToolCall> = {}): ToolCall {
  return {
    id: "bash-tool-1",
    name: "Bash",
    arguments: { command: "npm test", description: "Run tests" },
    result: "Tests passed",
    ...overrides,
  };
}

function renderWithContext(toolCall: ToolCall, storeKey: string | null = STORE_KEY) {
  return render(
    <ToolCallStoreKeyContext.Provider value={storeKey}>
      <BashWidget toolCall={toolCall} />
    </ToolCallStoreKeyContext.Provider>
  );
}

describe("BashWidget — duration display", () => {
  beforeEach(() => {
    // Reset store state before each test
    useChatStore.setState({
      toolCallStartTimes: {},
      toolCallCompletionTimestamps: {},
    });
  });

  describe("no duration when timing unavailable (backward compat)", () => {
    it("shows no duration when no storeKey is provided", () => {
      render(<BashWidget toolCall={makeToolCall()} />);
      expect(screen.queryByTestId("bash-duration")).not.toBeInTheDocument();
    });

    it("shows no duration when storeKey is null", () => {
      renderWithContext(makeToolCall(), null);
      expect(screen.queryByTestId("bash-duration")).not.toBeInTheDocument();
    });

    it("shows no duration when tool call has no timing in store", () => {
      renderWithContext(makeToolCall());
      expect(screen.queryByTestId("bash-duration")).not.toBeInTheDocument();
    });
  });

  describe("live elapsed timer during execution", () => {
    beforeEach(() => {
      vi.useFakeTimers();
    });

    afterEach(() => {
      vi.useRealTimers();
    });

    it("shows elapsed time when startTime is set and no completedAt", () => {
      const startTime = Date.now() - 5000; // 5 seconds ago
      useChatStore.setState({
        toolCallStartTimes: { [STORE_KEY]: { "bash-tool-1": startTime } },
        toolCallCompletionTimestamps: {},
      });

      renderWithContext(makeToolCall());

      expect(screen.getByTestId("bash-duration")).toBeInTheDocument();
      expect(screen.getByTestId("bash-duration")).toHaveTextContent("5s");
    });

    it("increments live elapsed timer every second", () => {
      const startTime = Date.now() - 10_000; // 10 seconds ago
      useChatStore.setState({
        toolCallStartTimes: { [STORE_KEY]: { "bash-tool-1": startTime } },
        toolCallCompletionTimestamps: {},
      });

      renderWithContext(makeToolCall());
      expect(screen.getByTestId("bash-duration")).toHaveTextContent("10s");

      act(() => {
        vi.advanceTimersByTime(1000);
      });
      expect(screen.getByTestId("bash-duration")).toHaveTextContent("11s");

      act(() => {
        vi.advanceTimersByTime(3000);
      });
      expect(screen.getByTestId("bash-duration")).toHaveTextContent("14s");
    });
  });

  describe("static final duration after completion", () => {
    it("shows static final duration when both startTime and completedAt are set", () => {
      const startTime = Date.now() - 30_000; // started 30s ago
      const completedAt = Date.now() - 5_000; // completed 5s ago (took 25s)
      useChatStore.setState({
        toolCallStartTimes: {},
        toolCallCompletionTimestamps: { [STORE_KEY]: { "bash-tool-1": completedAt } },
      });
      // Override startTimes to include the start for this tool call
      useChatStore.setState({
        toolCallStartTimes: { [STORE_KEY]: { "bash-tool-1": startTime } },
      });

      renderWithContext(makeToolCall());

      expect(screen.getByTestId("bash-duration")).toBeInTheDocument();
      expect(screen.getByTestId("bash-duration")).toHaveTextContent("25s");
    });

    it("shows static duration (not ticking) when completed", () => {
      vi.useFakeTimers();
      const startTime = Date.now() - 20_000;
      const completedAt = Date.now() - 2_000; // completed 2s ago, took 18s
      useChatStore.setState({
        toolCallStartTimes: { [STORE_KEY]: { "bash-tool-1": startTime } },
        toolCallCompletionTimestamps: { [STORE_KEY]: { "bash-tool-1": completedAt } },
      });

      renderWithContext(makeToolCall());
      expect(screen.getByTestId("bash-duration")).toHaveTextContent("18s");

      // Advance time — duration should NOT change (static)
      act(() => { vi.advanceTimersByTime(5000); });
      expect(screen.getByTestId("bash-duration")).toHaveTextContent("18s");

      vi.useRealTimers();
    });
  });

  describe("other tool calls in same store key do not affect this widget", () => {
    it("shows no duration when only a different tool call has timing", () => {
      useChatStore.setState({
        toolCallStartTimes: { [STORE_KEY]: { "other-tool-id": Date.now() - 5000 } },
        toolCallCompletionTimestamps: {},
      });

      renderWithContext(makeToolCall({ id: "bash-tool-1" }));
      expect(screen.queryByTestId("bash-duration")).not.toBeInTheDocument();
    });
  });

  describe("exit code display still works alongside duration", () => {
    it("shows both duration and exit code badge when completed with exit 0", () => {
      const startTime = Date.now() - 10_000;
      const completedAt = Date.now() - 1_000;
      useChatStore.setState({
        toolCallStartTimes: { [STORE_KEY]: { "bash-tool-1": startTime } },
        toolCallCompletionTimestamps: { [STORE_KEY]: { "bash-tool-1": completedAt } },
      });

      renderWithContext(makeToolCall({ result: "ok" }));

      expect(screen.getByTestId("bash-duration")).toBeInTheDocument();
      expect(screen.getByText("exit 0")).toBeInTheDocument();
    });

    it("reads structured Codex command results with explicit exit_code", () => {
      renderWithContext(
        makeToolCall({
          result: {
            text: "Tests passed",
            exit_code: 0,
            status: "completed",
          },
        }),
      );

      expect(screen.getByText("exit 0")).toBeInTheDocument();
      expect(screen.getByText("Tests passed")).toBeInTheDocument();
    });
  });
});

describe("ToolCallStoreKeyContext activation — context-scoped state", () => {
  beforeEach(() => {
    useChatStore.setState({
      toolCallStartTimes: {},
      toolCallCompletionTimestamps: {},
      activeConversationIds: {},
      delegateExpansionByConversation: {},
    });
  });

  it("scopes bash duration to the active store key and clears on a conversation switch", () => {
    const startTime = Date.now() - 30_000;
    const completedAt = Date.now() - 5_000; // took 25s
    useChatStore.setState({
      toolCallStartTimes: { "project:conv-a": { "bash-tool-1": startTime } },
      toolCallCompletionTimestamps: { "project:conv-a": { "bash-tool-1": completedAt } },
    });

    const { rerender } = render(
      <ToolCallStoreKeyContext.Provider value="project:conv-a">
        <BashWidget toolCall={makeToolCall()} />
      </ToolCallStoreKeyContext.Provider>,
    );
    expect(screen.getByTestId("bash-duration")).toHaveTextContent("25s");

    // Switching context keys (e.g. conversation switch) must not leak the previous key's duration.
    rerender(
      <ToolCallStoreKeyContext.Provider value="project:conv-b">
        <BashWidget toolCall={makeToolCall()} />
      </ToolCallStoreKeyContext.Provider>,
    );
    expect(screen.queryByTestId("bash-duration")).not.toBeInTheDocument();
  });

  it("drops delegate-card expansion state for the previous conversation on a conversation switch", () => {
    const storeKey = "project:conv-switch";
    act(() => {
      useChatStore.getState().setActiveConversation(storeKey, "conversation-a");
    });

    const { result } = renderHook(() => useDelegateCardExpansion("delegate-1", true), {
      wrapper: ({ children }) => (
        <ToolCallStoreKeyContext.Provider value={storeKey}>{children}</ToolCallStoreKeyContext.Provider>
      ),
    });

    act(() => {
      result.current.setIsExpanded(true);
    });
    expect(result.current.isExpanded).toBe(true);
    expect(
      useChatStore.getState().delegateExpansionByConversation["conversation-a"]?.["delegate-1"],
    ).toBe(true);

    act(() => {
      useChatStore.getState().setActiveConversation(storeKey, "conversation-b");
    });

    expect(useChatStore.getState().delegateExpansionByConversation["conversation-a"]).toBeUndefined();
    expect(result.current.isExpanded).toBe(false);
  });
});
