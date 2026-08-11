import { describe, expect, it } from "vitest";
import {
  extractDelegationMetadata,
  buildDelegationLifecycleTask,
  mergeDelegationContentBlocks,
  mergeDelegationToolCalls,
  normalizeDelegationTranscriptPayload,
  reconcileDelegationTaskMap,
  reconcileDelegationTaskMarkers,
} from "./delegation-tool-calls";
import type { StreamingContentBlock, StreamingTask } from "@/types/streaming-task";
import { makeContentToolUse, makeToolCall } from "./__tests__/chatRenderFixtures";

function makeDelegationResult(payload: Record<string, unknown>) {
  return [{ type: "text", text: JSON.stringify(payload) }];
}

describe("delegation-tool-calls", () => {
  function task(
    toolUseId: string,
    overrides: Partial<StreamingTask> = {},
  ): StreamingTask {
    return {
      toolUseId,
      toolName: "delegate_start",
      description: "",
      subagentType: "delegated",
      model: "unknown",
      status: "running",
      startedAt: 100,
      childToolCalls: [],
      ...overrides,
    };
  }

  it("coalesces provider and lifecycle aliases while preserving title, placement, and child calls", () => {
    const previous = new Map<string, StreamingTask>([
      ["provider-tool", task("provider-tool", {
        description: "Trace stale Claude MCP collision handling",
        startedAt: 100,
        childToolCalls: [{ id: "child-read", name: "Read", arguments: {} }],
      })],
      ["delegate-job:job-1", task("delegate-job:job-1", {
        description: "ralphx-general-explorer",
        delegatedJobId: "job-1",
        providerHarness: "codex",
        delegatedAgentRunId: "child-run-1",
        startedAt: 200,
        childToolCalls: [
          { id: "child-read", name: "Read", arguments: {}, result: "done" },
          { id: "child-shell", name: "Bash", arguments: {} },
        ],
      })],
    ]);

    const result = reconcileDelegationTaskMap(previous, {
      source: "provider",
      toolUseId: "provider-tool",
      jobId: "job-1",
      task: task("provider-tool", {
        description: "Trace stale Claude MCP collision handling",
        delegatedJobId: "job-1",
        startedAt: 300,
      }),
    });

    expect([...result.tasks.keys()]).toEqual(["provider-tool"]);
    expect(result.canonicalKey).toBe("provider-tool");
    expect(result.tasks.get("provider-tool")).toMatchObject({
      toolUseId: "provider-tool",
      description: "Trace stale Claude MCP collision handling",
      delegatedJobId: "job-1",
      providerHarness: "codex",
      delegatedAgentRunId: "child-run-1",
      startedAt: 100,
    });
    expect(result.tasks.get("provider-tool")?.childToolCalls.map((call) => call.id)).toEqual([
      "child-read",
      "child-shell",
    ]);
    expect(result.tasks.get("provider-tool")?.childToolCalls[0]?.result).toBe("done");
  });

  it("binds one unresolved provider placeholder but refuses to guess between two", () => {
    const one = reconcileDelegationTaskMap(
      new Map([["provider-one", task("provider-one", { description: "One" })]]),
      {
        source: "lifecycle-start",
        toolUseId: "delegate-job:job-1",
        jobId: "job-1",
        allowSingleUnresolvedPlaceholder: true,
        task: task("delegate-job:job-1", { delegatedJobId: "job-1" }),
      },
    );
    expect([...one.tasks.keys()]).toEqual(["provider-one"]);
    expect(one.tasks.get("provider-one")?.delegatedJobId).toBe("job-1");

    const two = reconcileDelegationTaskMap(
      new Map([
        ["provider-one", task("provider-one", { description: "One" })],
        ["provider-two", task("provider-two", { description: "Two" })],
      ]),
      {
        source: "lifecycle-start",
        toolUseId: "delegate-job:job-1",
        jobId: "job-1",
        allowSingleUnresolvedPlaceholder: true,
        task: task("delegate-job:job-1", { delegatedJobId: "job-1" }),
      },
    );
    expect([...two.tasks.keys()]).toEqual([
      "provider-one",
      "provider-two",
      "delegate-job:job-1",
    ]);
  });

  it("keeps terminal lifecycle evidence monotonic across reordered provider and recovery updates", () => {
    const terminal = task("provider-tool", {
      status: "failed",
      completedAt: 500,
      textOutput: "backend failure",
      inputTokens: 100,
      estimatedUsd: 0.12,
      delegatedJobId: "job-1",
      seq: 12,
    });

    const provider = reconcileDelegationTaskMap(new Map([["provider-tool", terminal]]), {
      source: "provider",
      toolUseId: "provider-tool",
      jobId: "job-1",
      seq: 10,
      task: task("provider-tool", {
        status: "completed",
        textOutput: "stale provider result",
        inputTokens: 50,
        estimatedUsd: 0.05,
        delegatedJobId: "job-1",
        seq: 10,
      }),
    });
    const recovered = reconcileDelegationTaskMap(provider.tasks, {
      source: "active-state",
      toolUseId: "delegate-job:job-1",
      jobId: "job-1",
      task: task("delegate-job:job-1", {
        status: "running",
        delegatedJobId: "job-1",
        inputTokens: 1,
      }),
    });

    expect(recovered.tasks.get("provider-tool")).toMatchObject({
      status: "failed",
      completedAt: 500,
      textOutput: "backend failure",
      inputTokens: 100,
      estimatedUsd: 0.12,
      seq: 12,
    });
  });

  it("prefers unsequenced lifecycle completion over a conflicting provider terminal result", () => {
    const completed = reconcileDelegationTaskMap(new Map(), {
      source: "lifecycle-complete",
      toolUseId: "delegate-job:job-1",
      jobId: "job-1",
      task: task("delegate-job:job-1", {
        status: "failed",
        delegatedJobId: "job-1",
        textOutput: "backend terminal output",
      }),
    });
    const provider = reconcileDelegationTaskMap(completed.tasks, {
      source: "provider",
      toolUseId: "provider-tool",
      providerToolUseId: "provider-tool",
      jobId: "job-1",
      task: task("provider-tool", {
        status: "completed",
        delegatedJobId: "job-1",
        textOutput: "conflicting provider output",
      }),
    });

    expect(provider.tasks.get("provider-tool")).toMatchObject({
      status: "failed",
      textOutput: "backend terminal output",
      delegationTerminalSource: "lifecycle-complete",
    });
  });

  it("replaces alias markers at their earliest position and removes duplicates", () => {
    const previous: StreamingContentBlock[] = [
      { type: "text", text: "before" },
      { type: "task", toolUseId: "provider-tool", seq: 2, receivedAt: 20 },
      { type: "text", text: "between" },
      { type: "task", toolUseId: "delegate-job:job-1", seq: 4, receivedAt: 40 },
    ];

    expect(reconcileDelegationTaskMarkers(previous, {
      canonicalKey: "provider-tool",
      aliasKeys: ["provider-tool", "delegate-job:job-1"],
    })).toEqual([
      { type: "text", text: "before" },
      { type: "task", toolUseId: "provider-tool", seq: 2, receivedAt: 20 },
      { type: "text", text: "between" },
    ]);
  });

  it("uses backend clocks for recovered lifecycle cards and clamps invalid terminal pairs", () => {
    const recovered = buildDelegationLifecycleTask({
      tool_use_id: "delegate-job:job-1",
      delegated_job_id: "job-1",
      status: "running",
      started_at: "2026-07-23T00:00:00Z",
      timestamp_provenance: "delegated_run",
    }, undefined, 999_999);
    const invalidTerminal = buildDelegationLifecycleTask({
      tool_use_id: "delegate-job:job-1",
      delegated_job_id: "job-1",
      status: "completed",
      started_at: "invalid",
      completed_at: "also-invalid",
    }, recovered, 1_000_000);

    expect(recovered).toMatchObject({
      startedAt: Date.parse("2026-07-23T00:00:00Z"),
      clockSource: "delegated-run",
    });
    expect(invalidTerminal.completedAt).toBe(1_000_000);
    expect(invalidTerminal.startedAt).toBe(recovered.startedAt);
  });

  it("folds delegate_wait into the original delegate_start tool call", () => {
    const startToolCall = makeToolCall("delegate_start", {
      id: "toolu-delegate-start",
      arguments: {
        agent_name: "ralphx-execution-reviewer",
        prompt: "Review the patch",
        harness: "codex",
        model: "gpt-5.4",
      },
      result: makeDelegationResult({
        job_id: "job-123",
        status: "running",
      }),
    });
    const waitToolCall = makeToolCall("delegate_wait", {
      id: "toolu-delegate-wait",
      arguments: {
        job_id: "job-123",
      },
      result: makeDelegationResult({
        job_id: "job-123",
        status: "completed",
        content: "Delegated review finished",
        delegated_status: {
          latest_run: {
            harness: "codex",
            provider_session_id: "thread-123",
            effective_model_id: "gpt-5.4",
            logical_effort: "high",
            input_tokens: 120,
            output_tokens: 45,
          },
        },
      }),
    });

    const mergedToolCalls = mergeDelegationToolCalls([startToolCall, waitToolCall]);
    expect(mergedToolCalls).toHaveLength(1);
    expect(mergedToolCalls[0]?.id).toBe("toolu-delegate-start");

    const mergedMetadata = extractDelegationMetadata(
      mergedToolCalls[0]?.arguments,
      mergedToolCalls[0]?.result,
    );
    expect(mergedMetadata.status).toBe("completed");
    expect(mergedMetadata.textOutput).toBe("Delegated review finished");
    expect(mergedMetadata.providerHarness).toBe("codex");
    expect(mergedMetadata.totalTokens).toBe(165);
  });

  it("uses the backend processed total for delegated Codex runs", () => {
    const metadata = extractDelegationMetadata(
      { job_id: "job-codex-usage" },
      makeDelegationResult({
        job_id: "job-codex-usage",
        status: "completed",
        total_tokens: 9_142_684,
        delegated_status: {
          latest_run: {
            harness: "codex",
            input_tokens: 9_116_803,
            output_tokens: 25_881,
            cache_read_tokens: 8_837_504,
            processed_tokens: 9_142_684,
          },
        },
      }),
    );

    expect(metadata.totalTokens).toBe(9_142_684);
  });

  it("folds namespaced delegate_wait into the original namespaced delegate_start tool call", () => {
    const startToolCall = makeToolCall("ralphx::delegate_start", {
      id: "toolu-delegate-start",
      arguments: {
        agent_name: "ralphx-plan-critic-completeness",
        prompt: "Review the plan",
      },
      result: makeDelegationResult({
        job_id: "job-456",
        status: "running",
      }),
    });
    const waitToolCall = makeToolCall("ralphx::delegate_wait", {
      id: "toolu-delegate-wait",
      arguments: {
        job_id: "job-456",
      },
      result: makeDelegationResult({
        job_id: "job-456",
        status: "completed",
        content: "Critic artifact published",
      }),
    });

    const mergedToolCalls = mergeDelegationToolCalls([startToolCall, waitToolCall]);
    expect(mergedToolCalls).toHaveLength(1);
    expect(mergedToolCalls[0]?.name).toBe("ralphx::delegate_start");
    expect(
      extractDelegationMetadata(
        mergedToolCalls[0]?.arguments,
        mergedToolCalls[0]?.result,
      ).textOutput,
    ).toBe("Critic artifact published");
  });

  it("promotes standalone namespaced delegate_wait into the delegated task-card contract", () => {
    const waitToolCall = makeToolCall("ralphx::delegate_wait", {
      id: "toolu-delegate-wait-only",
      arguments: {
        job_id: "job-789",
      },
      result: makeDelegationResult({
        job_id: "job-789",
        status: "completed",
        content: "Critic artifact published",
        agent_name: "ralphx-plan-critic-completeness",
      }),
    });

    const mergedToolCalls = mergeDelegationToolCalls([waitToolCall]);
    expect(mergedToolCalls).toHaveLength(1);
    expect(mergedToolCalls[0]?.name).toBe("ralphx::delegate_start");
    expect(
      extractDelegationMetadata(
        mergedToolCalls[0]?.arguments,
        mergedToolCalls[0]?.result,
      ).agentName,
    ).toBe("ralphx-plan-critic-completeness");
  });

  it("extracts the delegated agent name from standalone delegated_status session payloads", () => {
    const metadata = extractDelegationMetadata(
      { job_id: "job-standalone" },
      makeDelegationResult({
        job_id: "job-standalone",
        status: "completed",
        delegated_status: {
          session: {
            agent_name: "ralphx-execution-reviewer",
            status: "completed",
          },
          latest_run: {
            harness: "codex",
            provider_session_id: "thread-standalone",
          },
        },
      }),
    );

    expect(metadata.agentName).toBe("ralphx-execution-reviewer");
  });

  it("normalizes persisted delegation transcript payloads with one shared contract", () => {
    const startBlock = makeContentToolUse("delegate_start", {
      id: "toolu-delegate-start",
      arguments: {
        agent_name: "ralphx-execution-reviewer",
      },
      result: makeDelegationResult({
        job_id: "job-123",
        status: "running",
      }),
    });
    const waitBlock = makeContentToolUse("delegate_wait", {
      id: "toolu-delegate-wait",
      arguments: {
        job_id: "job-123",
      },
      result: makeDelegationResult({
        job_id: "job-123",
        status: "completed",
        content: "Delegated review finished",
      }),
    });
    const startToolCall = makeToolCall("delegate_start", {
      id: "toolu-delegate-start",
      arguments: {
        agent_name: "ralphx-execution-reviewer",
      },
      result: makeDelegationResult({
        job_id: "job-123",
        status: "running",
      }),
    });
    const waitToolCall = makeToolCall("delegate_wait", {
      id: "toolu-delegate-wait",
      arguments: {
        job_id: "job-123",
      },
      result: makeDelegationResult({
        job_id: "job-123",
        status: "completed",
        content: "Delegated review finished",
      }),
    });

    const normalized = normalizeDelegationTranscriptPayload({
      contentBlocks: [startBlock, waitBlock],
      toolCalls: [startToolCall, waitToolCall],
    });

    expect(normalized.contentBlocks).toHaveLength(1);
    expect(normalized.toolCalls).toHaveLength(1);

    const mergedBlockMetadata = extractDelegationMetadata(
      normalized.contentBlocks[0]?.arguments,
      normalized.contentBlocks[0]?.result,
    );
    const mergedToolMetadata = extractDelegationMetadata(
      normalized.toolCalls[0]?.arguments,
      normalized.toolCalls[0]?.result,
    );

    expect(mergedBlockMetadata.status).toBe("completed");
    expect(mergedBlockMetadata.textOutput).toBe("Delegated review finished");
    expect(mergedToolMetadata.status).toBe("completed");
    expect(mergedToolMetadata.textOutput).toBe("Delegated review finished");
  });

  it("keeps direct block-level merging behavior aligned with the shared transcript contract", () => {
    const startBlock = makeContentToolUse("delegate_start", {
      id: "toolu-delegate-start",
      arguments: {
        agent_name: "ralphx-execution-reviewer",
      },
      result: makeDelegationResult({
        job_id: "job-123",
        status: "running",
      }),
    });
    const waitBlock = makeContentToolUse("delegate_wait", {
      id: "toolu-delegate-wait",
      arguments: {
        job_id: "job-123",
      },
      result: makeDelegationResult({
        job_id: "job-123",
        status: "completed",
        content: "Delegated review finished",
      }),
    });

    const mergedBlocks = mergeDelegationContentBlocks([startBlock, waitBlock]);
    expect(mergedBlocks).toHaveLength(1);

    const metadata = extractDelegationMetadata(
      mergedBlocks[0]?.arguments,
      mergedBlocks[0]?.result,
    );
    expect(metadata.status).toBe("completed");
    expect(metadata.textOutput).toBe("Delegated review finished");
  });

  it("extracts error text from object-shaped MCP results with content arrays", () => {
    const metadata = extractDelegationMetadata(
      { agent_name: "ralphx-ideation-specialist-backend" },
      {
        content: [
          {
            type: "text",
            text: "ERROR: Unknown canonical caller agent 'ralphx-ideation'",
          },
        ],
      },
    );

    expect(metadata.textOutput).toBe(
      "ERROR: Unknown canonical caller agent 'ralphx-ideation'",
    );
  });

  it("maps an empty delegated completion to a typed cause and safe terminal details", () => {
    const metadata = extractDelegationMetadata(
      { job_id: "job-no-output" },
      {
        job_id: "job-no-output",
        status: "failed",
        error: "Codex exited without a response (context=project, code=Some(1), signal=None); diagnostics: Reading additional input from stdin...",
      },
    );

    expect(metadata.status).toBe("failed");
    expect(metadata.textOutput).toBe(
      "Delegate completed without a response\n\nExit code: 1",
    );
    expect(metadata.textOutput).not.toContain("stdin");
  });

  it("falls back to the delegated handoff message when top-level content is absent", () => {
    const metadata = extractDelegationMetadata(
      { agent_name: "ralphx-general-explorer" },
      {
        job_id: "job-999",
        status: "completed",
        delegated_status: {
          latest_run: {
            harness: "codex",
            logical_model: "gpt-5.4-mini",
          },
          recent_messages: [
            {
              role: "assistant",
              content: "Final delegated handoff summary.",
              created_at: "2026-04-16T10:00:00Z",
            },
          ],
        },
      },
    );

    expect(metadata.status).toBe("completed");
    expect(metadata.textOutput).toBe("Final delegated handoff summary.");
    expect(metadata.providerHarness).toBe("codex");
  });

  it("extracts the bound task summary without treating it as a local ledger task", () => {
    const metadata = extractDelegationMetadata(
      {
        agent_name: "ralphx-general-explorer",
        task_ref: "4",
      },
      {
        job_id: "job-assigned",
        status: "running",
        assignment: {
          task_number: 4,
          title: "Inspect restart recovery",
          task_state: "active",
          assignment_state: "active",
          delegate_agent_name: "ralphx-general-explorer",
        },
      },
    );

    expect(metadata.assignment).toEqual({
      taskNumber: 4,
      title: "Inspect restart recovery",
      taskState: "active",
      assignmentState: "active",
      delegateAgentName: "ralphx-general-explorer",
    });
  });
});
