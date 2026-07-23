export type AgentPublishSubTab =
  | "changes"
  | "review"
  | "checks"
  | "history"
  | "automation";

export interface AgentPublishSubTabRequest {
  conversationId: string;
  requestId: number;
  tab: AgentPublishSubTab;
}
