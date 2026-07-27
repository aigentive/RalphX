import { describe, expect, it } from "vitest";
import type { ChatMessageData } from "./ChatMessageList";
import {
  foldDelegationTimelineMessages,
  projectDelegationTimelineMessages,
} from "./delegation-timeline";
import { buildDelegationLifecycleTask } from "./delegation-tool-calls";
import type { StreamingTask } from "@/types/streaming-task";

function lifecycleMessage(
  id: string,
  sequence: number,
  name: string,
  result: Record<string, unknown>,
): ChatMessageData {
  return {
    id,
    role: "assistant",
    content: name,
    createdAt: "2026-07-23T00:00:00Z",
    timelineSequence: sequence,
    contentBlocks: [{
      type: "tool_use",
      id: `${id}:tool`,
      name,
      arguments: { job_id: "job-1" },
      result,
    }],
  };
}

describe("foldDelegationTimelineMessages", () => {
  it("folds several controls without a loaded start into one earliest representative", () => {
    const folded = foldDelegationTimelineMessages([
      lifecycleMessage("wait", 20, "delegate_wait", { job_id: "job-1", status: "running" }),
      lifecycleMessage("terminal", 22, "delegate_terminal", {
        job_id: "job-1",
        status: "completed",
        content: "finished",
      }),
    ]);

    expect(folded).toHaveLength(1);
    expect(folded[0]).toMatchObject({ id: "wait" });
    expect(folded[0]?.contentBlocks?.[0]).toMatchObject({
      name: "delegate_start",
      result: { job_id: "job-1", status: "completed", content: "finished" },
    });
  });

  it("keeps different jobs distinct when their starts are not loaded", () => {
    const folded = foldDelegationTimelineMessages([
      lifecycleMessage("wait-one", 20, "delegate_wait", { job_id: "job-1", status: "running" }),
      {
        ...lifecycleMessage("wait-two", 21, "delegate_wait", { job_id: "job-2", status: "running" }),
        contentBlocks: [{
          type: "tool_use",
          id: "wait-two:tool",
          name: "delegate_wait",
          arguments: { job_id: "job-2" },
          result: { job_id: "job-2", status: "running" },
        }],
      },
    ]);

    expect(folded.map((message) => message.id)).toEqual(["wait-one", "wait-two"]);
  });
});

function liveTask(overrides: Partial<StreamingTask> = {}): StreamingTask {
  return {
    toolUseId: "provider-marker",
    toolName: "delegate_start",
    description: "Delegated reviewer",
    subagentType: "delegated",
    model: "gpt-5.6",
    status: "completed",
    startedAt: Date.parse("2026-07-23T00:00:00Z"),
    completedAt: Date.parse("2026-07-23T00:00:05Z"),
    totalDurationMs: 5_000,
    totalTokens: 123,
    textOutput: "Live terminal handoff",
    delegatedJobId: "job-1",
    delegatedConversationId: "child-conversation",
    delegatedAgentRunId: "child-run",
    childToolCalls: [],
    ...overrides,
  };
}

