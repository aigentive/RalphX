import type {
  ChatMessageResponse,
  ConversationActiveStateResponse,
  ConversationTimelinePageResponse,
} from "@/api/chat";
import type { ChatConversation } from "@/types/chat-conversation";
import type { ContentBlockItem } from "../MessageItem";
import {
  makeContentText,
  makeContentToolUse,
  makeEditContentToolUse,
} from "./chatRenderFixtures";

/**
 * One authoritative replay chronology. Timeline, recovery state, and event
 * replay deliberately project from this list so a test cannot substitute
 * unrelated persisted prefixes for a live/recovery transition.
 */
export const REPLAY_STEP_IDS = [
  "turn-1-user",
  "turn-1-opening-text",
  "turn-1-read-result",
  "turn-1-bridge-text",
  "turn-1-delegate-persisted",
  "turn-1-closing-text",
  "turn-2-user",
  "turn-2-text-delta",
  "turn-2-grep-start",
  "turn-2-grep-result",
  "turn-2-second-text-delta",
  "turn-2-delegate-started",
  "turn-2-delegate-completed",
  "turn-2-edit-started",
  "turn-2-final-text",
  "turn-2-finalized",
] as const;

export type ReplayStepId = (typeof REPLAY_STEP_IDS)[number];

export type ReplayLiveEvent = {
  name: string;
  payload: Record<string, unknown>;
};

type TimelineStatus = "streaming" | "finalized";

type PersistedTimelineEntry = {
  id: string;
  parentMessageId: string;
  role: "assistant" | "user";
  content: string;
  contentBlocks: ContentBlockItem[];
  status: TimelineStatus;
  /** A streaming timeline block becomes durable only at this later step. */
  finalizesAt?: ReplayStepId;
};

type ActiveStateSnapshot = Pick<
  ConversationActiveStateResponse,
  "is_active" | "tool_calls" | "streaming_tasks" | "partial_text"
>;

/** Test-only forward-compatible shape for the Phase 0 delta replay contract. */
type DeltaActiveStateSnapshot = ActiveStateSnapshot & {
  partial_text_segments?: string[];
};

type ReplayStep = {
  id: ReplayStepId;
  persisted?: PersistedTimelineEntry[];
  activeState?: ActiveStateSnapshot;
  events?: ReplayLiveEvent[];
};

export type ReplayConversationFixture = {
  conversation: ChatConversation;
  /** The backend view at a real persistence checkpoint. */
  timelinePage: (
    throughStep: ReplayStepId,
    options?: { limit?: number; beforeSequence?: number | null },
  ) => ConversationTimelinePageResponse;
  /** The backend's active-tail supplement at that exact checkpoint. */
  activeState: (throughStep: ReplayStepId) => ConversationActiveStateResponse;
  /** Events emitted strictly after `fromStep` and through `toStep`, in order. */
  events: (fromStep: ReplayStepId, toStep: ReplayStepId) => ReplayLiveEvent[];
};

const CONVERSATION_ID = "conversation-replay";
const PROJECT_ID = "project-replay";
const RUN_ID = "run-replay";
const BASE_TIME = "2026-04-10T07:00:00.000Z";
const EDIT_FILE_PATH = "frontend/src/replay/recovery.ts";

function timestampFor(sequence: number): string {
  return new Date(Date.parse(BASE_TIME) + sequence * 1_000).toISOString();
}

