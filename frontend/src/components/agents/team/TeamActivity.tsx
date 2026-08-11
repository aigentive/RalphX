import type { ManagedTeamMember } from "@/api/managed-team";

export function TeamActivity({ members }: { members: readonly ManagedTeamMember[] }) {
  const active = members.filter((member) => member.status !== "idle");
  return (
    <div className="space-y-2" data-testid="team-activity">
      {active.length === 0 ? (
        <p className="text-sm" style={{ color: "var(--text-muted)" }}>
          No member turn is active. Team messages and results appear in the chat transcript.
        </p>
      ) : (
        active.map((member) => (
          <div key={member.id} className="flex items-center justify-between gap-3 text-sm">
            <span className="min-w-0 truncate" style={{ color: "var(--text-primary)" }}>
              {member.name}
            </span>
            <span className="shrink-0 text-xs capitalize" style={{ color: "var(--text-muted)" }}>
              {member.status.replace(/_/g, " ")}
            </span>
          </div>
        ))
      )}
    </div>
  );
}