describe("projectDelegationTimelineMessages", () => {
  it("projects one typed failed settlement after the running delegate card", () => {
    const running = buildDelegationLifecycleTask({
      tool_use_id: "delegate-job:job-1",
      delegated_job_id: "job-1",
      status: "running",
    }, undefined, 100);
    const failed = buildDelegationLifecycleTask({
      tool_use_id: "delegate-job:job-1",
      delegated_job_id: "job-1",
      status: "failed",
      error: "Codex exited without a response (context=project, code=Some(1), signal=None); diagnostics: Reading additional input from stdin...",
      seq: 2,
    }, running, 200);
    const duplicateTerminal = buildDelegationLifecycleTask({
      tool_use_id: "delegate-job:job-1",
      delegated_job_id: "job-1",
      status: "failed",
      error: "generic duplicate terminal error",
      seq: 1,
    }, running, 300);

    const projection = projectDelegationTimelineMessages([
      lifecycleMessage("persisted-start", 20, "delegate_start", {
        job_id: "job-1",
        status: "running",
      }),
    ], new Map([
      ["delegate-job:job-1", failed],
      ["duplicate-terminal", duplicateTerminal],
    ]));

    expect(projection.messages).toHaveLength(1);
    expect(projection.messages[0]?.contentBlocks?.[0]).toMatchObject({
      result: expect.objectContaining({
        status: "failed",
        content: "Delegate completed without a response\n\nExit code: 1",
      }),
    });
    expect(projection.messages[0]?.contentBlocks?.[0]?.result).not.toEqual(
      expect.objectContaining({ content: expect.stringContaining("stdin") }),
    );
  });

  it("keeps the earliest persisted card while enriching it from live terminal evidence and suppressing aliases", () => {
    const projection = projectDelegationTimelineMessages([
      lifecycleMessage("persisted-start", 20, "delegate_start", {
        job_id: "job-1",
        status: "running",
      }),
      lifecycleMessage("persisted-wait", 21, "delegate_wait", {
        job_id: "job-1",
        status: "running",
      }),
    ], new Map([
      ["provider-marker", liveTask()],
      ["delegate-job:job-1", liveTask({ toolUseId: "delegate-job:job-1" })],
    ]));

    expect(projection.messages).toHaveLength(1);
    expect(projection.messages[0]).toMatchObject({ id: "persisted-start" });
    expect(projection.messages[0]?.contentBlocks?.[0]).toMatchObject({
      result: expect.objectContaining({
        status: "completed",
        content: "Live terminal handoff",
        total_duration_ms: 5_000,
        total_tokens: 123,
        delegated_conversation_id: "child-conversation",
        delegated_agent_run_id: "child-run",
      }),
    });
    expect(projection.liveAliases).toEqual(new Set([
      "provider-marker",
      "delegate-job:job-1",
    ]));
  });

  it("does not let active live state replace persisted terminal transcript evidence", () => {
    const projection = projectDelegationTimelineMessages([
      lifecycleMessage("persisted-terminal", 20, "delegate_start", {
        job_id: "job-1",
        status: "completed",
        content: "Persisted terminal handoff",
      }),
    ], new Map([["provider-marker", liveTask({ status: "running", textOutput: "Live draft" })]]));

    expect(projection.messages[0]?.contentBlocks?.[0]).toMatchObject({
      result: expect.objectContaining({
        status: "completed",
        content: "Persisted terminal handoff",
      }),
    });
  });

  it("does not relabel a local fallback clock as backend-owned", () => {
    const localClockTask = liveTask({
      status: "running",
      clockSource: "local-fallback",
    });
    delete localClockTask.completedAt;
    const projection = projectDelegationTimelineMessages([
      lifecycleMessage("persisted-start", 20, "delegate_start", {
        job_id: "job-1",
        status: "running",
      }),
    ], new Map([["provider-marker", localClockTask]]));

    expect(projection.messages[0]?.contentBlocks?.[0]).toMatchObject({
      result: expect.not.objectContaining({
        started_at: expect.anything(),
        timestamp_provenance: expect.anything(),
      }),
    });
  });

  it("matches a persisted start to its live task by tool-use identity before the job id is persisted", () => {
    const persistedStart: ChatMessageData = {
      id: "persisted-start",
      role: "assistant",
      content: "Delegated reviewer",
      createdAt: "2026-07-23T00:00:00Z",
      timelineSequence: 20,
      contentBlocks: [{
        type: "tool_use",
        id: "provider-marker",
        name: "delegate_start",
        arguments: { agent_name: "ralphx-general-explorer" },
        result: { status: "running" },
      }],
    };

    const projection = projectDelegationTimelineMessages(
      [persistedStart],
      new Map([["provider-marker", liveTask()]]),
    );

    expect(projection.messages).toHaveLength(1);
    expect(projection.messages[0]?.contentBlocks?.[0]).toMatchObject({
      result: expect.objectContaining({
        job_id: "job-1",
        status: "completed",
        content: "Live terminal handoff",
        delegated_conversation_id: "child-conversation",
      }),
    });
    expect(projection.liveAliases).toEqual(new Set(["provider-marker"]));
  });
});