function messageFromEntry(
  entry: PersistedTimelineEntry,
  sequence: number,
  status: TimelineStatus,
): ChatMessageResponse {
  const toolCalls = entry.contentBlocks.flatMap((block) => {
    if (block.type !== "tool_use" || !block.name) return [];
    return [{
      id: block.id ?? `replay-tool-${sequence}`,
      name: block.name,
      arguments: block.arguments ?? {},
      ...(block.result !== undefined ? { result: block.result } : {}),
      ...(block.diffContext !== undefined ? { diffContext: block.diffContext } : {}),
    }];
  });
  const createdAt = timestampFor(sequence);
  return {
    id: `timeline-${entry.id}`,
    sessionId: null,
    projectId: PROJECT_ID,
    taskId: null,
    role: entry.role,
    content: entry.content,
    metadata: null,
    parentMessageId: entry.parentMessageId,
    conversationId: CONVERSATION_ID,
    toolCalls: toolCalls.length > 0 ? toolCalls : null,
    contentBlocks: entry.contentBlocks,
    sender: null,
    providerHarness: "codex",
    providerSessionId: "provider-session-replay",
    createdAt,
    timelineStatus: status,
    timelineKind: entry.contentBlocks[0]?.type === "tool_use" ? "tool" : "text",
    timelineSequence: sequence,
    runId: RUN_ID,
  };
}

function replayEvent(name: string, payload: Record<string, unknown>): ReplayLiveEvent {
  return { name, payload };
}

const TURN_ONE_DELEGATE_ID = "replay-delegate-turn-1";
const TURN_TWO_DELEGATE_ID = "replay-delegate-turn-2";
const TURN_TWO_GREP_ID = "replay-grep-turn-2";
const TURN_TWO_EDIT_ID = "replay-edit-turn-2";
const TURN_ONE_USER_MESSAGE_ID = "replay-turn-1-user-message";
const TURN_ONE_ASSISTANT_MESSAGE_ID = "replay-turn-1-assistant-message";
const TURN_TWO_USER_MESSAGE_ID = "replay-turn-2-user-message";
const TURN_TWO_ASSISTANT_MESSAGE_ID = "replay-turn-2-assistant-message";
const TURN_TWO_OPENING_TEXT = "Live turn two begins before remount. ";
const TURN_TWO_BRIDGE_TEXT = "The live Grep result stays before the delegated child. ";
const TURN_TWO_FINAL_TEXT = "Live turn two is ready to finalize.";

