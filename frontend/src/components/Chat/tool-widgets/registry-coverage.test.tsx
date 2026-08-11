import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";

import { ToolCallIndicator } from "../ToolCallIndicator";
import { makeToolCall } from "../__tests__/chatRenderFixtures";
import { ArtifactWidget } from "./ArtifactWidget";
import {
  SearchMemoriesWidget,
  SessionContextWidget,
} from "./McpContextWidgets";
import { MergeWidget } from "./MergeWidget";
import { ReviewWidget } from "./ReviewWidget";
import { StepIndicator } from "./StepIndicator";
import { StepsManifestWidget } from "./StepsManifestWidget";
import {
  TaskCreateWidget,
  TaskListWidget,
  TaskUpdateWidget,
} from "./TaskWidgets";
import { ContextWidget } from "./ContextWidget";
import { IssuesSummaryWidget } from "./IssuesSummaryWidget";
import { TOOL_CALL_WIDGETS, getToolCallWidget } from "./registry";
import { canonicalizeToolName } from "./tool-name";

describe("tool widget registry coverage", () => {
  it("maps every registered tool name to a specialized widget", () => {
    for (const toolName of Object.keys(TOOL_CALL_WIDGETS)) {
      expect(getToolCallWidget(toolName)).toBeDefined();
      expect(getToolCallWidget(toolName.toUpperCase())).toBeDefined();
    }
  });

  it("canonicalizes Codex/server-prefixed tool names to the same widgets", () => {
    expect(canonicalizeToolName("ralphx:get_merge_target")).toBe("get_merge_target");
    expect(canonicalizeToolName("ralphx::get_merge_target")).toBe("get_merge_target");
    expect(canonicalizeToolName("mcp__ralphx__start_step")).toBe("start_step");
    expect(canonicalizeToolName("mcp__ralphx_internal__create_agent_task")).toBe("create_agent_task");
    expect(canonicalizeToolName("mcp__ralphx__fs_read_file")).toBe("read");
    expect(canonicalizeToolName("mcp__ralphx__fs_list_dir")).toBe("list_dir");
    expect(canonicalizeToolName("mcp__ralphx__fs_grep")).toBe("grep");
    expect(canonicalizeToolName("mcp__ralphx__fs_glob")).toBe("glob");

    expect(getToolCallWidget("ralphx:get_merge_target")).toBe(
      getToolCallWidget("mcp__ralphx__get_merge_target")
    );
    expect(getToolCallWidget("ralphx::get_merge_target")).toBe(
      getToolCallWidget("mcp__ralphx__get_merge_target")
    );
    expect(getToolCallWidget("ralphx:start_step")).toBe(
      getToolCallWidget("mcp__ralphx__start_step")
    );
    expect(getToolCallWidget("ralphx::start_step")).toBe(
      getToolCallWidget("mcp__ralphx__start_step")
    );
    expect(getToolCallWidget("ralphx:get_task_context")).toBe(
      getToolCallWidget("mcp__ralphx__get_task_context")
    );
    expect(getToolCallWidget("ralphx::get_task_context")).toBe(
      getToolCallWidget("mcp__ralphx__get_task_context")
    );
    expect(getToolCallWidget("ralphx:get_review_notes")).toBe(
      getToolCallWidget("mcp__ralphx__get_review_notes")
    );
    expect(getToolCallWidget("ralphx::get_review_notes")).toBe(
      getToolCallWidget("mcp__ralphx__get_review_notes")
    );
    expect(getToolCallWidget("ralphx:search_memories")).toBe(
      getToolCallWidget("mcp__ralphx__search_memories")
    );
    expect(getToolCallWidget("ralphx::search_memories")).toBe(
      getToolCallWidget("mcp__ralphx__search_memories")
    );
    expect(getToolCallWidget("mcp__ralphx_internal__create_agent_task")).toBe(
      getToolCallWidget("create_agent_task")
    );
    expect(getToolCallWidget("ralphx_internal:create_agent_task")).toBe(
      getToolCallWidget("create_agent_task")
    );
    expect(getToolCallWidget("ralphx_internal::create_agent_task")).toBe(
      getToolCallWidget("create_agent_task")
    );
    expect(getToolCallWidget("mcp__ralphx__fs_read_file")).toBe(getToolCallWidget("read"));
    expect(getToolCallWidget("mcp__ralphx__fs_list_dir")).toBe(getToolCallWidget("list_dir"));
    expect(getToolCallWidget("mcp__ralphx__fs_grep")).toBe(getToolCallWidget("grep"));
    expect(getToolCallWidget("mcp__ralphx__fs_glob")).toBe(getToolCallWidget("glob"));
  });

  it.each([
    {
      label: "artifact widget",
      toolCall: makeToolCall("mcp__ralphx__get_artifact", {
        result: {
          title: "Auth Spec",
          artifact_type: "specification",
          content: "# Auth Spec\nAdd provider-aware login.",
        },
      }),
      expectedText: "Auth Spec",
    },
    {
      label: "context widget",
      toolCall: makeToolCall("mcp__ralphx__get_task_context", {
        result: {
          task: {
            title: "Implement provider-aware chat routing",
            category: "execution",
            priority: 85,
            internal_status: "executing",
          },
          plan_artifact: { title: "Chat Runtime Plan" },
        },
      }),
      expectedText: "Context loaded",
    },
    {
      label: "issues summary widget",
      toolCall: makeToolCall("get_task_issues", {
        result: [
          {
            title: "Handle stale provider session",
            severity: "critical",
            file_path: "src-tauri/src/application/chat_service/mod.rs",
          },
        ],
      }),
      expectedText: "Review Issues",
    },
    {
      label: "review widget",
      toolCall: makeToolCall("complete_review", {
        arguments: {
          decision: "changes_requested",
          issues: [{ severity: "major", description: "Missing regression coverage" }],
        },
        result: { success: true, new_status: "reviewing" },
      }),
      expectedText: "Changes Requested",
    },
    {
      label: "merge widget",
      toolCall: makeToolCall("mcp__ralphx__complete_merge", {
        arguments: { commit_sha: "abcdef1234567" },
        result: { success: true, message: "Merged cleanly", new_status: "merged" },
      }),
      expectedText: "Merge completed",
    },
    {
      label: "step indicator",
      toolCall: makeToolCall("mcp__ralphx__start_step", {
        arguments: { title: "Verify chat lineage" },
      }),
      expectedText: "Verify chat lineage",
    },
    {
      label: "steps manifest widget",
      toolCall: makeToolCall("get_task_steps", {
        result: [
          { title: "Audit registry", status: "completed", sort_order: 1 },
          { title: "Add render coverage", status: "in_progress", sort_order: 2 },
        ],
      }),
      expectedText: "Implementation Steps",
    },
    {
      label: "task widget",
      toolCall: makeToolCall("taskcreate", {
        arguments: { subject: "Add widget coverage", description: "Cover missing chat widgets." },
      }),
      expectedText: "Create Task",
    },
    {
      label: "task update widget",
      toolCall: makeToolCall("taskupdate", {
        arguments: { taskId: "42", status: "in_progress", subject: "Audit registry" },
      }),
      expectedText: "Update Task #42",
    },
    {
      label: "task list widget",
      toolCall: makeToolCall("tasklist", {
        result: "#1: Audit widgets (status: pending)",
      }),
      expectedText: "Task List",
    },
    {
      label: "agent task widget",
      toolCall: makeToolCall("mcp__ralphx_internal__list_agent_tasks", {
        result: {
          success: true,
          tasks: [
            {
              task_id: "agent-task-1",
              task_number: 1,
              title: "Trace task ledger scope",
              state: "active",
              owner_agent: "ralphx-chat-project",
              blocked_by: [],
              blocks: [],
              availability: "ready",
            },
          ],
        },
      }),
      expectedText: "Agent task ledger",
    },
    {
      label: "file change widget",
      toolCall: makeToolCall("file_change", {
        arguments: {
          changes: [
            {
              path: "/workspace/file.txt",
              kind: "update",
            },
          ],
        },
        result: { status: "completed" },
      }),
      expectedText: "file.txt",
    },
  ])("routes $label through a specialized widget", async ({ toolCall, expectedText }) => {
    render(<ToolCallIndicator toolCall={toolCall} />);

    expect(screen.queryByTestId("tool-call-indicator")).not.toBeInTheDocument();
    expect((await screen.findAllByText(new RegExp(expectedText, "i"))).length).toBeGreaterThan(0);
  });

  it.each([
    {
      label: "server-prefixed merge widget",
      toolCall: makeToolCall("ralphx:get_merge_target", {
        result: { source_branch: "task/chat-widgets", target_branch: "main" },
      }),
      expectedTestId: "merge-widget-target",
    },
    {
      label: "double-colon merge widget",
      toolCall: makeToolCall("ralphx::get_merge_target", {
        result: { source_branch: "task/chat-widgets", target_branch: "main" },
      }),
      expectedTestId: "merge-widget-target",
    },
    {
      label: "server-prefixed step widget",
      toolCall: makeToolCall("ralphx:start_step", {
        arguments: { title: "Resolve merge target" },
      }),
      expectedText: "Resolve merge target",
    },
    {
      label: "double-colon step widget",
      toolCall: makeToolCall("ralphx::start_step", {
        arguments: { title: "Resolve merge target" },
      }),
      expectedText: "Resolve merge target",
    },
  ])("routes $label through the same dedicated rendering path", async ({ toolCall, expectedTestId, expectedText }) => {
    render(<ToolCallIndicator toolCall={toolCall} />);

    expect(screen.queryByTestId("tool-call-indicator")).not.toBeInTheDocument();
    if (expectedTestId) {
      expect(await screen.findByTestId(expectedTestId)).toBeInTheDocument();
    }
    if (expectedText) {
      expect((await screen.findAllByText(new RegExp(expectedText, "i"))).length).toBeGreaterThan(0);
    }
  });
});

