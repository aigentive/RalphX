import type { z } from "zod";

import type {
  ManagedTeamAssignment,
  ManagedTeamMember,
  ManagedTeamSession,
  ManagedTeamStatus,
} from "./managed-team.types";
import type {
  ManagedTeamAssignmentSchema,
  ManagedTeamMemberSchema,
  ManagedTeamSessionSchema,
  ManagedTeamStatusSchema,
} from "./managed-team.schemas";

type RawSession = z.infer<typeof ManagedTeamSessionSchema>;
type RawMember = z.infer<typeof ManagedTeamMemberSchema>;
type RawStatus = z.infer<typeof ManagedTeamStatusSchema>;
type RawAssignment = z.infer<typeof ManagedTeamAssignmentSchema>;

export function transformManagedTeamSession(raw: RawSession): ManagedTeamSession {
  return { ...raw };
}

export function transformManagedTeamMember(raw: RawMember): ManagedTeamMember {
  return { ...raw };
}

export function transformManagedTeamStatus(raw: RawStatus): ManagedTeamStatus {
  return {
    session: transformManagedTeamSession(raw.session),
    members: raw.members.map(transformManagedTeamMember),
    usage: raw.usage,
  };
}

export function transformManagedTeamAssignment(
  raw: RawAssignment,
): ManagedTeamAssignment {
  return { ...raw, member: transformManagedTeamMember(raw.member) };
}
