import {
  useInfiniteQuery,
  useMutation,
  useQuery,
  useQueryClient,
  type InfiniteData,
  type QueryClient,
  type QueryKey,
} from "@tanstack/react-query";

import {
  ticketingApi,
  type AddTicketCommentInput,
  type AssignTicketInput,
  type GetTicketAssociationsInput,
  type ListTicketingColumnsInput,
  type ListTicketingContainersInput,
  type ListTicketsInput,
  type RefreshTicketsInput,
  type StartWorkFromTicketInput,
  type TicketAssociations,
  type TicketComment,
  type TicketDetail,
  type TicketingColumn,
  type TicketPage,
  type TicketRefInput,
  type TicketSummary,
  type TicketTransitionOption,
  type TransitionTicketStatusInput,
} from "@/api/ticketing";

interface QueryOptions {
  enabled?: boolean | undefined;
}

export const ticketingKeys = {
  all: ["ticketing"] as const,
  providers: (projectId?: string) =>
    [...ticketingKeys.all, "providers", projectId ?? null] as const,
  containers: (input: ListTicketingContainersInput) =>
    [...ticketingKeys.all, "containers", input.provider, input.projectId ?? null] as const,
  columns: (input: ListTicketingColumnsInput) =>
    [...ticketingKeys.all, "columns", input.provider, input.containerId ?? null] as const,
  tickets: (input: ListTicketsInput) =>
    [
      ...ticketingKeys.all,
      "tickets",
      input.provider,
      input.projectId ?? null,
      input.containerId ?? null,
      input.filters ?? null,
      input.sort ?? null,
      input.limit ?? null,
    ] as const,
  ticketLists: () => [...ticketingKeys.all, "tickets"] as const,
  detail: (input: TicketRefInput) =>
    [...ticketingKeys.all, "detail", input.provider, input.ticketRef.id, input.ticketRef.key ?? null] as const,
  transitions: (input: TicketRefInput) =>
    [...ticketingKeys.detail(input), "transitions"] as const,
  associations: (input: GetTicketAssociationsInput) =>
    [...ticketingKeys.detail(input), "associations", input.projectId] as const,
};

export interface TransitionTicketStatusMutationInput extends TicketRefInput {
  transition: TicketTransitionOption;
  clientOperationId?: string | undefined;
  projectId?: string | undefined;
}

export interface AssignTicketMutationInput extends TicketRefInput {
  clientOperationId?: string | undefined;
  projectId?: string | undefined;
}

export interface AddTicketCommentMutationInput extends TicketRefInput {
  bodyMarkdown: string;
  clientOperationId?: string | undefined;
  projectId?: string | undefined;
}

interface TicketMutationSnapshot {
  detailKey: ReturnType<typeof ticketingKeys.detail>;
  previousDetail: TicketDetail | undefined;
  previousTicketLists: Array<[QueryKey, InfiniteData<TicketPage> | undefined]>;
}

type TicketOperationKind = "transition" | "assign" | "comment";

export function createTicketClientOperationId(
  operation: TicketOperationKind,
  ticketRef: TicketRefInput["ticketRef"],
): string {
  const randomId = typeof globalThis.crypto?.randomUUID === "function"
    ? globalThis.crypto.randomUUID()
    : `${Date.now()}-${Math.random().toString(36).slice(2)}`;
  return `ticketing:${operation}:${ticketRef.provider}:${ticketRef.key ?? ticketRef.id}:${randomId}`;
}

function withClientOperationId<T extends { clientOperationId?: string | undefined; ticketRef: TicketRefInput["ticketRef"] }>(
  input: T,
  operation: TicketOperationKind,
): T & { clientOperationId: string } {
  const clientOperationId = input.clientOperationId?.trim()
    || createTicketClientOperationId(operation, input.ticketRef);
  return { ...input, clientOperationId };
}

function ticketRefsMatch(
  left: TicketRefInput["ticketRef"],
  right: TicketRefInput["ticketRef"],
): boolean {
  return left.provider === right.provider && left.id === right.id && (left.key ?? null) === (right.key ?? null);
}

function patchTicketPages(
  data: InfiniteData<TicketPage> | undefined,
  ticketRef: TicketRefInput["ticketRef"],
  patchTicket: (ticket: TicketSummary) => TicketSummary,
): InfiniteData<TicketPage> | undefined {
  if (!data) {
    return data;
  }
  return {
    ...data,
    pages: data.pages.map((page) => ({
      ...page,
      items: page.items.map((ticket) => (
        ticketRefsMatch(ticket.ref, ticketRef) ? patchTicket(ticket) : ticket
      )),
    })),
  };
}

