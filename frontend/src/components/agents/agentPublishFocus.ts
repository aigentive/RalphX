import type { DiffFilterMode } from "./AgentsPublishDiffFilter";

export interface AgentPublishFocusRequest {
  conversationId: string;
  filePath: string;
  mode: DiffFilterMode;
  requestId: number;
}
