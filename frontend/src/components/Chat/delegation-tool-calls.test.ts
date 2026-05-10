import { describe, expect, it } from "vitest";
import {
  extractDelegationMetadata,
  mergeDelegationContentBlocks,
  mergeDelegationToolCalls,
  normalizeDelegationTranscriptPayload,
} from "./delegation-tool-calls";
import { makeContentToolUse, makeToolCall } from "./__tests__/chatRenderFixtures";

function makeDelegationResult(payload: Record<string, unknown>) {
  return [{ type: "text", text: JSON.stringify(payload) }];
}

describe("delegation-tool-calls", () => {
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
});
