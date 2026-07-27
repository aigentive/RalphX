/**
 * TaskToolCallCard tests
 *
 * Tests the static card for completed Task and Agent tool calls.
 * Agent tool calls share the same argument shape (description, subagent_type, model)
 * so the same component renders both.
 */

import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClientProvider } from "@tanstack/react-query";
import { TaskToolCallCard } from "./TaskToolCallCard";
import type { ToolCall } from "./ToolCallIndicator";
import { createTestQueryClient } from "@/test/store-utils";
import { chatApi, type ChatMessageResponse } from "@/api/chat";

type EventHandler = (payload: unknown) => void;

const listeners = new Map<string, Set<EventHandler>>();

function mockSubscribe(event: string, handler: EventHandler) {
  if (!listeners.has(event)) {
    listeners.set(event, new Set());
  }
  listeners.get(event)!.add(handler);
  return () => {
    listeners.get(event)?.delete(handler);
  };
}

vi.mock("@/providers/EventProvider", () => ({
  useEventBus: () => ({
    subscribe: mockSubscribe,
    emit: vi.fn(),
  }),
}));

// ============================================================================
// Test Data
// ============================================================================

function makeAgentToolCall(overrides?: Partial<ToolCall>): ToolCall {
  return {
    id: "agent-call-1",
    name: "Agent",
    arguments: {
      description: "Explore codebase structure",
      subagent_type: "Explore",
      model: "sonnet",
      prompt: "Find all TypeScript files",
    },
    ...overrides,
  };
}

function makeTaskToolCall(overrides?: Partial<ToolCall>): ToolCall {
  return {
    id: "task-call-1",
    name: "Task",
    arguments: {
      description: "Run tests",
      subagent_type: "general-purpose",
      model: "opus",
      prompt: "Execute the test suite",
    },
    ...overrides,
  };
}

function makeDelegateToolCall(overrides?: Partial<ToolCall>): ToolCall {
  return {
    id: "delegate-call-1",
    name: "delegate_start",
    arguments: {
      agent_name: "ralphx-execution-reviewer",
      prompt: "Review the patch and report blockers",
      harness: "codex",
      model: "gpt-5.4",
    },
    result: [{
      type: "text",
      text: JSON.stringify({
        job_id: "job-123",
        status: "completed",
        content: "Delegated review finished",
        delegated_status: {
          latest_run: {
            harness: "codex",
            provider_session_id: "thread-123",
            upstream_provider: "openai",
            provider_profile: "openai",
            logical_model: "gpt-5.4",
            effective_model_id: "gpt-5.4",
            logical_effort: "high",
            input_tokens: 120,
            output_tokens: 45,
            cache_read_tokens: 10,
            estimated_usd: 0.34,
            started_at: "2026-04-12T10:00:00Z",
            completed_at: "2026-04-12T10:00:06Z",
          },
        },
      }),
    }],
    ...overrides,
  };
}

function renderWithQueryClient(ui: React.ReactElement) {
  const queryClient = createTestQueryClient();
  return render(
    <QueryClientProvider client={queryClient}>{ui}</QueryClientProvider>,
  );
}

afterEach(() => {
  listeners.clear();
  vi.restoreAllMocks();
});

// ============================================================================
// Tests
// ============================================================================

