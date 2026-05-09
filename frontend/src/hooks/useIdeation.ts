/**
 * useIdeation hooks - TanStack Query wrappers for ideation session management
 *
 * Provides hooks for fetching, creating, archiving, and deleting ideation sessions
 * with automatic caching, refetching, and error handling.
 */

import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import {
  ideationApi,
  type SessionWithDataResponse,
  type IdeationSessionResponse,
  type ApplyProposalsInput,
  type IdeationAnalysisBaseSelection,
} from "@/api/ideation";
import { planBranchApi } from "@/api/plan-branch";
import { taskKeys } from "@/hooks/useTasks";
import type { SessionGroupCounts } from "@/types/ideation";
import { useIdeationStore } from "@/stores/ideationStore";

const dependencyGraphsQueryKey = ["dependencies", "graph"] as const;

/**
 * Query key factory for ideation
 */
export const ideationKeys = {
  all: ["ideation"] as const,
  sessions: () => [...ideationKeys.all, "sessions"] as const,
  sessionList: (projectId: string) => [...ideationKeys.sessions(), "list", projectId] as const,
  sessionDetails: () => [...ideationKeys.sessions(), "detail"] as const,
  sessionDetail: (sessionId: string) => [...ideationKeys.sessionDetails(), sessionId] as const,
  sessionWithData: (sessionId: string) => [...ideationKeys.sessionDetail(sessionId), "with-data"] as const,
  sessionGroupCounts: (projectId: string, search?: string) => [...ideationKeys.sessions(), "counts", projectId, ...(search ? [search] : [])] as const,
  sessionsByGroup: (projectId: string, group: string, search?: string) => [...ideationKeys.sessions(), "group", projectId, group, ...(search ? [search] : [])] as const,
  proposals: () => [...ideationKeys.all, "proposals"] as const,
};

/**
 * Hook to fetch an ideation session with its proposals and messages
 *
 * @param sessionId - The session ID to fetch
 * @returns TanStack Query result with session data, proposals, and messages
 *
 * @remarks
 * This hook explicitly disables placeholder data to prevent a "flash" bug when
 * switching between sessions. The global queryClient config uses the previous
 * query data as placeholder, but for session switching we need clean transitions.
 * See resolvedSession in App.tsx for the full solution.
 *
 * @example
 * ```tsx
 * const { data, isLoading, error } = useIdeationSession("session-123");
 *
 * if (isLoading) return <Loading />;
 * if (error) return <Error message={error.message} />;
 * if (!data) return <NotFound />;
 *
 * return (
 *   <IdeationView
 *     session={data.session}
 *     proposals={data.proposals}
 *     messages={data.messages}
 *   />
 * );
 * ```
 */
interface IdeationQueryOptions {
  enabled?: boolean;
}

export function useIdeationSession(
  sessionId: string,
  options: IdeationQueryOptions = {}
) {
  return useQuery<SessionWithDataResponse | null, Error>({
    queryKey: ideationKeys.sessionWithData(sessionId),
    queryFn: () => ideationApi.sessions.getWithData(sessionId),
    enabled: Boolean(sessionId) && (options.enabled ?? true),
    /**
     * Explicitly return null instead of using global placeholderData.
     *
     * The global config preserves previous query data as placeholder, which causes
     * a flash when switching sessions: old session data briefly appears before
     * the new session loads. By returning null, we force clean transitions where
     * the consumer falls back to the store's activeSession.
     */
    placeholderData: () => null,
  });
}

/**
 * Hook to fetch all ideation sessions for a project
 *
 * @param projectId - The project ID to fetch sessions for
 * @returns TanStack Query result with sessions array
 *
 * @example
 * ```tsx
 * const { data: sessions, isLoading, error } = useIdeationSessions("project-123");
 *
 * if (isLoading) return <Loading />;
 * if (error) return <Error message={error.message} />;
 * return <SessionList sessions={sessions ?? []} />;
 * ```
 */
export function useIdeationSessions(
  projectId: string,
  options: IdeationQueryOptions = {}
) {
  return useQuery<IdeationSessionResponse[], Error>({
    queryKey: ideationKeys.sessionList(projectId),
    queryFn: () => ideationApi.sessions.list(projectId),
    enabled: Boolean(projectId) && (options.enabled ?? true),
  });
}

/**
 * Input for creating a new ideation session
 */
interface CreateSessionInput {
  projectId: string;
  title?: string;
  seedTaskId?: string;
  teamMode?: string;
  teamConfig?: { maxTeammates: number; modelCeiling: string; budgetLimit?: number | undefined; compositionMode: string };
  analysisBase?: IdeationAnalysisBaseSelection;
}

/**
 * Hook to create a new ideation session
 *
 * @returns Mutation object for creating sessions
 *
 * @example
 * ```tsx
 * const createSession = useCreateIdeationSession();
 *
 * const handleCreate = async () => {
 *   const session = await createSession.mutateAsync({
 *     projectId: "project-123",
 *     title: "New Feature Ideas",
 *   });
 *   navigate(`/ideation/${session.id}`);
 * };
 * ```
 */
