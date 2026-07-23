export type AgentPublishSubTab =
  | "changes"
  | "review"
  | "history"
  | "automation";

export interface AgentPublishSubTabRequest {
  conversationId: string;
  requestId: number;
  tab: AgentPublishSubTab;
}