function snapshotAndPatchTicket(
  queryClient: QueryClient,
  input: TicketRefInput,
  patchTicket: (ticket: TicketSummary) => TicketSummary,
): TicketMutationSnapshot {
  const detailKey = ticketingKeys.detail(input);
  const previousDetail = queryClient.getQueryData<TicketDetail>(detailKey);
  const previousTicketLists = queryClient.getQueriesData<InfiniteData<TicketPage>>({
    queryKey: ticketingKeys.ticketLists(),
  });

  queryClient.setQueryData<TicketDetail>(detailKey, (detail) => {
    if (!detail || !ticketRefsMatch(detail.ref, input.ticketRef)) {
      return detail;
    }
    return patchTicket(detail) as TicketDetail;
  });
  queryClient.setQueriesData<InfiniteData<TicketPage>>(
    { queryKey: ticketingKeys.ticketLists() },
    (data) => patchTicketPages(data, input.ticketRef, patchTicket),
  );

  return { detailKey, previousDetail, previousTicketLists };
}

function restoreTicketSnapshot(
  queryClient: QueryClient,
  snapshot: TicketMutationSnapshot | undefined,
) {
  if (!snapshot) {
    return;
  }
  queryClient.setQueryData(snapshot.detailKey, snapshot.previousDetail);
  for (const [queryKey, data] of snapshot.previousTicketLists) {
    queryClient.setQueryData(queryKey, data);
  }
}

function invalidateTicketMutationQueries(
  queryClient: QueryClient,
  input: TicketRefInput & { projectId?: string | undefined },
) {
  void queryClient.invalidateQueries({ queryKey: ticketingKeys.ticketLists() });
  void queryClient.invalidateQueries({ queryKey: ticketingKeys.detail(input) });
  void queryClient.invalidateQueries({ queryKey: ticketingKeys.transitions(input) });
  if (input.projectId) {
    void queryClient.invalidateQueries({
      queryKey: ticketingKeys.associations({ ...input, projectId: input.projectId }),
    });
  }
}

function transitionPatch(transition: TicketTransitionOption) {
  return (ticket: TicketSummary): TicketSummary => ({
    ...ticket,
    state: {
      id: transition.toStateId,
      name: transition.name,
      category: transition.category,
      ...(ticket.state.color !== undefined && { color: ticket.state.color }),
    },
  });
}

function assignToMePatch(ticket: TicketSummary): TicketSummary {
  return {
    ...ticket,
    assignee: { name: "Me" },
  };
}

function optimisticComment(input: AddTicketCommentMutationInput & { clientOperationId: string }): TicketComment {
  const createdAt = new Date().toISOString();
  return {
    id: `optimistic:${input.clientOperationId}`,
    author: { name: "You" },
    bodyMarkdown: input.bodyMarkdown,
    bodyText: input.bodyMarkdown,
    createdAt,
    updatedAt: createdAt,
  };
}

function hasTicketComments(ticket: TicketSummary): ticket is TicketDetail {
  return "comments" in ticket && Array.isArray((ticket as TicketDetail).comments);
}

function addCommentPatch(comment: TicketComment) {
  return (ticket: TicketSummary): TicketSummary => {
    if (!hasTicketComments(ticket)) {
      return ticket;
    }
    return {
      ...ticket,
      comments: [...ticket.comments, comment],
    } as TicketDetail;
  };
}

function replaceOptimisticCommentPatch(
  clientOperationId: string,
  comment: TicketComment,
) {
  return (ticket: TicketSummary): TicketSummary => {
    if (!hasTicketComments(ticket)) {
      return ticket;
    }
    return {
      ...ticket,
      comments: ticket.comments.map((existing) => (
        existing.id === `optimistic:${clientOperationId}` ? comment : existing
      )),
    } as TicketDetail;
  };
}

export function useTicketingProviders(projectId?: string, options: QueryOptions = {}) {
  return useQuery({
    queryKey: ticketingKeys.providers(projectId),
    queryFn: () => ticketingApi.listProviders({ projectId }),
    enabled: options.enabled ?? true,
    staleTime: 60_000,
  });
}

export function useTicketingContainers(
  input: ListTicketingContainersInput | null,
  options: QueryOptions = {},
) {
  return useQuery({
    queryKey: input ? ticketingKeys.containers(input) : [...ticketingKeys.all, "containers", null],
    queryFn: () => {
      if (!input) {
        throw new Error("Ticketing provider is required");
      }
      return ticketingApi.listContainers(input);
    },
    enabled: (options.enabled ?? true) && Boolean(input?.provider),
    staleTime: 60_000,
  });
}