describe("TaskToolCallCard — Agent tool call arguments", () => {
  it("renders the card wrapper with data-testid", () => {
    render(<TaskToolCallCard toolCall={makeAgentToolCall()} />);
    expect(screen.getByTestId("task-tool-call-card")).toBeInTheDocument();
  });

  it("shows description from Agent args", () => {
    render(<TaskToolCallCard toolCall={makeAgentToolCall()} />);
    expect(screen.getByText("Explore codebase structure")).toBeInTheDocument();
  });

  it("shows subagent_type badge from Agent args", () => {
    render(<TaskToolCallCard toolCall={makeAgentToolCall()} />);
    expect(screen.getByText("Explore")).toBeInTheDocument();
  });

  it("shows model badge from Agent args", () => {
    render(<TaskToolCallCard toolCall={makeAgentToolCall()} />);
    expect(screen.getByText("sonnet")).toBeInTheDocument();
  });

  it("shows 'Plan' subagent type badge for Plan agent", () => {
    const tc = makeAgentToolCall({
      arguments: {
        description: "Plan the implementation",
        subagent_type: "Plan",
        model: "opus",
        prompt: "Create a plan",
      },
    });
    render(<TaskToolCallCard toolCall={tc} />);
    expect(screen.getByText("Plan")).toBeInTheDocument();
    expect(screen.getByText("opus")).toBeInTheDocument();
  });

  it("shows 'general-purpose' badge for general-purpose agent type", () => {
    const tc = makeAgentToolCall({
      arguments: {
        description: "Research the codebase",
        subagent_type: "general-purpose",
        model: "haiku",
        prompt: "Find patterns",
      },
    });
    render(<TaskToolCallCard toolCall={tc} />);
    expect(screen.getByText("general-purpose")).toBeInTheDocument();
  });

  it("hides subagent_type badge when subagent_type is missing (defaults to 'agent')", () => {
    const tc = makeAgentToolCall({
      arguments: {
        description: "Do some work",
        model: "sonnet",
        prompt: "...",
      },
    });
    render(<TaskToolCallCard toolCall={tc} />);
    // "agent" default is suppressed — badge should not render
    expect(screen.queryByText("agent")).not.toBeInTheDocument();
  });

  it("falls back to 'Agent task' title when description is missing (Agent call)", () => {
    const tc = makeAgentToolCall({
      arguments: {
        subagent_type: "Explore",
        model: "sonnet",
        prompt: "...",
      },
    });
    render(<TaskToolCallCard toolCall={tc} />);
    expect(screen.getByText("Agent task")).toBeInTheDocument();
  });

  it("does not show model badge when model is missing", () => {
    const tc = makeAgentToolCall({
      arguments: {
        description: "Do something",
        subagent_type: "Explore",
        prompt: "...",
      },
    });
    render(<TaskToolCallCard toolCall={tc} />);
    // No sonnet/opus/haiku badge
    expect(screen.queryByText("sonnet")).not.toBeInTheDocument();
    expect(screen.queryByText("opus")).not.toBeInTheDocument();
    expect(screen.queryByText("haiku")).not.toBeInTheDocument();
  });

  it("handles null/invalid arguments gracefully", () => {
    const tc = makeAgentToolCall({ arguments: null });
    render(<TaskToolCallCard toolCall={tc} />);
    // Falls back to defaults — Agent call shows "Agent task"
    expect(screen.getByTestId("task-tool-call-card")).toBeInTheDocument();
    expect(screen.getByText("Agent task")).toBeInTheDocument();
  });
});

