import { z } from "zod";

import {
  StartAgentConversationResponseSchema,
  startAgentConversationInvokeInput,
  transformStartAgentConversationResponse,
  type StartAgentConversationInput,
  type StartAgentConversationResult,
} from "@/api/chat";
import { typedInvoke, TauriVoidSchema } from "@/lib/tauri";
import { ViewTypeSchema } from "@/types/chat";

export const TicketingProviderSchema = z.enum(["jira", "linear", "clickup"]);
export type TicketingProvider = z.infer<typeof TicketingProviderSchema>;

export const TicketingConnectionStatusSchema = z.enum([
  "connected",
  "disconnected",
  "permission_limited",
  "error",
]);
export type TicketingConnectionStatus = z.infer<typeof TicketingConnectionStatusSchema>;

export const TicketingFreshnessSchema = z.enum(["webhook", "poll", "manual"]);
export type TicketingFreshness = z.infer<typeof TicketingFreshnessSchema>;

export const TicketingCapabilitiesSchema = z.object({
  supportsBoards: z.boolean().default(false),
  supportsKanban: z.boolean().default(false),
  kanbanWrite: z.boolean().default(false),
  statusWrite: z.boolean().default(false),
  assignmentWrite: z.boolean().default(false),
  commentWrite: z.boolean().default(false),
  labelWrite: z.boolean().default(false),
  freshness: TicketingFreshnessSchema.default("manual"),
});
export type TicketingCapabilities = z.infer<typeof TicketingCapabilitiesSchema>;

export const TicketingProviderSummarySchema = z.object({
  provider: TicketingProviderSchema,
  label: z.string(),
  enabled: z.boolean(),
  connectionStatus: TicketingConnectionStatusSchema,
  capabilities: TicketingCapabilitiesSchema,
  fetchedAt: z.string().nullable().optional(),
  staleAt: z.string().nullable().optional(),
  permissionMessage: z.string().nullable().optional(),
  errorMessage: z.string().nullable().optional(),
});
export type TicketingProviderSummary = z.infer<typeof TicketingProviderSummarySchema>;

export const TicketingContainerKindSchema = z.enum(["project", "board", "team"]);
export type TicketingContainerKind = z.infer<typeof TicketingContainerKindSchema>;

export const TicketingContainerSchema = z.object({
  provider: TicketingProviderSchema,
  id: z.string(),
  key: z.string().nullable().optional(),
  name: z.string(),
  kind: TicketingContainerKindSchema,
  parentId: z.string().nullable().optional(),
  ticketCount: z.number().nullable().optional(),
});
export type TicketingContainer = z.infer<typeof TicketingContainerSchema>;

export const TicketStateCategorySchema = z.enum([
  "todo",
  "in_progress",
  "done",
  "other",
]);
export type TicketStateCategory = z.infer<typeof TicketStateCategorySchema>;

export const TicketingColumnSchema = z.object({
  id: z.string(),
  name: z.string(),
  category: TicketStateCategorySchema,
  order: z.number().default(0),
  color: z.string().nullable().optional(),
});
export type TicketingColumn = z.infer<typeof TicketingColumnSchema>;

export const TicketRefSchema = z.object({
  provider: TicketingProviderSchema,
  id: z.string(),
  key: z.string().nullable().optional(),
});
export type TicketRef = z.infer<typeof TicketRefSchema>;

export const TicketingStateSchema = z.object({
  id: z.string(),
  name: z.string(),
  category: TicketStateCategorySchema,
  color: z.string().nullable().optional(),
});
export type TicketingState = z.infer<typeof TicketingStateSchema>;

export const TicketingPersonSchema = z.object({
  id: z.string().nullable().optional(),
  name: z.string(),
  email: z.string().nullable().optional(),
  avatarUrl: z.string().nullable().optional(),
});
export type TicketingPerson = z.infer<typeof TicketingPersonSchema>;

export const TicketSummarySchema = z.object({
  ref: TicketRefSchema,
  title: z.string(),
  state: TicketingStateSchema,
  assignee: TicketingPersonSchema.nullable().optional(),
  reporter: TicketingPersonSchema.nullable().optional(),
  labels: z.array(z.string()).default([]),
  project: z.string().nullable().optional(),
  priority: z.string().nullable().optional(),
  updatedAt: z.string(),
  url: z.string().nullable().optional(),
  associationCount: z.number().default(0),
  openPrCount: z.number().default(0),
  openPrNumber: z.number().nullable().optional(),
  openPrUrl: z.string().nullable().optional(),
  openPrStatus: z.string().nullable().optional(),
});
export type TicketSummary = z.infer<typeof TicketSummarySchema>;