export function useTicketingColumns(
  input: ListTicketingColumnsInput | null,
  options: QueryOptions = {},
) {
  return useQuery({
    queryKey: input ? ticketingKeys.columns(input) : [...ticketingKeys.all, "columns", null],
    queryFn: () => {
      if (!input) {
        throw new Error("Ticketing provider is required");
      }
      return ticketingApi.listColumns(input);
    },
    enabled: (options.enabled ?? true) && Boolean(input?.provider),
    staleTime: 60_000,
  });
}

export function useTickets(input: ListTicketsInput | null, options: QueryOptions = {}) {
  return useInfiniteQuery({
    queryKey: input ? ticketingKeys.tickets(input) : [...ticketingKeys.all, "tickets", null],
    queryFn: ({ pageParam }) => {
      if (!input) {
        throw new Error("Ticketing query is required");
      }
      const cursor = typeof pageParam === "string" ? pageParam : input.cursor;
      return ticketingApi.listTickets({
        ...input,
        ...(cursor !== undefined && { cursor }),
      });
    },
    getNextPageParam: (lastPage: TicketPage) => lastPage.nextCursor ?? undefined,
    initialPageParam: input?.cursor ?? null,
    enabled: (options.enabled ?? true) && Boolean(input?.provider),
    staleTime: 30_000,
  });
}

export function flattenTicketPages(
  data: InfiniteData<TicketPage> | undefined,
) {
  return data?.pages.flatMap((page) => page.items) ?? [];
}

export function useTicketDetail(input: TicketRefInput | null, options: QueryOptions = {}) {
  return useQuery<TicketDetail>({
    queryKey: input ? ticketingKeys.detail(input) : [...ticketingKeys.all, "detail", null],
    queryFn: () => {
      if (!input) {
        throw new Error("Ticket ref is required");
      }
      return ticketingApi.getTicketDetail(input);
    },
    enabled: (options.enabled ?? true) && Boolean(input?.ticketRef.id),
    staleTime: 30_000,
  });
}

export function useTicketTransitions(input: TicketRefInput | null, options: QueryOptions = {}) {
  return useQuery<TicketTransitionOption[]>({
    queryKey: input ? ticketingKeys.transitions(input) : [...ticketingKeys.all, "transitions", null],
    queryFn: () => {
      if (!input) {
        throw new Error("Ticket ref is required");
      }
      return ticketingApi.listTicketTransitions(input);
    },
    enabled: (options.enabled ?? true) && Boolean(input?.ticketRef.id),
    staleTime: 30_000,
  });
}

export function fetchTicketTransitionsForMove(
  queryClient: QueryClient,
  input: TicketRefInput,
) {
  return queryClient.fetchQuery({
    queryKey: ticketingKeys.transitions(input),
    queryFn: () => ticketingApi.listTicketTransitions(input),
    staleTime: 30_000,
  });
}

export function findTicketTransitionForColumn(
  transitions: TicketTransitionOption[],
  column: TicketingColumn,
): TicketTransitionOption | null {
  const transition = transitions.find((item) => item.toStateId === column.id);
  if (!transition || transition.disabledReason) {
    return null;
  }
  return transition;
}

export function useTicketAssociations(
  input: GetTicketAssociationsInput | null,
  options: QueryOptions = {},
) {
  return useQuery<TicketAssociations>({
    queryKey: input ? ticketingKeys.associations(input) : [...ticketingKeys.all, "associations", null],
    queryFn: () => {
      if (!input) {
        throw new Error("Ticket association query is required");
      }
      return ticketingApi.getTicketAssociations(input);
    },
    enabled: (options.enabled ?? true) && Boolean(input?.projectId && input.ticketRef.id),
    staleTime: 30_000,
  });
}

export function useRefreshTickets() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (input: RefreshTicketsInput) => ticketingApi.refreshTickets(input),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ticketingKeys.all });
    },
  });
}