const STEPS: readonly ReplayStep[] = [
  {
    id: "turn-1-user",
    persisted: [{
      id: "replay-turn-1-user",
      parentMessageId: TURN_ONE_USER_MESSAGE_ID,
      role: "user",
      content: "Replay the full two-turn recovery path.",
      contentBlocks: [makeContentText("Replay the full two-turn recovery path.")],
      status: "finalized",
    }],
  },
  {
    id: "turn-1-opening-text",
    persisted: [{
      id: "replay-turn-1-opening",
      parentMessageId: TURN_ONE_ASSISTANT_MESSAGE_ID,
      role: "assistant",
      content: "Turn one starts from durable history.",
      contentBlocks: [makeContentText("Turn one starts from durable history.")],
      status: "finalized",
    }],
  },
  {
    id: "turn-1-read-result",
    persisted: [{
      id: "replay-turn-1-read",
      parentMessageId: TURN_ONE_ASSISTANT_MESSAGE_ID,
      role: "assistant",
      content: "",
      contentBlocks: [makeContentToolUse("Read", {
        id: "replay-read-turn-1",
        arguments: { file_path: "frontend/src/components/Chat/ChatMessageList.tsx" },
        result: "timeline projection found",
      })],
      status: "finalized",
    }],
  },
  {
    id: "turn-1-bridge-text",
    persisted: [{
      id: "replay-turn-1-bridge",
      parentMessageId: TURN_ONE_ASSISTANT_MESSAGE_ID,
      role: "assistant",
      content: "The first turn keeps its tool result in place.",
      contentBlocks: [makeContentText("The first turn keeps its tool result in place.")],
      status: "finalized",
    }],
  },
  {
    id: "turn-1-delegate-persisted",
    persisted: [{
      id: "replay-turn-1-delegate",
      parentMessageId: TURN_ONE_ASSISTANT_MESSAGE_ID,
      role: "assistant",
      content: "",
      contentBlocks: [makeContentToolUse("delegate_start", {
        id: TURN_ONE_DELEGATE_ID,
        arguments: {
          title: "Replay delegated child replay-delegate-turn-1",
          prompt: "Inspect durable turn-one replay history.",
        },
        result: {
          job_id: "replay-delegate-job-turn-1",
          status: "completed",
          title: "Replay delegated child replay-delegate-turn-1",
          delegated_session_id: "session-replay-child-turn-1",
          delegated_conversation_id: "conversation-replay-child-turn-1",
          delegated_agent_run_id: "run-replay-child-turn-1",
        },
      })],
      status: "finalized",
    }],
  },
  {
    id: "turn-1-closing-text",
    persisted: [{
      id: "replay-turn-1-closing",
      parentMessageId: TURN_ONE_ASSISTANT_MESSAGE_ID,
      role: "assistant",
      content: "Turn one is finalized before recovery begins.",
      contentBlocks: [makeContentText("Turn one is finalized before recovery begins.")],
      status: "finalized",
    }],
  },
  {
    id: "turn-2-user",
    persisted: [{
      id: "replay-turn-2-user",
      parentMessageId: TURN_TWO_USER_MESSAGE_ID,
      role: "user",
      content: "Now prove live recovery without losing turn one.",
      contentBlocks: [makeContentText("Now prove live recovery without losing turn one.")],
      status: "finalized",
    }],
  },
  {
    id: "turn-2-text-delta",
    persisted: [{
      id: "replay-turn-2-opening",
      parentMessageId: TURN_TWO_ASSISTANT_MESSAGE_ID,
      role: "assistant",
      content: TURN_TWO_OPENING_TEXT,
      contentBlocks: [makeContentText(TURN_TWO_OPENING_TEXT)],
      status: "streaming",
      finalizesAt: "turn-2-finalized",
    }],
    activeState: {
      is_active: true,
      tool_calls: [],
      streaming_tasks: [],
      partial_text: TURN_TWO_OPENING_TEXT,
    },
    events: [replayEvent("agent:chunk", {
      conversation_id: CONVERSATION_ID,
      context_id: PROJECT_ID,
      context_type: "project",
      run_id: RUN_ID,
      seq: 8,
      append_to_previous: false,
      text: TURN_TWO_OPENING_TEXT,
    })],
  },
  {
    id: "turn-2-grep-start",
    events: [replayEvent("agent:tool_call", {
      conversation_id: CONVERSATION_ID,
      context_id: PROJECT_ID,
      context_type: "project",
      run_id: RUN_ID,
      seq: 9,
      tool_name: "Grep",
      tool_id: TURN_TWO_GREP_ID,
      arguments: { pattern: "hydrate", path: "frontend/src/hooks" },
    })],
  },
  {
    id: "turn-2-grep-result",
    persisted: [{
      id: "replay-turn-2-grep",
      parentMessageId: TURN_TWO_ASSISTANT_MESSAGE_ID,
      role: "assistant",
      content: "",
      contentBlocks: [makeContentToolUse("Grep", {
        id: TURN_TWO_GREP_ID,
        arguments: { pattern: "hydrate", path: "frontend/src/hooks" },
        result: "frontend/src/hooks/useChatRecovery.ts",
      })],
      status: "streaming",
      finalizesAt: "turn-2-finalized",
    }],
    events: [replayEvent("agent:tool_call", {
      conversation_id: CONVERSATION_ID,
      context_id: PROJECT_ID,
      context_type: "project",
      run_id: RUN_ID,
      seq: 10,
      tool_name: `result:${TURN_TWO_GREP_ID}`,
      arguments: {},
      result: "frontend/src/hooks/useChatRecovery.ts",
    })],
  },
  {
    id: "turn-2-second-text-delta",
    persisted: [{
      id: "replay-turn-2-bridge",
      parentMessageId: TURN_TWO_ASSISTANT_MESSAGE_ID,
      role: "assistant",
      content: TURN_TWO_BRIDGE_TEXT,
      contentBlocks: [makeContentText(TURN_TWO_BRIDGE_TEXT)],
      status: "streaming",
      finalizesAt: "turn-2-finalized",
    }],
    events: [replayEvent("agent:chunk", {
      conversation_id: CONVERSATION_ID,
      context_id: PROJECT_ID,
      context_type: "project",
      run_id: RUN_ID,
      seq: 11,
      append_to_previous: false,
      text: TURN_TWO_BRIDGE_TEXT,
    })],
  },
  {
    id: "turn-2-delegate-started",
    events: [
      replayEvent("agent:tool_call", {
        conversation_id: CONVERSATION_ID,
        context_id: PROJECT_ID,
        context_type: "project",
        run_id: RUN_ID,
        seq: 12,
        tool_name: "delegate_start",
        tool_id: TURN_TWO_DELEGATE_ID,
        arguments: {
          title: "Replay delegated child replay-delegate-turn-2",
          prompt: "Inspect the live remount boundary.",
          job_id: "replay-delegate-job-turn-2",
        },
      }),
      replayEvent("agent:task_started", {
        conversation_id: CONVERSATION_ID,
        context_id: PROJECT_ID,
        context_type: "project",
        run_id: RUN_ID,
        seq: 13,
        tool_use_id: TURN_TWO_DELEGATE_ID,
        tool_name: "delegate_start",
        subagent_type: "delegated",
        description: "Replay delegated child replay-delegate-turn-2",
        delegated_job_id: "replay-delegate-job-turn-2",
        delegated_session_id: "session-replay-child-turn-2",
        delegated_conversation_id: "conversation-replay-child-turn-2",
        delegated_agent_run_id: "run-replay-child-turn-2",
      }),
    ],
  },
  {
    id: "turn-2-delegate-completed",
    persisted: [{
      id: "replay-turn-2-delegate",
      parentMessageId: TURN_TWO_ASSISTANT_MESSAGE_ID,
      role: "assistant",
      content: "",
      contentBlocks: [makeContentToolUse("delegate_start", {
        id: TURN_TWO_DELEGATE_ID,
        arguments: {
          title: "Replay delegated child replay-delegate-turn-2",
          prompt: "Inspect the live remount boundary.",
        },
        result: {
          job_id: "replay-delegate-job-turn-2",
          status: "completed",
          title: "Replay delegated child replay-delegate-turn-2",
          delegated_session_id: "session-replay-child-turn-2",
          delegated_conversation_id: "conversation-replay-child-turn-2",
          delegated_agent_run_id: "run-replay-child-turn-2",
        },
      })],
      status: "streaming",
      finalizesAt: "turn-2-finalized",
    }],
    events: [replayEvent("agent:task_completed", {
      conversation_id: CONVERSATION_ID,
      context_id: PROJECT_ID,
      context_type: "project",
      run_id: RUN_ID,
      seq: 14,
      tool_use_id: TURN_TWO_DELEGATE_ID,
      tool_name: "delegate_start",
      subagent_type: "delegated",
      status: "completed",
      delegated_job_id: "replay-delegate-job-turn-2",
      delegated_session_id: "session-replay-child-turn-2",
      delegated_conversation_id: "conversation-replay-child-turn-2",
      delegated_agent_run_id: "run-replay-child-turn-2",
      total_duration_ms: 321,
      total_tokens: 34,
      total_tool_use_count: 2,
    })],
  },
  {
    id: "turn-2-edit-started",
    activeState: {
      is_active: true,
      tool_calls: [{
        id: TURN_TWO_GREP_ID,
        name: "Grep",
        arguments: { pattern: "hydrate", path: "frontend/src/hooks" },
        result: "frontend/src/hooks/useChatRecovery.ts",
      }, {
        id: TURN_TWO_DELEGATE_ID,
        name: "delegate_start",
        arguments: {
          title: "Replay delegated child replay-delegate-turn-2",
          prompt: "Inspect the live remount boundary.",
        },
        result: {
          job_id: "replay-delegate-job-turn-2",
          status: "completed",
          title: "Replay delegated child replay-delegate-turn-2",
        },
      }, {
        id: TURN_TWO_EDIT_ID,
        name: "Edit",
        arguments: {
          file_path: EDIT_FILE_PATH,
          old_string: "return restored;",
          new_string: "return replayed;",
        },
        diff_context: {
          file_path: EDIT_FILE_PATH,
          old_content: "export const restore = () => {\n  return restored;\n};\n",
          old_file_exists: true,
        },
      }],
      streaming_tasks: [{
        tool_use_id: TURN_TWO_DELEGATE_ID,
        description: "Replay delegated child replay-delegate-turn-2",
        subagent_type: "delegated",
        model: "gpt-5.6-terra",
        status: "completed",
        delegated_job_id: "replay-delegate-job-turn-2",
        delegated_session_id: "session-replay-child-turn-2",
        delegated_conversation_id: "conversation-replay-child-turn-2",
        delegated_agent_run_id: "run-replay-child-turn-2",
        total_tokens: 34,
        total_tool_uses: 2,
        duration_ms: 321,
        seq: 14,
      }],
      partial_text: TURN_TWO_OPENING_TEXT + TURN_TWO_BRIDGE_TEXT,
    },
    events: [replayEvent("agent:tool_call", {
      conversation_id: CONVERSATION_ID,
      context_id: PROJECT_ID,
      context_type: "project",
      run_id: RUN_ID,
      seq: 15,
      tool_name: "Edit",
      tool_id: TURN_TWO_EDIT_ID,
      arguments: {
        file_path: EDIT_FILE_PATH,
        old_string: "return restored;",
        new_string: "return replayed;",
      },
      diff_context: {
        file_path: EDIT_FILE_PATH,
        old_content: "export const restore = () => {\n  return restored;\n};\n",
        old_file_exists: true,
      },
    })],
  },
  {
    id: "turn-2-final-text",
    activeState: {
      is_active: true,
      tool_calls: [{
        id: TURN_TWO_GREP_ID,
        name: "Grep",
        arguments: { pattern: "hydrate", path: "frontend/src/hooks" },
        result: "frontend/src/hooks/useChatRecovery.ts",
      }, {
        id: TURN_TWO_DELEGATE_ID,
        name: "delegate_start",
        arguments: {
          title: "Replay delegated child replay-delegate-turn-2",
          prompt: "Inspect the live remount boundary.",
        },
        result: {
          job_id: "replay-delegate-job-turn-2",
          status: "completed",
          title: "Replay delegated child replay-delegate-turn-2",
        },
      }, {
        id: TURN_TWO_EDIT_ID,
        name: "Edit",
        arguments: {
          file_path: EDIT_FILE_PATH,
          old_string: "return restored;",
          new_string: "return replayed;",
        },
        diff_context: {
          file_path: EDIT_FILE_PATH,
          old_content: "export const restore = () => {\n  return restored;\n};\n",
          old_file_exists: true,
        },
      }],
      streaming_tasks: [{
        tool_use_id: TURN_TWO_DELEGATE_ID,
        description: "Replay delegated child replay-delegate-turn-2",
        subagent_type: "delegated",
        model: "gpt-5.6-terra",
        status: "completed",
        delegated_job_id: "replay-delegate-job-turn-2",
        delegated_session_id: "session-replay-child-turn-2",
        delegated_conversation_id: "conversation-replay-child-turn-2",
        delegated_agent_run_id: "run-replay-child-turn-2",
        total_tokens: 34,
        total_tool_uses: 2,
        duration_ms: 321,
        seq: 14,
      }],
      partial_text: TURN_TWO_OPENING_TEXT + TURN_TWO_BRIDGE_TEXT + TURN_TWO_FINAL_TEXT,
    },
    events: [replayEvent("agent:chunk", {
      conversation_id: CONVERSATION_ID,
      context_id: PROJECT_ID,
      context_type: "project",
      run_id: RUN_ID,
      seq: 16,
      append_to_previous: false,
      text: TURN_TWO_FINAL_TEXT,
    })],
  },
  {
    id: "turn-2-finalized",
    persisted: [
      {
        id: "replay-turn-2-edit",
        parentMessageId: TURN_TWO_ASSISTANT_MESSAGE_ID,
        role: "assistant",
        content: "",
        contentBlocks: [makeEditContentToolUse(TURN_TWO_EDIT_ID, {
          filePath: EDIT_FILE_PATH,
          oldContent: "export const restore = () => {\n  return restored;\n};\n",
          oldString: "return restored;",
          newString: "return replayed;",
        })],
        status: "finalized",
      },
      {
        id: "replay-turn-2-final-text",
        parentMessageId: TURN_TWO_ASSISTANT_MESSAGE_ID,
        role: "assistant",
        content: TURN_TWO_FINAL_TEXT,
        contentBlocks: [makeContentText(TURN_TWO_FINAL_TEXT)],
        status: "finalized",
      },
    ],
    activeState: {
      is_active: false,
      tool_calls: [],
      streaming_tasks: [],
      partial_text: "",
    },
    events: [
      replayEvent("agent:message_created", {
        conversation_id: CONVERSATION_ID,
        context_id: PROJECT_ID,
        context_type: "project",
        role: "assistant",
        message_id: "replay-turn-2-final-text",
        content: TURN_TWO_FINAL_TEXT,
        created_at: timestampFor(REPLAY_STEP_IDS.length),
      }),
      replayEvent("agent:turn_completed", {
        conversation_id: CONVERSATION_ID,
        context_id: PROJECT_ID,
        context_type: "project",
        run_id: RUN_ID,
      }),
    ],
  },
];