describe("TaskToolCallCard — Agent identity display (name, badge, subtitle)", () => {
  it("delegation mode: includes upstream provider evidence in the harness tooltip", () => {
    render(<TaskToolCallCard toolCall={makeDelegateToolCall()} />);
    expect(screen.getByText("Codex")).toHaveAttribute(
      "title",
      expect.stringContaining("Upstream: openai"),
    );
  });

  it("team mode: shows agent name as card title when name is present", () => {
    const tc = makeAgentToolCall({
      arguments: {
        name: "frontend-researcher",
        description: "Research React patterns in the codebase",
        subagent_type: "Explore",
        model: "sonnet",
        prompt: "Find all React components...",
      },
    });
    render(<TaskToolCallCard toolCall={tc} />);
    // Name is the primary title
    expect(screen.getByText("frontend-researcher")).toBeInTheDocument();
    // Agent/Task type badge always visible
    expect(screen.getByText("Agent")).toBeInTheDocument();
    // Subagent type badge visible (Explore !== "agent")
    expect(screen.getByText("Explore")).toBeInTheDocument();
  });

  it("team mode: shows description as subtitle when agent has name and description", () => {
    const tc = makeAgentToolCall({
      arguments: {
        name: "backend-analyst",
        description: "Analyze Rust service boundaries",
        subagent_type: "Explore",
        model: "opus",
        prompt: "Read all service files...",
      },
    });
    render(<TaskToolCallCard toolCall={tc} />);
    // Name is title, description appears as subtitle
    expect(screen.getByText("backend-analyst")).toBeInTheDocument();
    expect(screen.getByText("Analyze Rust service boundaries")).toBeInTheDocument();
  });

  it("team mode: shows prompt preview as subtitle when agent has name but no description", () => {
    const tc = makeAgentToolCall({
      arguments: {
        name: "test-runner",
        subagent_type: "Bash",
        model: "sonnet",
        prompt: "Run all tests in the src-tauri directory and report results",
      },
    });
    render(<TaskToolCallCard toolCall={tc} />);
    expect(screen.getByText("test-runner")).toBeInTheDocument();
    expect(screen.getByText("Run all tests in the src-tauri directory and report results...")).toBeInTheDocument();
  });

  it("team mode: truncates long prompt preview to 100 chars", () => {
    const longPrompt = "A".repeat(150);
    const tc = makeAgentToolCall({
      arguments: {
        name: "long-prompt-agent",
        subagent_type: "Explore",
        model: "sonnet",
        prompt: longPrompt,
      },
    });
    render(<TaskToolCallCard toolCall={tc} />);
    expect(screen.getByText("A".repeat(100) + "...")).toBeInTheDocument();
  });

  it("solo: uses description as title when no name (no subtitle shown)", () => {
    const tc = makeAgentToolCall({
      arguments: {
        description: "Explore codebase structure",
        subagent_type: "Explore",
        model: "sonnet",
        prompt: "Find all TypeScript files",
      },
    });
    render(<TaskToolCallCard toolCall={tc} />);
    expect(screen.getByText("Explore codebase structure")).toBeInTheDocument();
    // No subtitle — description IS the title, not a subtitle
    const allText = document.body.textContent ?? "";
    const occurrences = (allText.match(/Explore codebase structure/g) ?? []).length;
    expect(occurrences).toBe(1);
  });

  it("always shows Agent/Task type badge regardless of subagent_type", () => {
    const tc = makeAgentToolCall();
    render(<TaskToolCallCard toolCall={tc} />);
    expect(screen.getByText("Agent")).toBeInTheDocument();
  });

  it("hides subagent_type badge when value is the default 'agent'", () => {
    const tc = makeAgentToolCall({
      arguments: {
        description: "Generic agent task",
        subagent_type: "agent",
        model: "sonnet",
        prompt: "Do work",
      },
    });
    render(<TaskToolCallCard toolCall={tc} />);
    // "Agent" type badge should still show
    expect(screen.getByText("Agent")).toBeInTheDocument();
    // But "agent" subagent_type badge should NOT show (would be duplicate/redundant)
    // The only "Agent" text is from the type badge
    expect(screen.getAllByText("Agent")).toHaveLength(1);
  });
});

