import type { ContextType } from "./chat-conversation";
import type { InternalStatus } from "./status";

export type TaskRuntimeHistoryContextType = Extract<
  ContextType,
  "task_execution" | "review" | "merge" | "branch_update"
>;

export interface TaskHistoryState {
  status: InternalStatus;
  timestamp: string;
  /** Conversation ID from the state transition metadata, when a transcript exists. */
  conversationId?: string | undefined;
  /** Agent run ID from the state transition metadata. */
  agentRunId?: string | undefined;
  /** Runtime context that owns the stage transcript. */
  contextType?: TaskRuntimeHistoryContextType | undefined;
  /** Stable transition identity when provided or derived from status + timestamp. */
  transitionId?: string | undefined;
  /** One-based attempt index within the derived runtime stage family. */
  attemptIndex?: number | undefined;
  /** Explicit transcript availability marker; false means do not borrow another conversation. */
  hasConversation?: boolean | undefined;
}