export const TicketCommentSchema = z.object({
  id: z.string().nullable().optional(),
  author: TicketingPersonSchema.nullable().optional(),
  bodyMarkdown: z.string().default(""),
  bodyText: z.string().default(""),
  createdAt: z.string().nullable().optional(),
  updatedAt: z.string().nullable().optional(),
});
export type TicketComment = z.infer<typeof TicketCommentSchema>;

export const TicketAttachmentSchema = z.object({
  id: z.string().nullable().optional(),
  filename: z.string(),
  mimeType: z.string().nullable().optional(),
  size: z.number().nullable().optional(),
  url: z.string().nullable().optional(),
});
export type TicketAttachment = z.infer<typeof TicketAttachmentSchema>;

export const TicketTransitionOptionSchema = z.object({
  toStateId: z.string(),
  providerTransitionId: z.string().nullable().optional(),
  name: z.string(),
  category: TicketStateCategorySchema,
  disabledReason: z.string().nullable().optional(),
});
export type TicketTransitionOption = z.infer<typeof TicketTransitionOptionSchema>;

export const TicketLabelOptionSchema = z.object({
  id: z.string().nullable().optional(),
  name: z.string(),
});
export type TicketLabelOption = z.infer<typeof TicketLabelOptionSchema>;

export const TicketLabelsResponseSchema = z.object({
  labels: z.array(z.string()).default([]),
});
export type TicketLabelsResponse = z.infer<typeof TicketLabelsResponseSchema>;

export const TicketDetailSchema = TicketSummarySchema.extend({
  descriptionMarkdown: z.string().nullable().optional(),
  descriptionText: z.string().nullable().optional(),
  acceptanceCriteriaMarkdown: z.string().nullable().optional(),
  comments: z.array(TicketCommentSchema).default([]),
  attachments: z.array(TicketAttachmentSchema).default([]),
  transitions: z.array(TicketTransitionOptionSchema).default([]),
  fetchedAt: z.string().nullable().optional(),
});
export type TicketDetail = z.infer<typeof TicketDetailSchema>;

export const TicketPageSchema = z.object({
  items: z.array(TicketSummarySchema),
  nextCursor: z.string().nullable().optional(),
  total: z.number().nullable().optional(),
  fetchedAt: z.string().nullable().optional(),
});
export type TicketPage = z.infer<typeof TicketPageSchema>;

export const TicketDeepLinkSchema = z.object({
  view: ViewTypeSchema,
  id: z.string(),
  projectId: z.string().nullable().optional(),
});
export type TicketDeepLink = z.infer<typeof TicketDeepLinkSchema>;

export const TicketAssociationItemSchema = z.object({
  id: z.string(),
  title: z.string(),
  subtitle: z.string().nullable().optional(),
  status: z.string().nullable().optional(),
  active: z.boolean().default(false),
  deepLink: TicketDeepLinkSchema,
});
export type TicketAssociationItem = z.infer<typeof TicketAssociationItemSchema>;

export const TicketAssociationsSchema = z.object({
  tasks: z.array(TicketAssociationItemSchema).default([]),
  proposals: z.array(TicketAssociationItemSchema).default([]),
  sessions: z.array(TicketAssociationItemSchema).default([]),
  conversations: z.array(TicketAssociationItemSchema).default([]),
  pullRequests: z.array(TicketAssociationItemSchema).default([]),
  checks: z.array(TicketAssociationItemSchema).default([]),
  qa: z.array(TicketAssociationItemSchema).default([]),
  specs: z.array(TicketAssociationItemSchema).default([]),
  fetchedAt: z.string().nullable().optional(),
});
export type TicketAssociations = z.infer<typeof TicketAssociationsSchema>;

export const ConversationTicketSchema = z.object({
  ticketRef: TicketRefSchema,
  projectId: z.string(),
  title: z.string().nullable().optional(),
  url: z.string().nullable().optional(),
});
export type ConversationTicket = z.infer<typeof ConversationTicketSchema>;

export const RefreshTicketsResponseSchema = z.object({
  refreshedAt: z.string(),
});
export type RefreshTicketsResponse = z.infer<typeof RefreshTicketsResponseSchema>;

export const TicketOperationResponseSchema = z.object({
  id: z.string(),
  operation: z.enum(["transition", "assign", "comment"]),
  clientOperationId: z.string(),
  status: z.enum(["pending", "succeeded", "failed", "canceled", "timed_out"]),
  providerOperationId: z.string().nullable().optional(),
  errorMessage: z.string().nullable().optional(),
  linked: z.boolean(),
  createdAt: z.string(),
  updatedAt: z.string(),
});
export type TicketOperationResponse = z.infer<typeof TicketOperationResponseSchema>;