describe("TaskToolCallCard — RalphX native delegation", () => {
  it("renders delegate label, target agent, and harness/model metadata", () => {
    render(<TaskToolCallCard toolCall={makeDelegateToolCall()} />);
    expect(screen.getByText("Delegate")).toBeInTheDocument();
    expect(screen.getByText("ralphx-execution-reviewer")).toBeInTheDocument();
    expect(screen.getByText("Codex")).toBeInTheDocument();
    expect(screen.getByText("gpt-5.4 · high")).toBeInTheDocument();
  });

  it("shows bound work and its current settlement state on the existing delegate card", () => {
    render(
      <TaskToolCallCard
        toolCall={makeDelegateToolCall({
          result: {
            job_id: "job-assigned",
            status: "running",
            agent_name: "ralphx-execution-reviewer",
            assignment: {
              task_number: 6,
              title: "Review state recovery",
              task_state: "active",
              assignment_state: "completion_requested",
              delegate_agent_name: "ralphx-execution-reviewer",
            },
          },
        })}
      />,
    );

    expect(
      screen.getByText("Assigned work #6 · Review state recovery · completion requested"),
    ).toBeInTheDocument();
  });

  it("shows provider-correct delegated usage and final output when expanded", async () => {
    const user = userEvent.setup();
    render(<TaskToolCallCard toolCall={makeDelegateToolCall()} />);
    expect(screen.getByText(/165 tokens/)).toBeInTheDocument();
    expect(screen.getByText(/\$0\.34/)).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /delegated task: ralphx-execution-reviewer/i }));
    expect(screen.getByText("Delegated review finished")).toBeInTheDocument();
  });

  it("renders the typed empty-completion cause and safe terminal details when expanded", async () => {
    const user = userEvent.setup();
    render(
      <TaskToolCallCard
        toolCall={makeDelegateToolCall({
          result: {
            job_id: "job-no-output",
            status: "failed",
            error: "Codex exited without a response (context=project, code=Some(1), signal=None); diagnostics: Reading additional input from stdin...",
          },
        })}
      />,
    );

    await user.click(screen.getByRole("button", { name: /delegated task: ralphx-execution-reviewer/i }));

    expect(screen.getByText(/Delegate completed without a response/)).toBeInTheDocument();
    expect(screen.getByText(/Exit code: 1/)).toBeInTheDocument();
    expect(screen.queryByText(/Reading additional input from stdin/)).not.toBeInTheDocument();
  });

  it("does not fetch the delegated conversation until the card is expanded", () => {
    const getConversationMessagesPageSpy = vi.spyOn(chatApi, "getConversationMessagesPage").mockResolvedValue({
      conversation: {
        id: "child-conv-1",
        contextType: "project",
        contextId: "project-1",
        claudeSessionId: null,
        providerSessionId: "thread-123",
        providerHarness: "codex",
        upstreamProvider: "openai",
        providerProfile: "openai",
        title: "Delegated reviewer",
        messageCount: 0,
        lastMessageAt: null,
        createdAt: "2026-04-12T10:00:00Z",
        updatedAt: "2026-04-12T10:00:00Z",
      },
      messages: [],
      limit: 40,
      offset: 0,
      totalMessageCount: 0,
      hasOlder: false,
    });

    renderWithQueryClient(
      <TaskToolCallCard
        toolCall={makeDelegateToolCall({
          result: [{
            type: "text",
            text: JSON.stringify({
              job_id: "job-123",
              status: "completed",
              delegated_conversation_id: "child-conv-1",
              content: "Delegated review finished",
            }),
          }],
        })}
      />,
    );

    expect(getConversationMessagesPageSpy).not.toHaveBeenCalled();
  });

  it("renders the delegated conversation transcript inside the expanded card", async () => {
    const getConversationMessagesPageSpy = vi.spyOn(chatApi, "getConversationMessagesPage").mockResolvedValue({
      conversation: {
        id: "child-conv-1",
        contextType: "project",
        contextId: "project-1",
        claudeSessionId: null,
        providerSessionId: "thread-123",
        providerHarness: "codex",
        upstreamProvider: "openai",
        providerProfile: "openai",
        title: "Delegated reviewer",
        messageCount: 2,
        lastMessageAt: "2026-04-12T10:00:06Z",
        createdAt: "2026-04-12T10:00:00Z",
        updatedAt: "2026-04-12T10:00:06Z",
      },
      messages: [
        {
          id: "child-msg-1",
          sessionId: null,
          projectId: null,
          taskId: null,
          role: "user",
          content: "Please inspect the patch",
          metadata: null,
          parentMessageId: null,
          conversationId: "child-conv-1",
          toolCalls: null,
          contentBlocks: null,
          sender: null,
          createdAt: "2026-04-12T10:00:00Z",
        } satisfies ChatMessageResponse,
        {
          id: "child-msg-2",
          sessionId: null,
          projectId: null,
          taskId: null,
          role: "assistant",
          content: "Review complete with no blockers",
          metadata: null,
          parentMessageId: null,
          conversationId: "child-conv-1",
          toolCalls: [
            {
              id: "child-tool-1",
              name: "bash",
              arguments: { command: "git diff --stat" },
            },
          ],
          contentBlocks: null,
          sender: null,
          createdAt: "2026-04-12T10:00:06Z",
        } satisfies ChatMessageResponse,
      ],
      limit: 40,
      offset: 0,
      totalMessageCount: 2,
      hasOlder: false,
    });
    const user = userEvent.setup();

    renderWithQueryClient(
      <TaskToolCallCard
        toolCall={makeDelegateToolCall({
          result: [{
            type: "text",
            text: JSON.stringify({
              job_id: "job-123",
              status: "completed",
              delegated_conversation_id: "child-conv-1",
              content: "Delegated review finished",
            }),
          }],
        })}
      />,
    );

    await user.click(screen.getByRole("button", { name: /delegated task: ralphx-execution-reviewer/i }));

    await waitFor(() => expect(getConversationMessagesPageSpy).toHaveBeenCalledWith("child-conv-1", 40, 0));
    expect(await screen.findByText("Delegated conversation")).toBeInTheDocument();
    expect(screen.getByText("Please inspect the patch")).toBeInTheDocument();
    expect(screen.getByText("Review complete with no blockers")).toBeInTheDocument();
  });
});

