export type AgentPublishSubTab = "changes" | "review";

export interface AgentPublishSubTabRequest {
  conversationId: string;
  requestId: number;
  tab: AgentPublishSubTab;
}
