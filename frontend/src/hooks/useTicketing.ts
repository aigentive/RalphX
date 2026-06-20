import { useInfiniteQuery, useMutation, useQuery, useQueryClient, type InfiniteData } from "@tanstack/react-query";

import {
  ticketingApi,
  type GetTicketAssociationsInput,
  type ListTicketingColumnsInput,
  type ListTicketingContainersInput,
  type ListTicketsInput,
  type RefreshTicketsInput,
  type StartWorkFromTicketInput,
  type TicketAssociations,
  type TicketDetail,
  type TicketPage,
  type TicketRefInput,
  type TicketTransitionOption,
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
  detail: (input: TicketRefInput) =>
    [...ticketingKeys.all, "detail", input.provider, input.ticketRef.id, input.ticketRef.key ?? null] as const,
  transitions: (input: TicketRefInput) =>
    [...ticketingKeys.detail(input), "transitions"] as const,
  associations: (input: GetTicketAssociationsInput) =>
    [...ticketingKeys.detail(input), "associations", input.projectId] as const,
};

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