describe("TaskToolCallCard — Task tool call (baseline, unchanged behavior)", () => {
  it("renders correctly with Task tool call arguments", () => {
    render(<TaskToolCallCard toolCall={makeTaskToolCall()} />);
    expect(screen.getByTestId("task-tool-call-card")).toBeInTheDocument();
    expect(screen.getByText("Run tests")).toBeInTheDocument();
    // Task type badge always visible
    expect(screen.getByText("Task")).toBeInTheDocument();
    // non-"agent" subagent_type badge visible
    expect(screen.getByText("general-purpose")).toBeInTheDocument();
    expect(screen.getByText("opus")).toBeInTheDocument();
  });

  it("falls back to 'Subagent task' title when Task call has no description", () => {
    const tc = makeTaskToolCall({
      arguments: {
        subagent_type: "general-purpose",
        model: "opus",
        prompt: "Do something",
      },
    });
    render(<TaskToolCallCard toolCall={tc} />);
    expect(screen.getByText("Subagent task")).toBeInTheDocument();
  });
});

describe("TaskToolCallCard — stats rendering", () => {
  it("shows duration, tokens, and tool count from result usage block", () => {
    const result = `
Agent output here.
agentId: abc123def
<usage>total_tokens: 5432
tool_uses: 12
duration_ms: 47000</usage>
    `.trim();
    const tc = makeAgentToolCall({ result });
    render(<TaskToolCallCard toolCall={tc} />);

    // Stats row should show formatted values
    expect(screen.getByText(/47s/)).toBeInTheDocument();
    expect(screen.getByText(/5,432 tokens/)).toBeInTheDocument();
    expect(screen.getByText(/12 tools/)).toBeInTheDocument();
  });

  it("shows no stats row when result has no usage block", () => {
    const tc = makeAgentToolCall({ result: undefined });
    const { container } = render(<TaskToolCallCard toolCall={tc} />);
    // No stats div with a middle dot separator
    expect(container.textContent).not.toContain("\u00B7");
  });

  it("shows tool count for array result with text block containing usage tags", () => {
    const result = [
      {
        type: "text",
        text: "Agent completed work.\nagentId: abc123\n<usage>total_tokens: 200\ntool_uses: 7\nduration_ms: 5000</usage>",
      },
    ];
    const tc = makeAgentToolCall({ result });
    render(<TaskToolCallCard toolCall={tc} />);
    expect(screen.getByText(/7 tools/)).toBeInTheDocument();
  });

  it("shows singular 'tool' when tool use count is 1", () => {
    const result = `Done.\nagentId: abc123\n<usage>total_tokens: 100\ntool_uses: 1\nduration_ms: 1000</usage>`;
    const tc = makeAgentToolCall({ result });
    render(<TaskToolCallCard toolCall={tc} />);
    // Should say "1 tool" not "1 tools"
    expect(screen.getByText(/1 tool$/)).toBeInTheDocument();
  });

  it("shows '0 tools' when tool use count is zero", () => {
    const result = `Done.\nagentId: abc123\n<usage>total_tokens: 100\ntool_uses: 0\nduration_ms: 1000</usage>`;
    const tc = makeAgentToolCall({ result });
    render(<TaskToolCallCard toolCall={tc} />);
    expect(screen.getByText(/0 tools/)).toBeInTheDocument();
  });

  it("shows no stats for array result with only tool_use blocks (no usage tags in text)", () => {
    const result = [
      { type: "tool_use", id: "tc-1", name: "bash", input: { cmd: "ls" } },
      { type: "tool_result", tool_use_id: "tc-1", content: "file.txt" },
    ];
    const tc = makeAgentToolCall({ result });
    const { container } = render(<TaskToolCallCard toolCall={tc} />);
    // No stats row — tool_use blocks have no text with <usage> tags
    expect(container.textContent).not.toContain("\u00B7");
  });

  it("parses agentId with uppercase hex digits (case-insensitive)", () => {
    const result = `Done.\nagentId: ABCDEF123\n<usage>total_tokens: 50\ntool_uses: 2\nduration_ms: 500</usage>`;
    const tc = makeAgentToolCall({ result });
    render(<TaskToolCallCard toolCall={tc} />);
    expect(screen.getByText(/2 tools/)).toBeInTheDocument();
  });

  it("extracts text output when agentId line has no preceding newline", () => {
    // Edge case: text output has no trailing newline before agentId:
    const result = `Agent output with no trailing newlineagentId: abc123\n<usage>total_tokens: 50\ntool_uses: 2\nduration_ms: 500</usage>`;
    const tc = makeAgentToolCall({ result });
    render(<TaskToolCallCard toolCall={tc} />);
    // Stats still parse correctly
    expect(screen.getByText(/2 tools/)).toBeInTheDocument();
  });
});