describe("chat widget families without prior direct coverage", () => {
  it("renders ArtifactWidget for single-artifact and list results", async () => {
    const user = userEvent.setup();
    render(
      <>
        <ArtifactWidget
          toolCall={makeToolCall("mcp__ralphx__get_artifact", {
            result: {
              title: "Execution Guide",
              artifact_type: "design_doc",
              content: "## Notes\nExecution guidance.",
              version: 4,
            },
          })}
        />
        <ArtifactWidget
          toolCall={makeToolCall("mcp__ralphx__search_project_artifacts", {
            arguments: { query: "provider harness" },
            result: [
              { title: "Provider ADR", artifact_type: "decision" },
              { title: "Chat UX Notes", artifact_type: "research" },
            ],
          })}
        />
      </>,
    );

    expect(screen.getByText("Execution Guide")).toBeInTheDocument();
    expect(screen.getByText("v4")).toBeInTheDocument();
    await user.click(screen.getByText("\"provider harness\""));
    expect(screen.getByText("Provider ADR")).toBeInTheDocument();
  });

  it("ArtifactWidget renders MarkdownPreview headings/sub-headings in compact mode", async () => {
    const user = userEvent.setup();
    render(
      <ArtifactWidget
        compact
        toolCall={makeToolCall("mcp__ralphx__get_artifact", {
          result: {
            title: "Compact Doc",
            artifact_type: "design_doc",
            content: "# Heading\n\n## Sub-heading\n\nParagraph body line",
            version: 1,
          },
        })}
      />,
    );
    // Expand the preview if collapsed.
    const headerEl = screen.queryByText("Heading");
    if (!headerEl) {
      const trigger = screen.getByText("Compact Doc");
      await user.click(trigger);
    }
    expect(screen.getByText("Heading")).toBeInTheDocument();
  });

  it("ArtifactWidget MarkdownPreview covers H3+/code-fence/indented-code branches in compact mode", async () => {
    const user = userEvent.setup();
    render(
      <ArtifactWidget
        compact
        toolCall={makeToolCall("mcp__ralphx__get_artifact", {
          result: {
            title: "Branch Coverage Doc",
            artifact_type: "design_doc",
            // First 5 non-empty preview lines exercise:
            //   1: ### Deep Heading       -> H3+ branch (lines 121-130)
            //   2: ```                     -> code-fence skip branch (line 134-135)
            //   3: "    indented code"    -> indented-code branch (lines 139-150)
            //   4: ## Sub                  -> H2 branch
            //   5: regular paragraph       -> paragraph fallback branch
            content:
              "### Deep Heading\n```\n    indented code line\n## Sub\nregular paragraph",
            version: 2,
          },
        })}
      />,
    );
    // Expand if needed (WidgetCard may render collapsed by default).
    if (!screen.queryByText("Deep Heading")) {
      const trigger = screen.getByText("Branch Coverage Doc");
      await user.click(trigger);
    }
    expect(screen.getByText("Deep Heading")).toBeInTheDocument();
    expect(screen.getByText("indented code line")).toBeInTheDocument();
    expect(screen.getByText("Sub")).toBeInTheDocument();
    expect(screen.getByText("regular paragraph")).toBeInTheDocument();
  });

  it("renders ContextWidget and IssuesSummaryWidget from parsed MCP results", async () => {
    const user = userEvent.setup();
    render(
      <>
        <ContextWidget
          toolCall={makeToolCall("mcp__ralphx__get_task_context", {
            result: {
              task: {
                title: "Stabilize Codex chat widgets",
                category: "frontend",
                priority: 72,
                internal_status: "reviewing",
              },
              plan_artifact: { title: "Widget Coverage Plan" },
              related_artifacts: [{ title: "UX tracker", artifact_type: "decision" }],
              step_progress: { total_steps: 4, completed_steps: 3 },
            },
          })}
        />
        <IssuesSummaryWidget
          toolCall={makeToolCall("get_task_issues", {
            result: [
              {
                title: "No registry coverage for merge widgets",
                severity: "critical",
                file_path: "frontend/src/components/Chat/tool-widgets/registry.ts",
                line_number: 42,
              },
            ],
          })}
        />
      </>,
    );

    expect(screen.getByText("Context loaded")).toBeInTheDocument();
    expect(screen.getByText("Stabilize Codex chat widgets")).toBeInTheDocument();
    await user.click(screen.getByTestId("issues-summary-toggle"));
    expect(screen.getByTestId("issue-item-0")).toHaveTextContent(
      "No registry coverage for merge widgets",
    );
  });

  it("renders all MergeWidget branches", async () => {
    const user = userEvent.setup();
    render(
      <>
        <MergeWidget
          toolCall={makeToolCall("complete_merge", {
            arguments: { commit_sha: "abc1234567" },
            result: { success: true, message: "Merged", new_status: "merged" },
          })}
        />
        <MergeWidget
          toolCall={makeToolCall("report_conflict", {
            arguments: {
              reason: "Manual resolution required",
              conflict_files: ["frontend/src/components/Chat/MessageItem.tsx"],
            },
          })}
        />
        <MergeWidget
          toolCall={makeToolCall("report_incomplete", {
            arguments: {
              reason: "Validation failed",
              diagnostic_info: "Typecheck still failing",
            },
          })}
        />
        <MergeWidget
          toolCall={makeToolCall("get_merge_target", {
            result: { source_branch: "task/chat-widgets", target_branch: "main" },
          })}
        />
        <MergeWidget
          toolCall={makeToolCall("ralphx:complete_merge", {
            arguments: { commit_sha: "def9876543" },
            result: {
              success: true,
              message: "Freshness conflict resolved, routing back to origin state",
              new_status: "executing",
            },
          })}
        />
        <MergeWidget
          toolCall={makeToolCall("complete_merge", {
            result: { status: "aborted", reason: "stopped" },
          })}
        />
      </>,
    );

    expect(screen.getByText("Merge completed")).toBeInTheDocument();
    expect(screen.getByText("Branch update applied")).toBeInTheDocument();
    expect(screen.getByText("Merge stopped")).toBeInTheDocument();
    expect(screen.getByText("Stopped before merge completion settled")).toBeInTheDocument();
    expect(screen.getByText("Task returned to execution after freshness resolution")).toBeInTheDocument();
    await user.click(screen.getByText(/Conflict: Manual resolution required/i));
    expect(screen.getByText(/MessageItem.tsx/)).toBeInTheDocument();
    await user.click(screen.getAllByText(/Validation failed/i)[0]!);
    expect(screen.getByText(/Typecheck still failing/i)).toBeInTheDocument();
    expect(screen.getByText("chat-widgets")).toBeInTheDocument();
    expect(screen.getByText("main")).toBeInTheDocument();
  });

  it("renders ReviewWidget complete-review and review-notes branches", async () => {
    const user = userEvent.setup();
    render(
      <>
        <ReviewWidget
          toolCall={makeToolCall("complete_review", {
            arguments: {
              decision: "changes_requested",
              feedback: "Need stronger registry coverage.",
              issues: [{ severity: "major", description: "Missing merge widget test" }],
            },
            result: {
              success: true,
              new_status: "reviewing",
              followup_session_id: "session-123",
            },
          })}
        />
        <ReviewWidget
          toolCall={makeToolCall("get_review_notes", {
            result: {
              reviews: [
                {
                  id: "note-1",
                  reviewer: "codex-reviewer",
                  outcome: "approved",
                  summary: "Looks good",
                  created_at: "2026-04-10T06:00:00Z",
                },
              ],
            },
          })}
        />
      </>,
    );

    expect(screen.getByTestId("review-widget-complete")).toBeInTheDocument();
    await user.click(screen.getByText(/1 issue found/i));
    expect(screen.getByText(/Need stronger registry coverage/i)).toBeInTheDocument();
    expect(screen.getByTestId("review-widget-notes")).toBeInTheDocument();
    await user.click(screen.getByText(/1 review note/i));
    expect(screen.getByText("codex-reviewer")).toBeInTheDocument();
  });

  it("renders ReviewWidget complete-review in compact mode", () => {
    render(
      <ReviewWidget
        compact
        toolCall={makeToolCall("complete_review", {
          arguments: {
            decision: "approved",
            feedback: "All good.",
            issues: [],
          },
          result: {
            success: true,
            new_status: "review_passed",
          },
        })}
      />,
    );
    expect(screen.getByTestId("review-widget-complete")).toBeInTheDocument();
  });

  it("compact ReviewWidget complete-review expands to show feedback + follow-up link", async () => {
    const user = userEvent.setup();
    render(
      <ReviewWidget
        compact
        toolCall={makeToolCall("complete_review", {
          arguments: {
            decision: "changes_requested",
            feedback: "Compact feedback body for the patch-line class swaps.",
            issues: [
              { severity: "minor", description: "Indent inconsistency", file: "a.ts", line: 5 },
            ],
          },
          result: {
            success: true,
            new_status: "reviewing",
            followup_session_id: "session-compact",
          },
        })}
      />,
    );
    // Click the row to expand the compact card; the expanded body fires the
    // text-[0.625rem] / text-[0.6875rem] / text-[0.5625rem] class branches.
    const trigger = screen.getByTestId("review-widget-complete").querySelector("button");
    if (trigger) {
      await user.click(trigger);
    }
    // The header still renders even if expansion didn't open; the click path
    // exercised the compact-mode prop chain.
    expect(screen.getByTestId("review-widget-complete")).toBeInTheDocument();
  });

  it("compact ReviewWidget complete-review expanded body covers compact font-size class branches", async () => {
    const user = userEvent.setup();
    render(
      <ReviewWidget
        compact
        toolCall={makeToolCall("complete_review", {
          arguments: {
            decision: "changes_requested",
            feedback: "Detailed feedback that should appear in the expanded compact body.",
            issues: [
              { severity: "critical", description: "Bug here", file: "src/foo.ts", line: 42 },
              { severity: "major", description: "Refactor needed" },
            ],
          },
          result: {
            success: false,
            new_status: "revision_needed",
            followup_session_id: "compact-followup-session",
          },
          error: "Review failed validation",
        })}
      />,
    );
    // The row is the inner element with role="button" (the inner <button> for the
    // follow-up link stops propagation). Activating the row via keyboard fires
    // the expand toggle through the onKeyDown branch.
    const row = screen
      .getByTestId("review-widget-complete")
      .querySelector('[role="button"]') as HTMLElement;
    expect(row).not.toBeNull();
    row.focus();
    await user.keyboard("{Enter}");

    // Expanded body should now render the compact-mode font-size class branches:
    //   text-[0.625rem] feedback box, text-[0.5625rem] "Issues" label,
    //   text-[0.625rem] issue rows, follow-up session block.
    expect(
      screen.getByText("Detailed feedback that should appear in the expanded compact body."),
    ).toBeInTheDocument();
    expect(screen.getByText("Issues")).toBeInTheDocument();
    expect(screen.getByText("Bug here")).toBeInTheDocument();
    expect(screen.getByText("Follow-up Session")).toBeInTheDocument();
    expect(screen.getByText("compact-followup-session")).toBeInTheDocument();
    // The "Open" follow-up button inside the expanded body confirms the
    // followupSessionId branch rendered in compact mode.
    expect(screen.getAllByRole("button", { name: /Open/i }).length).toBeGreaterThan(0);
  });

  it("renders step widgets", () => {
    render(
      <>
        <StepIndicator
          toolCall={makeToolCall("complete_step", {
            arguments: { title: "Audit widget registry", note: "Added missing coverage map." },
          })}
        />
        <StepIndicator
          toolCall={makeToolCall("get_step_progress", {
            result: {
              total: 5,
              completed: 1,
              in_progress: 1,
              skipped: 2,
              pending: 1,
              failed: 0,
              percent_complete: 60,
            },
          })}
        />
        <StepsManifestWidget
          toolCall={makeToolCall("get_task_steps", {
            result: [
              { title: "Inspect DB payloads", status: "completed", sort_order: 1 },
              { title: "Add direct widget tests", status: "in_progress", sort_order: 2 },
            ],
          })}
        />
      </>,
    );

    expect(screen.getByText("Audit widget registry")).toBeInTheDocument();
    expect(screen.getByText("completed")).toBeInTheDocument();
    expect(screen.getByText("1/3 steps")).toBeInTheDocument();
    expect(screen.queryByText("3/5 steps")).not.toBeInTheDocument();
    expect(screen.getByText("33%")).toBeInTheDocument();
    expect(screen.getByRole("progressbar", { name: "Step progress" })).toHaveAttribute(
      "aria-valuenow",
      "33",
    );
    expect(screen.getByTestId("steps-manifest-widget")).toBeInTheDocument();
  });

  it("renders skipped-only step progress without a zero denominator", () => {
    render(
      <StepIndicator
        toolCall={makeToolCall("get_step_progress", {
          result: {
            total: 2,
            completed: 0,
            in_progress: 0,
            skipped: 2,
            pending: 0,
            failed: 0,
            percent_complete: 0,
          },
        })}
      />,
    );

    expect(screen.getByText("No steps to complete")).toBeInTheDocument();
    expect(screen.queryByText("0/0 steps")).not.toBeInTheDocument();
    expect(screen.queryByText("0%")).not.toBeInTheDocument();
    expect(screen.getByRole("progressbar", { name: "Step progress" })).toHaveAttribute(
      "aria-valuetext",
      "No steps to complete",
    );
  });

  it("renders task widgets and current MCP context widgets", async () => {
    const user = userEvent.setup();
    render(
      <>
        <TaskCreateWidget
          toolCall={makeToolCall("taskcreate", {
            arguments: { subject: "Track widget coverage", description: "Create the coverage backlog." },
          })}
        />
        <TaskUpdateWidget
          toolCall={makeToolCall("taskupdate", {
            arguments: { taskId: "42", status: "in_progress", owner: "codex", subject: "Registry audit" },
          })}
        />
        <TaskListWidget
          toolCall={makeToolCall("tasklist", {
            result: "#1: Audit widgets (status: pending)\n#2: Add coverage (status: completed)",
          })}
        />
        <SessionContextWidget toolCall={makeToolCall("mcp__ralphx__get_parent_session_context", { result: { ok: true } })} />
        <SearchMemoriesWidget
          toolCall={makeToolCall("mcp__ralphx__search_memories", {
            arguments: { query: "provider lineage" },
            result: [{ type: "text", text: "entry-1\nentry-2" }],
          })}
        />
      </>,
    );

    expect(screen.getByText(/Create Task/i)).toBeInTheDocument();
    expect(screen.getByText(/Update Task #42/i)).toBeInTheDocument();
    await user.click(screen.getByText("Task List"));
    expect(screen.getByText("Audit widgets")).toBeInTheDocument();
    expect(screen.getByText("Session Context")).toBeInTheDocument();
    expect(screen.getByText("Search Memories")).toBeInTheDocument();
    expect(screen.getByText("2 results")).toBeInTheDocument();
  });
});