export const TicketMutationResponseSchema = z.object({
  ticketRef: TicketRefSchema,
  operation: TicketOperationResponseSchema,
  idempotent: z.boolean(),
  transition: TicketTransitionOptionSchema.nullable().optional(),
  assignee: TicketingPersonSchema.nullable().optional(),
  comment: TicketCommentSchema.nullable().optional(),
  labels: TicketLabelsResponseSchema.nullable().optional(),
  refreshedAt: z.string(),
});
export type TicketMutationResponse = z.infer<typeof TicketMutationResponseSchema>;

export const TicketSortSchema = z.enum(["updated_desc", "updated_asc", "priority"]);
export type TicketSort = z.infer<typeof TicketSortSchema>;

export interface ListTicketingProvidersInput {
  projectId?: string | undefined;
}

export interface ListTicketingContainersInput {
  provider: TicketingProvider;
  projectId?: string | undefined;
}

export interface ListTicketingColumnsInput {
  provider: TicketingProvider;
  containerId?: string | undefined;
}

export interface TicketFiltersInput {
  text?: string | undefined;
  assignee?: string | null | undefined;
  stateIds?: string[] | undefined;
  labels?: string[] | undefined;
}

export interface ListTicketsInput {
  provider: TicketingProvider;
  projectId?: string | undefined;
  containerId?: string | undefined;
  cursor?: string | undefined;
  limit?: number | undefined;
  filters?: TicketFiltersInput | undefined;
  sort?: TicketSort | undefined;
}

export interface TicketRefInput {
  provider: TicketingProvider;
  ticketRef: TicketRef;
}

export interface GetTicketAssociationsInput extends TicketRefInput {
  projectId: string;
}

export interface StartWorkFromTicketInput extends StartAgentConversationInput {
  ticketRef: TicketRef;
}

export interface RefreshTicketsInput {
  provider: TicketingProvider;
  containerId?: string | undefined;
}

export interface TransitionTicketStatusInput extends TicketRefInput {
  toStateId: string;
  providerTransitionId?: string | null | undefined;
  clientOperationId?: string | undefined;
  projectId?: string | undefined;
}

export interface AssignTicketInput extends TicketRefInput {
  clientOperationId?: string | undefined;
  projectId?: string | undefined;
}

export interface AddTicketCommentInput extends TicketRefInput {
  bodyMarkdown: string;
  clientOperationId?: string | undefined;
  projectId?: string | undefined;
}

export interface SetTicketLabelsInput extends TicketRefInput {
  labels: string[];
  clientOperationId?: string | undefined;
  projectId?: string | undefined;
}

function listTicketsQuery(input: ListTicketsInput): Record<string, unknown> {
  return {
    provider: input.provider,
    ...(input.projectId !== undefined && { projectId: input.projectId }),
    ...(input.containerId !== undefined && { containerId: input.containerId }),
    ...(input.cursor !== undefined && { cursor: input.cursor }),
    ...(input.limit !== undefined && { limit: input.limit }),
    ...(input.filters !== undefined && { filters: input.filters }),
    ...(input.sort !== undefined && { sort: input.sort }),
  };
}

