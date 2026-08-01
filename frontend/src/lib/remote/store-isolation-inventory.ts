export type StoreClassification = "env-owned" | "global" | "mixed" | "infrastructure";

export type ResetPolicy =
  | { readonly mode: "full" }
  | { readonly mode: "fields"; readonly fields: readonly string[] }
  | { readonly mode: "none" };

export interface PersistedSliceSpec {
  readonly storageName: string;
  readonly envFields: readonly string[];
  readonly globalFields: readonly string[];
}

export interface StoreInventoryEntry {
  readonly storeName: string;
  readonly modulePath: string;
  readonly classification: StoreClassification;
  readonly persisted: PersistedSliceSpec | null;
  readonly reset: ResetPolicy;
  readonly rationale: string;
}

const full = { mode: "full" } as const;
const none = { mode: "none" } as const;

export const STORE_ISOLATION_INVENTORY: readonly StoreInventoryEntry[] = [
  { storeName: "useActivityStore", modulePath: "src/stores/activityStore.ts", classification: "env-owned", persisted: null, reset: full, rationale: "Contains backend activity and host task identifiers." },
  {
    storeName: "useAgentSessionStore",
    modulePath: "src/stores/agentSessionStore.ts",
    classification: "mixed",
    persisted: {
      storageName: "ralphx-agent-session-store",
      envFields: ["focusedProjectId", "selectedProjectId", "selectedConversationId", "lastSelectedConversationByProjectId", "expandedProjectIds", "sidebarProjectFilterIds", "pinnedConversationIds", "artifactByConversationId", "runtimeByConversationId", "serviceTierByConversationId", "roleRuntimeOverridesByConversationId", "lastRuntimeByProjectId", "branchBaseCacheByProjectId", "lastBranchBaseSelectionByProjectId"],
      globalFields: ["defaultStartMode", "showAllProjects", "showEmptyProjectGroups", "projectSort", "sidebarGroupBy", "sidebarInboxActiveLane", "sidebarPublicationStateFilters", "lastModelEffortByProvider"],
    },
    reset: full,
    rationale: "Combines host-keyed sessions with global agent sidebar and runtime preferences.",
  },
  { storeName: "useArtifactStore", modulePath: "src/stores/artifactStore.ts", classification: "env-owned", persisted: null, reset: full, rationale: "Contains backend artifacts, buckets, and selections." },
  { storeName: "useChatStore", modulePath: "src/stores/chatStore.ts", classification: "env-owned", persisted: null, reset: full, rationale: "Contains host conversations, messages, runs, queues, and drafts." },
  { storeName: "useEnvironmentStore", modulePath: "src/stores/environmentStore.ts", classification: "infrastructure", persisted: null, reset: none, rationale: "Owns environment identity and the switch funnel." },
  { storeName: "useIdeationStore", modulePath: "src/stores/ideationStore.ts", classification: "env-owned", persisted: null, reset: full, rationale: "Contains backend ideation sessions, artifacts, and verification state." },
  { storeName: "useIntegrationDashboardStore", modulePath: "src/stores/integrationDashboardStore.ts", classification: "env-owned", persisted: null, reset: full, rationale: "Dashboard state is keyed by host project and backend integration entities." },
  { storeName: "useMethodologyStore", modulePath: "src/stores/methodologyStore.ts", classification: "env-owned", persisted: null, reset: full, rationale: "Contains backend methodologies and workflow identifiers." },
  { storeName: "usePlanStore", modulePath: "src/stores/planStore.ts", classification: "env-owned", persisted: null, reset: full, rationale: "Contains host project-keyed plans and backend candidates." },
  {
    storeName: "useProjectStore", modulePath: "src/stores/projectStore.ts", classification: "env-owned",
    persisted: { storageName: "ralphx-project-store", envFields: ["activeProjectId"], globalFields: [] },
    reset: full, rationale: "Projects and the active project identifier belong to one host.",
  },
  { storeName: "useProposalStore", modulePath: "src/stores/proposalStore.ts", classification: "env-owned", persisted: null, reset: full, rationale: "Contains backend task proposals and proposal identifiers." },
  { storeName: "useQAStore", modulePath: "src/stores/qaStore.ts", classification: "env-owned", persisted: null, reset: full, rationale: "Contains backend QA settings and task-keyed results." },
  { storeName: "useRemoteConnectionJournalStore", modulePath: "src/stores/remoteConnectionJournalStore.ts", classification: "infrastructure", persisted: null, reset: none, rationale: "Client-side connection diagnostics keyed explicitly by environment id; rows are cleared by the runtime when an environment is removed." },
  { storeName: "useReviewStore", modulePath: "src/stores/reviewStore.ts", classification: "env-owned", persisted: null, reset: full, rationale: "Contains backend reviews and selected review identifiers." },
  { storeName: "useTaskStore", modulePath: "src/stores/taskStore.ts", classification: "env-owned", persisted: null, reset: full, rationale: "Contains the active host's backend tasks." },
  { storeName: "useThemeStore", modulePath: "src/stores/themeStore.ts", classification: "global", persisted: null, reset: none, rationale: "Theme, motion, and font scale are client presentation preferences." },
  {
    storeName: "useTicketingStore", modulePath: "src/stores/ticketingStore.ts", classification: "mixed",
    persisted: { storageName: "ralphx-ticketing-store", envFields: ["activeProvider", "activeContainerId", "filters", "selectedTicketRef", "lastOpenedAt"], globalFields: ["viewMode"] },
    reset: full, rationale: "Ticket entities and filters are host-owned while view mode is presentation.",
  },
  {
    storeName: "useUiStore", modulePath: "src/stores/uiStore.ts", classification: "mixed", persisted: null,
    reset: { mode: "fields", fields: ["activeModal", "modalContext", "notifications", "loading", "confirmation", "activeQuestions", "answeredQuestions", "recoveryPrompt", "recoveryPromptSurface", "executionStatus", "boardSearchQuery", "isSearching", "graphSelection", "taskHistoryState", "taskCreationContext", "preserveCurrentViewOnProjectSwitch", "activityFilter", "collapsedColumns", "viewByProject", "pendingConfirmationQueue", "autoAcceptPlans", "autoAcceptSessions"] },
    rationale: "Resets host-derived runtime and identifier-keyed UI while retaining presentation preferences and client-owned feature flags.",
  },
  { storeName: "useWorkflowStore", modulePath: "src/stores/workflowStore.ts", classification: "env-owned", persisted: null, reset: full, rationale: "Contains backend workflows and active workflow identifiers." },
  { storeName: "useAgentArtifactUiStore", modulePath: "src/components/agents/agentArtifactUiStore.ts", classification: "env-owned", persisted: null, reset: full, rationale: "Artifact UI is keyed by host conversation identifiers." },
  {
    storeName: "useAgentTerminalStore", modulePath: "src/components/agents/agentTerminalStore.ts", classification: "mixed",
    persisted: { storageName: "ralphx-agent-terminal-ui", envFields: ["openByConversationId", "heightByConversationId", "activeTerminalByConversationId", "metadataByConversationId"], globalFields: ["placement"] },
    reset: full, rationale: "Terminal maps are conversation-owned while placement is presentation.",
  },
  { storeName: "useHookEventsStore", modulePath: "src/hooks/useAgentHookEvents.ts", classification: "env-owned", persisted: null, reset: full, rationale: "Contains streamed backend hook events and task identifiers." },
  { storeName: "useSupervisorStore", modulePath: "src/hooks/useSupervisorAlerts.store.ts", classification: "env-owned", persisted: null, reset: full, rationale: "Contains backend supervisor alerts, connection state, and host configuration." },
];
