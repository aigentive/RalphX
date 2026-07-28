export interface ManagedTeamSession {
  id: string;
  projectId: string;
  coordinatorConversationId: string;
  status: string;
  configuredConcurrency: number;
  effectiveConcurrency: number;
  automaticWakeLimit: number;
  version: number;
  createdAt: string;
  updatedAt: string;
}

export interface ManagedTeamMember {
  id: string;
  teamId: string;
  name: string;
  normalizedName: string;
  canonicalAgentName: string;
  roleSummary: string;
  status: string;
  generation: number;
}

export interface ManagedTeamStatus {
  session: ManagedTeamSession;
  members: ManagedTeamMember[];
  usage: ManagedTeamUsage;
}

export interface ManagedTeamUsage {
  tokens: number;
  costMicros: number;
  members: ManagedTeamMemberUsage[];
}

export interface ManagedTeamMemberUsage {
  memberId: string | null;
  tokens: number;
  costMicros: number;
}

export interface ManagedTeamAssignment {
  assignmentId: string;
  agentRunId: string;
  member: ManagedTeamMember;
}

/** Trusted runtime identity is carried only in HTTP headers, never in the body. */
export interface ManagedTeamAuthority {
  conversationId: string;
  agentRunId: string;
}

export interface EnsureManagedTeamInput {
  conversationId: string;
  projectId: string;
}

export interface AddManagedTeamMemberInput {
  authority: ManagedTeamAuthority;
  name: string;
  canonicalAgentName: string;
  roleSummary: string;
  harness?: string | null;
  logicalModel?: string | null;
  logicalEffort?: string | null;
}

export interface AssignManagedTeamMemberInput {
  authority: ManagedTeamAuthority;
  memberName: string;
  taskRef: string;
  workClassification: string;
  writablePaths?: string[];
  generatedOutputs?: string[];
  resourceLocks?: string[];
}

export interface StopManagedTeamMemberInput {
  authority: ManagedTeamAuthority;
  memberName: string;
}

export interface ExitManagedTeamInput {
  authority: ManagedTeamAuthority;
  action: "suspend" | "drain_and_close";
}