describe("TaskToolCallCard — expanded body", () => {
  it("shows expanded text output when clicked", async () => {
    const user = userEvent.setup();
    const result = `Agent found 42 TypeScript files.
agentId: abc123
<usage>total_tokens: 100
tool_uses: 3
duration_ms: 2000</usage>`;

    const tc = makeAgentToolCall({ result });
    render(<TaskToolCallCard toolCall={tc} />);

    // Click header to expand
    const header = screen.getByRole("button");
    await user.click(header);

    expect(screen.getByText("Agent found 42 TypeScript files.")).toBeInTheDocument();
  });

  it("shows child tool calls in expanded view when result has content blocks", async () => {
    const user = userEvent.setup();
    // Use a generic tool name (no widget registered) so generic ToolCallIndicator renders
    // and shows the tool name as text directly in the DOM.
    const result = [
      { type: "tool_use", id: "child-1", name: "inspect_code", input: { target: "src/" } },
      { type: "tool_result", tool_use_id: "child-1", content: "inspected" },
      { type: "text", text: "Found files." },
    ];

    const tc = makeAgentToolCall({ result });
    const { container } = render(<TaskToolCallCard toolCall={tc} />);

    // Click header to expand
    const header = screen.getByRole("button");
    await user.click(header);

    // Child tool call should render inside the expanded body (as ToolCallIndicator)
    expect(container.querySelector('[data-testid="tool-call-indicator"]')).toBeInTheDocument();
    // The generic ToolCallIndicator shows the tool name
    expect(screen.getByText("inspect_code")).toBeInTheDocument();
  });
});

