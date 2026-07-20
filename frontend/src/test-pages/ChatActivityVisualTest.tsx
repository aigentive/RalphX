import { useEffect } from "react";
import { MessageItem, type ContentBlockItem } from "@/components/Chat/MessageItem";
import { TooltipProvider } from "@/components/ui/tooltip";

const CHAT_CONTEXTS = [
  "Ideation",
  "Project",
  "Task",
  "Execution",
  "Review",
  "Merge",
  "Branch update",
  "Delegation",
];

function toolUse(
  id: string,
  name: string,
  args: Record<string, unknown>,
  options: Pick<ContentBlockItem, "result" | "diffContext"> = {},
): ContentBlockItem {
  return {
    type: "tool_use",
    id,
    name,
    arguments: args,
    ...options,
  };
}

function activityBlocks(provider: "claude" | "codex"): ContentBlockItem[] {
  const write = provider === "claude" ? "Write" : "write";
  const edit = provider === "claude" ? "Edit" : "edit";
  const delegateStart = provider === "claude"
    ? "mcp__ralphx__delegate_start"
    : "ralphx::delegate_start";
  const delegateWait = provider === "claude"
    ? "mcp__ralphx__delegate_wait"
    : "ralphx::delegate_wait";

  return [
    toolUse("create-component", write, { file_path: "src/ChatActivitySummary.tsx" }, {
      diffContext: {
        filePath: "src/ChatActivitySummary.tsx",
        oldFileExists: false,
      },
    }),
    toolUse("edit-message-list", edit, {
      file_path: "src/ChatMessageList.tsx",
      old_string: "Working…",
      new_string: "Agent called 5 tools",
    }, {
      diffContext: {
        filePath: "src/ChatMessageList.tsx",
        oldFileExists: true,
      },
    }),
    toolUse("edit-events", edit, {
      file_path: "src/useChatEvents.ts",
      old_string: "supportsSubagentTasks",
      new_string: "isDelegatedTask",
    }, {
      diffContext: {
        filePath: "src/useChatEvents.ts",
        oldFileExists: true,
      },
    }),
    toolUse("delegate-explorer", delegateStart, {
      agent_name: "ralphx-general-explorer",
      prompt: "Inspect the shared chat surfaces",
    }, {
      result: { job_id: "job-explorer", status: "running" },
    }),
    toolUse("wait-explorer", delegateWait, { job_id: "job-explorer" }, {
      result: {
        job_id: "job-explorer",
        status: "completed",
        content: "All chat contexts use the shared activity presentation.",
      },
    }),
    toolUse("delegate-worker", delegateStart, {
      agent_name: "ralphx-general-worker",
      prompt: "Verify provider parity",
    }, {
      result: { job_id: "job-worker", status: "running" },
    }),
    toolUse("wait-worker", delegateWait, { job_id: "job-worker" }, {
      result: {
        job_id: "job-worker",
        status: "completed",
        content: "Claude and Codex normalize to the same chat widgets.",
      },
    }),
  ];
}

function ProviderFixture({ provider }: { provider: "claude" | "codex" }) {
  const label = provider === "claude" ? "Claude" : "Codex";

  return (
    <section
      className="rounded-xl border p-5"
      data-testid={`chat-activity-${provider}`}
      style={{
        backgroundColor: "var(--bg-surface)",
        borderColor: "var(--border-default)",
      }}
    >
      <div className="mb-4 flex items-center justify-between gap-3">
        <div>
          <p className="text-xs font-medium" style={{ color: "var(--text-muted)" }}>
            Provider fixture
          </p>
          <h2 className="text-base font-semibold" style={{ color: "var(--text-primary)" }}>
            {label}
          </h2>
        </div>
        <span
          className="rounded-full px-2.5 py-1 text-xs font-medium"
          style={{
            backgroundColor: "var(--bg-elevated)",
            color: "var(--text-secondary)",
          }}
        >
          Shared chat UI
        </span>
      </div>

      <MessageItem
        role="assistant"
        content=""
        createdAt="2026-07-15T12:00:00.000Z"
        contentBlocks={activityBlocks(provider)}
        providerHarness={provider}
        logicalModel={provider === "claude" ? "claude-sonnet-4-6" : "gpt-5.5"}
        hideMeta
      />
    </section>
  );
}

export function ChatActivityVisualTestPage() {
  useEffect(() => {
    const params = new URLSearchParams(window.location.search);
    document.documentElement.setAttribute("data-theme", params.get("theme") ?? "dark");
  }, []);

  return (
    <TooltipProvider delayDuration={0}>
      <main
        className="min-h-screen p-6"
        data-testid="chat-activity-visual-test-page"
        style={{
          backgroundColor: "var(--app-content-bg)",
          color: "var(--text-primary)",
        }}
      >
        <div className="mx-auto max-w-5xl">
          <header className="mb-5">
            <p className="text-xs font-semibold uppercase tracking-[0.16em]" style={{ color: "var(--text-muted)" }}>
              Universal chat contract
            </p>
            <h1 className="mt-1 text-2xl font-semibold">Activity summaries and delegated tasks</h1>
            <div className="mt-3 flex flex-wrap gap-1.5" aria-label="Supported chat contexts">
              {CHAT_CONTEXTS.map((context) => (
                <span
                  key={context}
                  className="rounded-md px-2 py-1 text-xs"
                  style={{
                    backgroundColor: "var(--bg-elevated)",
                    color: "var(--text-secondary)",
                  }}
                >
                  {context}
                </span>
              ))}
            </div>
          </header>

          <div className="grid gap-4 lg:grid-cols-2">
            <ProviderFixture provider="claude" />
            <ProviderFixture provider="codex" />
          </div>
        </div>
      </main>
    </TooltipProvider>
  );
}