export function useTicketingMutations(projectId?: string) {
  const queryClient = useQueryClient();

  const transitionStatusMutation = useMutation({
    mutationFn: (input: TransitionTicketStatusMutationInput & { clientOperationId: string }) => {
      const providerTransitionId = input.transition.providerTransitionId;
      const commandInput: TransitionTicketStatusInput = {
        provider: input.provider,
        ticketRef: input.ticketRef,
        toStateId: input.transition.toStateId,
        clientOperationId: input.clientOperationId,
        ...(providerTransitionId !== undefined && { providerTransitionId }),
        ...((input.projectId ?? projectId) !== undefined && { projectId: input.projectId ?? projectId }),
      };
      return ticketingApi.transitionTicketStatus(commandInput);
    },
    onMutate: async (input) => {
      await queryClient.cancelQueries({ queryKey: ticketingKeys.all });
      return snapshotAndPatchTicket(
        queryClient,
        input,
        transitionPatch(input.transition),
      );
    },
    onError: (_error, _input, snapshot) => {
      restoreTicketSnapshot(queryClient, snapshot);
    },
    onSettled: (_data, _error, input) => {
      invalidateTicketMutationQueries(queryClient, {
        provider: input.provider,
        ticketRef: input.ticketRef,
        ...(input.projectId ?? projectId ? { projectId: input.projectId ?? projectId } : {}),
      });
    },
  });

  const assignToMeMutation = useMutation({
    mutationFn: (input: AssignTicketMutationInput & { clientOperationId: string }) => {
      const commandInput: AssignTicketInput = {
        provider: input.provider,
        ticketRef: input.ticketRef,
        clientOperationId: input.clientOperationId,
        ...((input.projectId ?? projectId) !== undefined && { projectId: input.projectId ?? projectId }),
      };
      return ticketingApi.assignTicket(commandInput);
    },
    onMutate: async (input) => {
      await queryClient.cancelQueries({ queryKey: ticketingKeys.all });
      return snapshotAndPatchTicket(queryClient, input, assignToMePatch);
    },
    onError: (_error, _input, snapshot) => {
      restoreTicketSnapshot(queryClient, snapshot);
    },
    onSettled: (_data, _error, input) => {
      invalidateTicketMutationQueries(queryClient, {
        provider: input.provider,
        ticketRef: input.ticketRef,
        ...(input.projectId ?? projectId ? { projectId: input.projectId ?? projectId } : {}),
      });
    },
  });

  const addCommentMutation = useMutation({
    mutationFn: (input: AddTicketCommentMutationInput & { clientOperationId: string }) => {
      const commandInput: AddTicketCommentInput = {
        provider: input.provider,
        ticketRef: input.ticketRef,
        bodyMarkdown: input.bodyMarkdown,
        clientOperationId: input.clientOperationId,
        ...((input.projectId ?? projectId) !== undefined && { projectId: input.projectId ?? projectId }),
      };
      return ticketingApi.addTicketComment(commandInput);
    },
    onMutate: async (input) => {
      await queryClient.cancelQueries({ queryKey: ticketingKeys.all });
      return snapshotAndPatchTicket(
        queryClient,
        input,
        addCommentPatch(optimisticComment(input)),
      );
    },
    onSuccess: (data, input) => {
      if (!data.comment) {
        return;
      }
      snapshotAndPatchTicket(
        queryClient,
        input,
        replaceOptimisticCommentPatch(input.clientOperationId, data.comment),
      );
    },
    onError: (_error, _input, snapshot) => {
      restoreTicketSnapshot(queryClient, snapshot);
    },
    onSettled: (_data, _error, input) => {
      invalidateTicketMutationQueries(queryClient, {
        provider: input.provider,
        ticketRef: input.ticketRef,
        ...(input.projectId ?? projectId ? { projectId: input.projectId ?? projectId } : {}),
      });
    },
  });

  return {
    transitionStatus: (input: TransitionTicketStatusMutationInput) =>
      transitionStatusMutation.mutateAsync(withClientOperationId(input, "transition")),
    assignToMe: (input: AssignTicketMutationInput) =>
      assignToMeMutation.mutateAsync(withClientOperationId(input, "assign")),
    addComment: (input: AddTicketCommentMutationInput) =>
      addCommentMutation.mutateAsync(withClientOperationId(input, "comment")),
    transitionStatusMutation,
    assignToMeMutation,
    addCommentMutation,
  };
}

export function useStartWorkFromTicket() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (input: StartWorkFromTicketInput) =>
      ticketingApi.startWorkFromTicket(input),
    onSuccess: (_result, input) => {
      const ticketInput = {
        provider: input.ticketRef.provider,
        ticketRef: input.ticketRef,
      };
      void queryClient.invalidateQueries({
        queryKey: ticketingKeys.associations({
          ...ticketInput,
          projectId: input.projectId,
        }),
      });
      void queryClient.invalidateQueries({
        queryKey: ticketingKeys.detail(ticketInput),
      });
      void queryClient.invalidateQueries({
        queryKey: [
          ...ticketingKeys.all,
          "tickets",
          input.ticketRef.provider,
          input.projectId,
        ],
      });
    },
  });
}