export const ticketingApi = {
  listProviders(input: ListTicketingProvidersInput = {}): Promise<TicketingProviderSummary[]> {
    return typedInvoke(
      "list_ticketing_providers",
      {
        ...(input.projectId !== undefined && { projectId: input.projectId }),
      },
      z.array(TicketingProviderSummarySchema),
    );
  },

  listContainers(input: ListTicketingContainersInput): Promise<TicketingContainer[]> {
    return typedInvoke(
      "list_ticketing_containers",
      {
        provider: input.provider,
        ...(input.projectId !== undefined && { projectId: input.projectId }),
      },
      z.array(TicketingContainerSchema),
    );
  },

  listColumns(input: ListTicketingColumnsInput): Promise<TicketingColumn[]> {
    return typedInvoke(
      "list_ticketing_columns",
      {
        provider: input.provider,
        ...(input.containerId !== undefined && { containerId: input.containerId }),
      },
      z.array(TicketingColumnSchema),
    );
  },

  listTickets(input: ListTicketsInput): Promise<TicketPage> {
    return typedInvoke(
      "list_tickets",
      { query: listTicketsQuery(input) },
      TicketPageSchema,
    );
  },

  getTicketDetail(input: TicketRefInput): Promise<TicketDetail> {
    return typedInvoke(
      "get_ticket_detail",
      {
        provider: input.provider,
        ticketRef: input.ticketRef,
      },
      TicketDetailSchema,
    );
  },

  listTicketTransitions(input: TicketRefInput): Promise<TicketTransitionOption[]> {
    return typedInvoke(
      "list_ticket_transitions",
      {
        provider: input.provider,
        ticketRef: input.ticketRef,
      },
      z.array(TicketTransitionOptionSchema),
    );
  },

  getTicketAssociations(input: GetTicketAssociationsInput): Promise<TicketAssociations> {
    return typedInvoke(
      "get_ticket_associations",
      {
        provider: input.provider,
        ticketRef: input.ticketRef,
        projectId: input.projectId,
      },
      TicketAssociationsSchema,
    );
  },

  getConversationTicket(conversationId: string): Promise<ConversationTicket | null> {
    return typedInvoke(
      "get_conversation_ticket",
      { conversationId },
      ConversationTicketSchema.nullable(),
    );
  },

  async startWorkFromTicket(
    input: StartWorkFromTicketInput,
  ): Promise<StartAgentConversationResult> {
    const raw = await typedInvoke(
      "start_ralphx_work_from_ticket",
      {
        input: {
          ...startAgentConversationInvokeInput(input),
          ticketRef: input.ticketRef,
        },
      },
      StartAgentConversationResponseSchema,
    );
    return transformStartAgentConversationResponse(raw);
  },

  async refreshTickets(input: RefreshTicketsInput): Promise<RefreshTicketsResponse> {
    return typedInvoke(
      "refresh_tickets",
      {
        provider: input.provider,
        ...(input.containerId !== undefined && { containerId: input.containerId }),
      },
      RefreshTicketsResponseSchema.or(
        TauriVoidSchema.transform(() => ({
          refreshedAt: new Date().toISOString(),
        })),
      ),
    );
  },

  transitionTicketStatus(input: TransitionTicketStatusInput): Promise<TicketMutationResponse> {
    return typedInvoke(
      "transition_ticket_status",
      {
        input: {
          provider: input.provider,
          ticketRef: input.ticketRef,
          toStateId: input.toStateId,
          ...(input.providerTransitionId !== undefined && { providerTransitionId: input.providerTransitionId }),
          ...(input.clientOperationId !== undefined && { clientOperationId: input.clientOperationId }),
          ...(input.projectId !== undefined && { projectId: input.projectId }),
        },
      },
      TicketMutationResponseSchema,
    );
  },

  assignTicket(input: AssignTicketInput): Promise<TicketMutationResponse> {
    return typedInvoke(
      "assign_ticket",
      {
        input: {
          provider: input.provider,
          ticketRef: input.ticketRef,
          ...(input.clientOperationId !== undefined && { clientOperationId: input.clientOperationId }),
          ...(input.projectId !== undefined && { projectId: input.projectId }),
        },
      },
      TicketMutationResponseSchema,
    );
  },

  clearTicketAssignee(input: AssignTicketInput): Promise<TicketMutationResponse> {
    return typedInvoke(
      "clear_ticket_assignee",
      {
        input: {
          provider: input.provider,
          ticketRef: input.ticketRef,
          ...(input.clientOperationId !== undefined && { clientOperationId: input.clientOperationId }),
          ...(input.projectId !== undefined && { projectId: input.projectId }),
        },
      },
      TicketMutationResponseSchema,
    );
  },

  addTicketComment(input: AddTicketCommentInput): Promise<TicketMutationResponse> {
    return typedInvoke(
      "add_ticket_comment",
      {
        input: {
          provider: input.provider,
          ticketRef: input.ticketRef,
          bodyMarkdown: input.bodyMarkdown,
          ...(input.clientOperationId !== undefined && { clientOperationId: input.clientOperationId }),
          ...(input.projectId !== undefined && { projectId: input.projectId }),
        },
      },
      TicketMutationResponseSchema,
    );
  },

  setTicketLabels(input: SetTicketLabelsInput): Promise<TicketMutationResponse> {
    return typedInvoke(
      "set_ticket_labels",
      {
        input: {
          provider: input.provider,
          ticketRef: input.ticketRef,
          labels: input.labels,
          ...(input.clientOperationId !== undefined && { clientOperationId: input.clientOperationId }),
          ...(input.projectId !== undefined && { projectId: input.projectId }),
        },
      },
      TicketMutationResponseSchema,
    );
  },

  listTicketLabels(input: TicketRefInput): Promise<TicketLabelOption[]> {
    return typedInvoke(
      "list_ticket_labels",
      {
        provider: input.provider,
        ticketRef: input.ticketRef,
      },
      z.array(TicketLabelOptionSchema),
    );
  },
};