function stepIndex(stepId: ReplayStepId): number {
  const index = REPLAY_STEP_IDS.indexOf(stepId);
  if (index < 0) throw new Error(`Unknown replay step: ${stepId}`);
  return index;
}

function statusAt(entry: PersistedTimelineEntry, throughIndex: number): TimelineStatus {
  if (entry.status !== "streaming" || !entry.finalizesAt) return entry.status;
  return throughIndex >= stepIndex(entry.finalizesAt) ? "finalized" : "streaming";
}

export function createReplayConversationFixture(): ReplayConversationFixture {
  const conversation: ChatConversation = {
    id: CONVERSATION_ID,
    contextType: "project",
    contextId: PROJECT_ID,
    claudeSessionId: null,
    providerSessionId: "provider-session-replay",
    providerHarness: "codex",
    coordinationMode: "solo",
    title: "Replay parity",
    messageCount: 13,
    lastMessageAt: timestampFor(REPLAY_STEP_IDS.length),
    createdAt: BASE_TIME,
    updatedAt: timestampFor(REPLAY_STEP_IDS.length),
  };

  return {
    conversation,
    timelinePage(throughStep, options = {}) {
      const throughIndex = stepIndex(throughStep);
      const entries = STEPS
        .slice(0, throughIndex + 1)
        .flatMap((step) => step.persisted ?? []);
      const allItems = entries.map((entry, index) => {
        const sequence = index + 1;
        const status = statusAt(entry, throughIndex);
        const asMessage = messageFromEntry(entry, sequence, status);
        const createdAt = timestampFor(sequence);
        return {
          id: `timeline-${entry.id}`,
          conversationId: CONVERSATION_ID,
          messageId: entry.parentMessageId,
          runId: RUN_ID,
          sequence,
          blockIndex: 0,
          role: entry.role,
          kind: asMessage.timelineKind ?? "text",
          status,
          content: entry.content,
          contentBlocks: entry.contentBlocks,
          toolCall: asMessage.toolCalls?.[0] ?? null,
          metadata: null,
          providerHarness: "codex",
          providerSessionId: "provider-session-replay",
          createdAt,
          updatedAt: createdAt,
          finalizedAt: status === "finalized" ? timestampFor(REPLAY_STEP_IDS.length) : null,
          asMessage,
        };
      });
      const limit = options.limit ?? 40;
      const beforeSequence = options.beforeSequence ?? null;
      const eligibleItems = beforeSequence == null
        ? allItems
        : allItems.filter((item) => item.sequence < beforeSequence);
      const items = eligibleItems.slice(Math.max(0, eligibleItems.length - limit));
      const oldestLoadedSequence = items[0]?.sequence ?? null;
      return {
        conversation,
        items,
        messages: items.map((item) => item.asMessage),
        limit,
        beforeSequence,
        totalItemCount: allItems.length,
        hasOlder: oldestLoadedSequence != null && oldestLoadedSequence > 1,
        oldestLoadedSequence,
        newestLoadedSequence: items[items.length - 1]?.sequence ?? null,
      };
    },
    activeState(throughStep) {
      const throughIndex = stepIndex(throughStep);
      const latest = STEPS
        .slice(0, throughIndex + 1)
        .reverse()
        .find((step) => step.activeState)?.activeState;
      return {
        is_active: latest?.is_active ?? false,
        runId: RUN_ID,
        tool_calls: latest?.tool_calls ?? [],
        streaming_tasks: latest?.streaming_tasks ?? [],
        partial_text: latest?.partial_text ?? "",
      };
    },
    events(fromStep, toStep) {
      const fromIndex = stepIndex(fromStep);
      const toIndex = stepIndex(toStep);
      if (toIndex < fromIndex) {
        throw new Error(`Replay cannot move backward from ${fromStep} to ${toStep}`);
      }
      return STEPS
        .slice(fromIndex + 1, toIndex + 1)
        .flatMap((step) => step.events ?? []);
    },
  };
}