export function useCreateIdeationSession() {
  const queryClient = useQueryClient();

  return useMutation<IdeationSessionResponse, Error, CreateSessionInput>({
    mutationFn: ({ projectId, title, seedTaskId, teamMode, teamConfig, analysisBase }) =>
      ideationApi.sessions.create(projectId, title, seedTaskId, teamMode, teamConfig, analysisBase),
    onSuccess: () => {
      // Broad invalidation: covers sessionList, sessionGroupCounts, and sessionsByGroup
      queryClient.invalidateQueries({
        queryKey: ideationKeys.sessions(),
      });
    },
  });
}

/**
 * Hook to archive an ideation session
 *
 * @returns Mutation object for archiving sessions
 *
 * @example
 * ```tsx
 * const archiveSession = useArchiveIdeationSession();
 *
 * const handleArchive = async (sessionId: string) => {
 *   await archiveSession.mutateAsync(sessionId);
 *   toast.success("Session archived");
 * };
 * ```
 */
export function useArchiveIdeationSession() {
  const queryClient = useQueryClient();

  return useMutation<void, Error, string>({
    mutationFn: (sessionId) => ideationApi.sessions.archive(sessionId),
    onSuccess: (_data, sessionId) => {
      // Invalidate both the session detail and session lists
      queryClient.invalidateQueries({
        queryKey: ideationKeys.sessionDetail(sessionId),
      });
      queryClient.invalidateQueries({
        queryKey: ideationKeys.sessions(),
      });
    },
  });
}

/**
 * Hook to reopen an accepted/archived ideation session back to Active
 *
 * Deletes all associated tasks, cleans up git resources, and resets session status.
 */
export function useReopenSession() {
  const queryClient = useQueryClient();

  return useMutation<void, Error, string>({
    mutationFn: (sessionId) => ideationApi.sessions.reopen(sessionId),
    onSuccess: (_data, sessionId) => {
      // Broad invalidation: session detail, session lists, tasks (deleted), proposals (cleared task links)
      queryClient.invalidateQueries({
        queryKey: ideationKeys.sessionDetail(sessionId),
      });
      queryClient.invalidateQueries({
        queryKey: ideationKeys.sessions(),
      });
      queryClient.invalidateQueries({
        queryKey: taskKeys.all,
      });
      queryClient.invalidateQueries({
        queryKey: dependencyGraphsQueryKey,
      });
      queryClient.invalidateQueries({
        queryKey: ideationKeys.proposals(),
      });
      queryClient.invalidateQueries({
        queryKey: ["plan-branch"],
      });
    },
  });
}

/**
 * Input for the reset & re-accept mutation
 */
interface ResetAndReacceptInput {
  sessionId: string;
  proposalIds: string[];
}

/**
 * Hook to fetch session group counts for a project
 *
 * Returns counts for all five session groups (drafts, in_progress, accepted, done, archived).
 * Used for displaying badge counts on collapsed group headers in PlanBrowser.
 *
 * @param projectId - The project ID to fetch counts for
 * @returns TanStack Query result with SessionGroupCounts
 */
export function useSessionGroupCounts(projectId: string, search?: string) {
  return useQuery<SessionGroupCounts>({
    queryKey: ideationKeys.sessionGroupCounts(projectId, search),
    queryFn: () => invoke<SessionGroupCounts>("get_session_group_counts", {
      projectId,
      ...(search ? { search } : {}),
    }),
    enabled: Boolean(projectId),
    staleTime: 30_000, // 30s — counts change less frequently
  });
}

/**
 * Hook to reset & re-accept an ideation session
 *
 * Chains: reopen (delete tasks, cleanup) → apply_proposals_to_kanban (create fresh tasks).
 */
export function useResetAndReaccept() {
  const queryClient = useQueryClient();

  return useMutation<void, Error, ResetAndReacceptInput>({
    mutationFn: async ({ sessionId, proposalIds }) => {
      // Fetch existing plan branch BEFORE reopen (reopen deletes the record)
      const planArtifactId = useIdeationStore.getState().planArtifact?.id;
      const existingPlanBranch = planArtifactId
        ? await planBranchApi.getByPlan(planArtifactId).catch(() => null)
        : null;

      // Step 1: Reopen (deletes tasks, cleans git, resets to Active)
      await ideationApi.sessions.reopen(sessionId);
      // Step 2: Re-apply proposals as fresh tasks
      const applyInput: ApplyProposalsInput = {
        sessionId,
        proposalIds,
        targetColumn: "backlog",
        ...(existingPlanBranch?.baseBranchOverride != null && {
          baseBranchOverride: existingPlanBranch.baseBranchOverride,
        }),
      };
      await ideationApi.apply.toKanban(applyInput);
    },
    onSuccess: (_data, { sessionId }) => {
      // Broad invalidation: everything touched by reopen + apply
      queryClient.invalidateQueries({
        queryKey: ideationKeys.sessionDetail(sessionId),
      });
      queryClient.invalidateQueries({
        queryKey: ideationKeys.sessions(),
      });
      queryClient.invalidateQueries({
        queryKey: taskKeys.all,
      });
      queryClient.invalidateQueries({
        queryKey: dependencyGraphsQueryKey,
      });
      queryClient.invalidateQueries({
        queryKey: ideationKeys.proposals(),
      });
      queryClient.invalidateQueries({
        queryKey: ["plan-branch"],
      });
    },
  });
}