describe("TaskToolCallCard — extractTaskStats() result format variations", () => {
  // Helper: render with a result and check if stats row appears
  function renderWithResult(result: unknown) {
    const tc: ToolCall = {
      id: "tc-1",
      name: "Task",
      arguments: { description: "test", subagent_type: "general-purpose" },
      result,
    };
    return render(<TaskToolCallCard toolCall={tc} />);
  }

  it("parses string result with usage tags and agentId", () => {
    const result =
      "Agent output here.\nagentId: abc1DEF2\n<usage>total_tokens: 9876\ntool_uses: 5\nduration_ms: 30000</usage>";
    const { container } = renderWithResult(result);
    expect(container.textContent).toContain("30s");
    expect(container.textContent).toContain("9,876 tokens");
    expect(container.textContent).toContain("5 tools");
  });

  it("handles agentId with uppercase hex (case-insensitive regex fix)", () => {
    // agentId line has uppercase hex — old regex /[a-f0-9]+/ would fail on uppercase
    const result = "output\nagentId: ABCDEF01\n<usage>total_tokens: 100\ntool_uses: 1\nduration_ms: 1000</usage>";
    const { container } = renderWithResult(result);
    expect(container.textContent).toContain("1s");
    expect(container.textContent).toContain("100 tokens");
  });

  it("handles agentId at start of text (no preceding newline — textOutput newline fix)", async () => {
    const user = userEvent.setup();
    // agentId is first line — no preceding \n, old /\nagentId:/ search returned -1
    const result = "agentId: abc123\n<usage>total_tokens: 50\ntool_uses: 2\nduration_ms: 2000</usage>";
    const { container } = renderWithResult(result);
    expect(container.textContent).toContain("2s");
    // Expand and verify no phantom text output shown (agentId was the first line)
    const header = screen.getByRole("button");
    await user.click(header);
    // The pre element for text output should not appear (empty slice before agentId)
    const pres = container.querySelectorAll("pre");
    expect(pres).toHaveLength(0);
  });

  it("parses text array result (Array<{type: text, text: string}>)", () => {
    const result = [
      { type: "text", text: "Part one.\nagentId: aabbccdd\n<usage>total_tokens: 200\ntool_uses: 3\nduration_ms: 5000</usage>" },
    ];
    const { container } = renderWithResult(result);
    expect(container.textContent).toContain("5s");
    expect(container.textContent).toContain("200 tokens");
    expect(container.textContent).toContain("3 tools");
  });

  it("joins multiple text blocks in array result", () => {
    const result = [
      { type: "text", text: "Block one.\n" },
      { type: "text", text: "Block two.\nagentId: deadbeef\n<usage>total_tokens: 300\ntool_uses: 4\nduration_ms: 8000</usage>" },
    ];
    const { container } = renderWithResult(result);
    expect(container.textContent).toContain("8s");
    expect(container.textContent).toContain("300 tokens");
  });

  it("handles tool_use-only array — no stats row shown (statsAvailable: false)", () => {
    const result = [
      { type: "tool_use", id: "tu-1", name: "Bash", input: { command: "ls" } },
    ];
    const { container } = renderWithResult(result);
    // No stats row (middle dot separator)
    expect(container.textContent).not.toContain("\u00B7");
  });

  it("handles JSON object result with .text field — extracts text, not JSON.stringify", () => {
    const result = {
      type: "text",
      text: "Object result.\nagentId: 11223344\n<usage>total_tokens: 150\ntool_uses: 2\nduration_ms: 3000</usage>",
    };
    const { container } = renderWithResult(result);
    expect(container.textContent).toContain("3s");
    expect(container.textContent).toContain("150 tokens");
  });

  it("handles JSON object result without .text field — no stats row", () => {
    const result = { type: "tool_use", id: "tu-1", name: "Bash", input: {} };
    const { container } = renderWithResult(result);
    expect(container.textContent).not.toContain("\u00B7");
  });

  it("handles mixed array (text + tool_use blocks) — parses text blocks only", () => {
    const result = [
      { type: "tool_use", id: "tu-1", name: "Bash", input: {} },
      { type: "text", text: "Done.\nagentId: cafebabe\n<usage>total_tokens: 400\ntool_uses: 6\nduration_ms: 12000</usage>" },
    ];
    const { container } = renderWithResult(result);
    expect(container.textContent).toContain("12s");
    expect(container.textContent).toContain("400 tokens");
  });

  it("handles null result — no stats row", () => {
    const { container } = renderWithResult(null);
    expect(container.textContent).not.toContain("\u00B7");
  });

  it("handles undefined result — no stats row", () => {
    const { container } = renderWithResult(undefined);
    expect(container.textContent).not.toContain("\u00B7");
  });

  it("handles string result with no usage block — textOutput is whole text", async () => {
    const user = userEvent.setup();
    const result = "This is pure text output with no stats.";
    renderWithResult(result);
    // No stats row
    expect(screen.queryByText(/tokens/)).not.toBeInTheDocument();
    // But there should be a body (text output) — expand to verify
    const header = screen.getByRole("button");
    await user.click(header);
    expect(screen.getByText("This is pure text output with no stats.")).toBeInTheDocument();
  });

  it("parses singular tool count correctly (1 tool, not 1 tools)", () => {
    const result = "output\nagentId: abc\n<usage>total_tokens: 10\ntool_uses: 1\nduration_ms: 500</usage>";
    const { container } = renderWithResult(result);
    expect(container.textContent).toContain("1 tool");
    expect(container.textContent).not.toContain("1 tools");
  });
});

