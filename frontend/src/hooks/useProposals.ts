/**
 * useProposals hooks - TanStack Query wrappers for task proposal management
 *
 * Provides hooks for fetching and mutating task proposals within ideation sessions
 * with automatic caching, refetching, and error handling.
 */

import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import {
  ideationApi,
  type TaskProposalResponse,
  type CreateProposalInput,
  type UpdateProposalInput,
} from "@/api/ideation";
import { ideationKeys } from "./useIdeation";

/**
 * Query key factory for proposals
 */
export const proposalKeys = {
  all: ["proposals"] as const,
  lists: () => [...proposalKeys.all, "list"] as const,
  list: (sessionId: string) => [...proposalKeys.lists(), sessionId] as const,
  details: () => [...proposalKeys.all, "detail"] as const,
  detail: (proposalId: string) => [...proposalKeys.details(), proposalId] as const,
};

/**
 * Hook to fetch all proposals for an ideation session
 *
 * @param sessionId - The session ID to fetch proposals for
 * @returns TanStack Query result with proposals array
 *
 * @example
 * ```tsx
 * const { data: proposals, isLoading, error } = useProposals("session-123");
 *
 * if (isLoading) return <Loading />;
 * if (error) return <Error message={error.message} />;
 * return <ProposalList proposals={proposals ?? []} />;
 * ```
 */
export function useProposals(sessionId: string) {
  return useQuery<TaskProposalResponse[], Error>({
    queryKey: proposalKeys.list(sessionId),
    queryFn: () => ideationApi.proposals.list(sessionId),
    enabled: Boolean(sessionId),
  });
}

/**
 * Hook providing mutations for task proposals
 *
 * @returns Object with mutation functions for proposal CRUD operations
 *
 * @example
 * ```tsx
 * const { createProposal, updateProposal, deleteProposal, reorder } =
 *   useProposalMutations();
 *
 * // Create a new proposal
 * const handleCreate = async () => {
 *   const proposal = await createProposal.mutateAsync({
 *     sessionId: "session-123",
 *     title: "New Feature",
 *     category: "feature",
 *   });
 * };
 *
 * // Update a proposal
 * const handleUpdate = async (id: string) => {
 *   await updateProposal.mutateAsync({
 *     proposalId: id,
 *     changes: { title: "Updated Title" },
 *   });
 * };
 *
 * ```
 */
export function useProposalMutations() {
  const queryClient = useQueryClient();

  const createProposal = useMutation<TaskProposalResponse, Error, CreateProposalInput>({
    mutationFn: (input) => ideationApi.proposals.create(input),
    onSuccess: (newProposal) => {
      // Invalidate proposal list for the session
      queryClient.invalidateQueries({
        queryKey: proposalKeys.list(newProposal.sessionId),
      });
      // Also invalidate session with data
      queryClient.invalidateQueries({
        queryKey: ideationKeys.sessionWithData(newProposal.sessionId),
      });
    },
  });

  const updateProposal = useMutation<
    TaskProposalResponse,
    Error,
    { proposalId: string; changes: UpdateProposalInput }
  >({
    mutationFn: ({ proposalId, changes }) =>
      ideationApi.proposals.update(proposalId, changes),
    onSuccess: (updatedProposal) => {
      // Invalidate proposal list and detail
      queryClient.invalidateQueries({
        queryKey: proposalKeys.list(updatedProposal.sessionId),
      });
      queryClient.invalidateQueries({
        queryKey: proposalKeys.detail(updatedProposal.id),
      });
      // Also invalidate session with data
      queryClient.invalidateQueries({
        queryKey: ideationKeys.sessionWithData(updatedProposal.sessionId),
      });
    },
  });

  const deleteProposal = useMutation<void, Error, string>({
    mutationFn: (proposalId) => ideationApi.proposals.archive(proposalId),
    onSuccess: (_data, proposalId) => {
      // Remove from cache and invalidate lists
      queryClient.removeQueries({
        queryKey: proposalKeys.detail(proposalId),
      });
      // Invalidate all proposal lists since we don't know the sessionId
      queryClient.invalidateQueries({
        queryKey: proposalKeys.lists(),
      });
      // Also invalidate all sessions with data
      queryClient.invalidateQueries({
        queryKey: ideationKeys.sessionDetails(),
      });
    },
  });

  const reorder = useMutation<void, Error, { sessionId: string; proposalIds: string[] }>({
    mutationFn: ({ sessionId, proposalIds }) =>
      ideationApi.proposals.reorder(sessionId, proposalIds),
    onSuccess: (_data, { sessionId }) => {
      // Invalidate proposal list for the session
      queryClient.invalidateQueries({
        queryKey: proposalKeys.list(sessionId),
      });
      // Also invalidate session with data
      queryClient.invalidateQueries({
        queryKey: ideationKeys.sessionWithData(sessionId),
      });
    },
  });

  return {
    createProposal,
    updateProposal,
    deleteProposal,
    reorder,
  };
}
