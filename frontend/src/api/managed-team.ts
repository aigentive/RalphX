import { backendFetch } from "@/api/backend";

import {
  ManagedTeamAssignmentSchema,
  ManagedTeamMemberSchema,
  ManagedTeamRosterSchema,
  ManagedTeamSessionSchema,
  ManagedTeamStatusSchema,
} from "./managed-team.schemas";
import {
  transformManagedTeamAssignment,
  transformManagedTeamMember,
  transformManagedTeamSession,
  transformManagedTeamStatus,
} from "./managed-team.transforms";
import type {
  AddManagedTeamMemberInput,
  AssignManagedTeamMemberInput,
  EnsureManagedTeamInput,
  ExitManagedTeamInput,
  ManagedTeamAssignment,
  ManagedTeamAuthority,
  ManagedTeamMember,
  ManagedTeamSession,
  ManagedTeamStatus,
  StopManagedTeamMemberInput,
} from "./managed-team.types";

export type {
  AddManagedTeamMemberInput,
  AssignManagedTeamMemberInput,
  EnsureManagedTeamInput,
  ExitManagedTeamInput,
  ManagedTeamAssignment,
  ManagedTeamAuthority,
  ManagedTeamMember,
  ManagedTeamMemberUsage,
  ManagedTeamSession,
  ManagedTeamStatus,
  ManagedTeamUsage,
  StopManagedTeamMemberInput,
} from "./managed-team.types";
export {
  ManagedTeamAssignmentSchema,
  ManagedTeamMemberSchema,
  ManagedTeamRosterSchema,
  ManagedTeamSessionSchema,
  ManagedTeamStatusSchema,
} from "./managed-team.schemas";
export {
  transformManagedTeamAssignment,
  transformManagedTeamMember,
  transformManagedTeamSession,
  transformManagedTeamStatus,
} from "./managed-team.transforms";

function authorityHeaders(authority: ManagedTeamAuthority): HeadersInit {
  return {
    "x-ralphx-conversation-id": authority.conversationId,
    "x-ralphx-agent-run-id": authority.agentRunId,
  };
}

async function requestJson(
  endpoint: string,
  init: RequestInit,
): Promise<unknown> {
  // Through the env-aware seam: local stays byte-identical fetch; a remote environment
  // routes to the host proxy (and fails closed on unmounted routes) instead of silently
  // hitting this device's localhost.
  const response = await backendFetch(endpoint, init);
  if (response.ok) {
    if (response.status === 204) return null;
    return response.json();
  }
  const payload = (await response.json().catch(() => null)) as {
    error?: unknown;
    message?: unknown;
  } | null;
  const detail =
    typeof payload?.error === "string"
      ? payload.error
      : typeof payload?.message === "string"
        ? payload.message
        : response.statusText;
  throw new Error(`Managed Team request failed: ${response.status} ${detail}`);
}

function postJson(
  endpoint: string,
  body: Record<string, unknown>,
  authority?: ManagedTeamAuthority,
): Promise<unknown> {
  return requestJson(endpoint, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      ...(authority ? authorityHeaders(authority) : {}),
    },
    body: JSON.stringify(body),
  });
}

export const managedTeamApi = {
  async ensure(input: EnsureManagedTeamInput): Promise<ManagedTeamSession> {
    const raw = await postJson("managed_team/ensure", {
      conversationId: input.conversationId,
      projectId: input.projectId,
    });
    return transformManagedTeamSession(ManagedTeamSessionSchema.parse(raw));
  },

  async getStatus(conversationId: string): Promise<ManagedTeamStatus | null> {
    const raw = await requestJson(
      `managed_team/status/${encodeURIComponent(conversationId)}`,
      { method: "GET" },
    );
    return raw === null ? null : transformManagedTeamStatus(ManagedTeamStatusSchema.parse(raw));
  },

  async getRoster(teamId: string): Promise<ManagedTeamMember[]> {
    const raw = await requestJson(
      `managed_team/roster/${encodeURIComponent(teamId)}`,
      { method: "GET" },
    );
    return ManagedTeamRosterSchema.parse(raw).map(transformManagedTeamMember);
  },

  async getIdleMembers(
    authority: ManagedTeamAuthority,
  ): Promise<ManagedTeamMember[]> {
    const raw = await requestJson("managed_team/members/idle", {
      method: "GET",
      headers: authorityHeaders(authority),
    });
    return ManagedTeamRosterSchema.parse(raw).map(transformManagedTeamMember);
  },

  async addMember(
    input: AddManagedTeamMemberInput,
  ): Promise<ManagedTeamMember> {
    const raw = await postJson(
      "managed_team/member",
      {
        name: input.name,
        canonical_agent_name: input.canonicalAgentName,
        role_summary: input.roleSummary,
        ...(input.harness ? { harness: input.harness } : {}),
        ...(input.logicalModel ? { logical_model: input.logicalModel } : {}),
        ...(input.logicalEffort ? { logical_effort: input.logicalEffort } : {}),
      },
      input.authority,
    );
    return transformManagedTeamMember(ManagedTeamMemberSchema.parse(raw));
  },

  async assignMember(
    input: AssignManagedTeamMemberInput,
  ): Promise<ManagedTeamAssignment> {
    const raw = await postJson(
      "managed_team/member/assign",
      {
        member_name: input.memberName,
        task_ref: input.taskRef,
        work_classification: input.workClassification,
        writable_paths: input.writablePaths ?? [],
        generated_outputs: input.generatedOutputs ?? [],
        resource_locks: input.resourceLocks ?? [],
      },
      input.authority,
    );
    return transformManagedTeamAssignment(ManagedTeamAssignmentSchema.parse(raw));
  },

  async stopMember(input: StopManagedTeamMemberInput): Promise<ManagedTeamMember> {
    const raw = await postJson(
      "managed_team/member/stop",
      { member_name: input.memberName },
      input.authority,
    );
    return transformManagedTeamMember(ManagedTeamMemberSchema.parse(raw));
  },

  async exit(input: ExitManagedTeamInput): Promise<void> {
    await postJson("managed_team/exit", { action: input.action }, input.authority);
  },
} as const;