describe("TaskToolCallCard — extractTaskStats() structured stats field", () => {
  // Helper: render with a ToolCall that has a stats field
  function makeToolCallWithStats(stats: ToolCall["stats"], result?: unknown): ToolCall {
    return {
      id: "tc-1",
      name: "Task",
      arguments: { description: "test", subagent_type: "general-purpose" },
      result,
      stats,
    };
  }

  it("uses structured stats field when present (totalTokens, totalToolUses, durationMs)", () => {
    const tc = makeToolCallWithStats({
      totalTokens: 8888,
      totalToolUses: 15,
      durationMs: 45000,
      model: "claude-sonnet-4-6",
    });
    const { container } = render(<TaskToolCallCard toolCall={tc} />);

    // Stats come from structured field, not text parsing
    expect(container.textContent).toContain("8,888 tokens");
    expect(container.textContent).toContain("15 tools");
    expect(container.textContent).toContain("45s");
  });

  it("shows stats from structured field when result is undefined (no text to parse)", () => {
    const tc = makeToolCallWithStats({ totalTokens: 1234, totalToolUses: 3, durationMs: 5000 });
    const { container } = render(<TaskToolCallCard toolCall={tc} />);

    expect(container.textContent).toContain("1,234 tokens");
    expect(container.textContent).toContain("3 tools");
    expect(container.textContent).toContain("5s");
  });

  it("uses structured stats even when result text also has <usage> block (structured takes precedence)", () => {
    // result has different stats than the structured field — structured should win
    const result = "Agent output.\nagentId: abc123\n<usage>total_tokens: 999\ntool_uses: 1\nduration_ms: 1000</usage>";
    const tc = makeToolCallWithStats({ totalTokens: 5555, totalToolUses: 10, durationMs: 30000 }, result);
    const { container } = render(<TaskToolCallCard toolCall={tc} />);

    // Structured stats should be used (5555, not 999)
    expect(container.textContent).toContain("5,555 tokens");
    expect(container.textContent).toContain("10 tools");
    expect(container.textContent).toContain("30s");
    expect(container.textContent).not.toContain("999 tokens");
  });

  it("still shows textOutput from result when structured stats are present", async () => {
    const user = userEvent.setup();
    const result = "The agent completed the analysis.\nagentId: abc123\n<usage>total_tokens: 100\ntool_uses: 1\nduration_ms: 1000</usage>";
    const tc = makeToolCallWithStats({ totalTokens: 200, totalToolUses: 5, durationMs: 10000 }, result);
    render(<TaskToolCallCard toolCall={tc} />);

    // Stats from structured field
    expect(screen.getByText(/200 tokens/)).toBeInTheDocument();

    // Expand to see textOutput from result
    const header = screen.getByRole("button");
    await user.click(header);
    expect(screen.getByText("The agent completed the analysis.")).toBeInTheDocument();
  });

  it("partial stats: only some fields set — shows only available fields", () => {
    // durationMs only, no tokens or tools
    const tc = makeToolCallWithStats({ durationMs: 12000 });
    const { container } = render(<TaskToolCallCard toolCall={tc} />);

    expect(container.textContent).toContain("12s");
    expect(container.textContent).not.toContain("tokens");
    expect(container.textContent).not.toContain("tools");
  });

  it("partial stats: only totalTokens set", () => {
    const tc = makeToolCallWithStats({ totalTokens: 3000 });
    const { container } = render(<TaskToolCallCard toolCall={tc} />);

    expect(container.textContent).toContain("3,000 tokens");
    expect(container.textContent).not.toContain("tools");
    expect(container.textContent).not.toContain("ms");
  });

  it("partial stats: all values undefined — no stats row shown", () => {
    // stats object present but all fields undefined
    const tc = makeToolCallWithStats({});
    const { container } = render(<TaskToolCallCard toolCall={tc} />);
    // No stats row (middle dot separator)
    expect(container.textContent).not.toContain("\u00B7");
  });

  it("backward compat: no stats field → text-parsing fallback still works", () => {
    // Old DB row — stats field absent, result has <usage> block
    const result = "Agent output.\nagentId: abc123\n<usage>total_tokens: 7777\ntool_uses: 9\nduration_ms: 25000</usage>";
    const tc: ToolCall = {
      id: "tc-old",
      name: "Task",
      arguments: { description: "test" },
      result,
      // no stats field
    };
    const { container } = render(<TaskToolCallCard toolCall={tc} />);

    expect(container.textContent).toContain("7,777 tokens");
    expect(container.textContent).toContain("9 tools");
    expect(container.textContent).toContain("25s");
  });
});

describe("TaskToolCallCard — error state", () => {
  it("shows Failed badge when error is present", () => {
    const tc = makeAgentToolCall({ error: "Agent timed out" });
    render(<TaskToolCallCard toolCall={tc} />);
    expect(screen.getByText("Failed")).toBeInTheDocument();
  });

  it("shows error text in expanded view", async () => {
    const user = userEvent.setup();
    const tc = makeAgentToolCall({ error: "Connection refused to agent endpoint" });
    render(<TaskToolCallCard toolCall={tc} />);

    const header = screen.getByRole("button");
    await user.click(header);

    expect(screen.getByText("Connection refused to agent endpoint")).toBeInTheDocument();
  });
});
