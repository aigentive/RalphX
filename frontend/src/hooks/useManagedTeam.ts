import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import {
  managedTeamApi,
  type AddManagedTeamMemberInput,
  type AssignManagedTeamMemberInput,
  type ManagedTeamMember,
  type ManagedTeamStatus,
  type StopManagedTeamMemberInput,
} from "@/api/managed-team";

const RECOVERY_POLL_INTERVAL_MS = 12_000;
const MAX_RECOVERY_POLLS = 3;

export const managedTeamKeys = {
  all: ["managed-team"] as const,
  status: (conversationId: string) =>
    [...managedTeamKeys.all, "status", conversationId] as const,
  roster: (teamId: string) =>
    [...managedTeamKeys.all, "roster", teamId] as const,
  idle: (conversationId: string, agentRunId: string) =>
    [...managedTeamKeys.all, "idle", conversationId, agentRunId] as const,
  sequence: (conversationId: string) =>
    [...managedTeamKeys.all, "sequence", conversationId] as const,
};

function shouldRecoveryPoll(status: ManagedTeamStatus | null | undefined): boolean {
  return Boolean(
    status?.session.status === "active" &&
      status.members.some((member) =>
        ["working", "provisioning", "stopping"].includes(member.status),
      ),
  );
}

export function useManagedTeamStatus(
  conversationId: string | null | undefined,
  options: { enabled?: boolean } = {},
) {
  return useQuery({
    queryKey: managedTeamKeys.status(conversationId ?? ""),
    queryFn: () => managedTeamApi.getStatus(conversationId!),
    enabled: Boolean(conversationId) && (options.enabled ?? true),
    staleTime: 5_000,
    // Events are authoritative. A short, capped poll only repairs missed events
    // while an active member can still change state.
    refetchInterval: (query) =>
      shouldRecoveryPoll(query.state.data) &&
      query.state.dataUpdateCount < MAX_RECOVERY_POLLS + 1
        ? RECOVERY_POLL_INTERVAL_MS
        : false,
  });
}

export function useEnsureManagedTeam(
  conversationId: string | null | undefined,
) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (projectId: string) =>
      managedTeamApi.ensure({ conversationId: conversationId!, projectId }),
    onSuccess: () => {
      if (!conversationId) return;
      void queryClient.invalidateQueries({
        queryKey: managedTeamKeys.status(conversationId),
      });
    },
  });
}

export function useManagedTeamRoster(
  teamId: string | null | undefined,
  options: { enabled?: boolean } = {},
) {
  return useQuery({
    queryKey: managedTeamKeys.roster(teamId ?? ""),
    queryFn: () => managedTeamApi.getRoster(teamId!),
    enabled: Boolean(teamId) && (options.enabled ?? true),
    staleTime: 5_000,
  });
}

export function useIdleManagedTeamMembers(
  authority: { conversationId: string; agentRunId: string } | null,
) {
  return useQuery({
    queryKey: managedTeamKeys.idle(
      authority?.conversationId ?? "",
      authority?.agentRunId ?? "",
    ),
    queryFn: () => managedTeamApi.getIdleMembers(authority!),
    enabled: authority !== null,
    staleTime: 5_000,
  });
}

function patchMember(
  previous: ManagedTeamStatus | null | undefined,
  member: ManagedTeamMember,
): ManagedTeamStatus | null | undefined {
  if (!previous || previous.session.id !== member.teamId) return previous;
  const existing = previous.members.find((item) => item.id === member.id);
  if (existing && existing.generation > member.generation) return previous;
  const members = existing
    ? previous.members.map((item) => (item.id === member.id ? member : item))
    : [...previous.members, member];
  return { ...previous, members };
}

function patchStatusMember(
  queryClient: ReturnType<typeof useQueryClient>,
  conversationId: string,
  member: ManagedTeamMember,
) {
  queryClient.setQueryData<ManagedTeamStatus | null>(
    managedTeamKeys.status(conversationId),
    (previous) => patchMember(previous, member),
  );
}

export function useManagedTeamMemberActions(conversationId: string | null) {
  const queryClient = useQueryClient();
  const patch = (member: ManagedTeamMember) => {
    if (conversationId) patchStatusMember(queryClient, conversationId, member);
  };
  const invalidate = () => {
    if (!conversationId) return;
    void queryClient.invalidateQueries({
      queryKey: managedTeamKeys.status(conversationId),
    });
  };

  const addMember = useMutation({
    mutationFn: (input: AddManagedTeamMemberInput) => managedTeamApi.addMember(input),
    onSuccess: patch,
  });
  const assignMember = useMutation({
    mutationFn: (input: AssignManagedTeamMemberInput) =>
      managedTeamApi.assignMember(input),
    onSuccess: (assignment) => {
      patch(assignment.member);
      if (conversationId) {
        void queryClient.invalidateQueries({
          queryKey: ["agents", "agent-task-scope", "conversation", conversationId],
        });
      }
      invalidate();
    },
  });
  const stopMember = useMutation({
    mutationFn: (input: StopManagedTeamMemberInput) => managedTeamApi.stopMember(input),
    onSuccess: (member) => {
      patch(member);
      invalidate();
    },
  });

  return { addMember, assignMember, stopMember };
}

export interface ManagedTeamRealtimeEvent {
  conversationId: string;
  parentRunId?: string | null;
  sequence?: number | null;
  member?: ManagedTeamMember | null;
}

/**
 * The chat-event seam is the sole realtime writer for Team status caches.
 * Status/roster components consume these queries and never reconcile events.
 */
export function reconcileManagedTeamEvent(
  queryClient: ReturnType<typeof useQueryClient>,
  activeConversationId: string | null | undefined,
  activeParentRunId: string | null | undefined,
  event: ManagedTeamRealtimeEvent,
): boolean {
  if (!activeConversationId || event.conversationId !== activeConversationId) {
    return false;
  }
  if (
    activeParentRunId &&
    event.parentRunId &&
    event.parentRunId !== activeParentRunId
  ) {
    return false;
  }
  const sequenceKey = managedTeamKeys.sequence(activeConversationId);
  const priorSequence = queryClient.getQueryData<number>(sequenceKey);
  if (
    event.sequence !== null &&
    event.sequence !== undefined &&
    priorSequence !== undefined &&
    event.sequence <= priorSequence
  ) {
    return false;
  }
  if (event.member) {
    const before = queryClient.getQueryData<ManagedTeamStatus | null>(
      managedTeamKeys.status(activeConversationId),
    );
    const next = patchMember(before, event.member);
    if (next === before) return false;
    queryClient.setQueryData(managedTeamKeys.status(activeConversationId), next);
  }
  if (event.sequence !== null && event.sequence !== undefined) {
    queryClient.setQueryData(sequenceKey, event.sequence);
  }
  return true;
}