/**
 * Claude-style delta stream used by the mid-stream conversation-switch tests.
 * The production response has not gained `partial_text_segments` yet, so this
 * fixture intentionally carries the forward-compatible field at its boundary.
 */
export function createDeltaReplayConversationFixture(): ReplayConversationFixture {
  const legacy = createReplayConversationFixture();
  const segmentA = "First segment survives the switch. ";
  const segmentB = "Second segment resumes from its mid-stream tail.";
  const activeSnapshots = new Map<ReplayStepId, DeltaActiveStateSnapshot>([
    ["turn-2-text-delta", {
      is_active: true,
      tool_calls: [],
      streaming_tasks: [],
      partial_text: segmentA,
      partial_text_segments: [segmentA],
    }],
    ["turn-2-edit-started", {
      is_active: true,
      tool_calls: [{
        id: TURN_TWO_GREP_ID,
        name: "Grep",
        arguments: { pattern: "delta", path: "frontend/src/hooks" },
        result: "frontend/src/hooks/useChatRecovery.ts",
      }],
      streaming_tasks: [],
      partial_text: segmentA + segmentB,
      partial_text_segments: [segmentA, segmentB],
    }],
  ]);

  return {
    ...legacy,
    activeState(step) {
      const snapshot = activeSnapshots.get(step);
      return snapshot
        ? ({ ...legacy.activeState(step), ...snapshot } as ConversationActiveStateResponse)
        : legacy.activeState(step);
    },
    events(fromStep, toStep) {
      const events = legacy.events(fromStep, toStep);
      return events.map((event) => event.name !== "agent:chunk" ? event : {
        ...event,
        payload: {
          ...event.payload,
          // block_index is the 0-based ordinal among TEXT blocks of the turn
          // (backend current_text_block_ordinal), not the content-block index.
          block_index: (event.payload.seq as number) >= 11 ? 1 : 0,
          append_to_previous: true,
        },
      });
    },
  };
}
