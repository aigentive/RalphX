import type { ManagedTeamMember } from "@/api/managed-team";
import { StatusPill, type StatusPillTone } from "@/components/ui/status-pill";

function memberTone(status: string): StatusPillTone {
  if (status === "working") return "accent";
  if (status === "idle") return "success";
  if (status === "failed" || status === "stopped") return "error";
  if (status.includes("awaiting") || status === "stopping") return "warning";
  return "neutral";
}

export function TeamRoster({ members }: { members: readonly ManagedTeamMember[] }) {
  if (members.length === 0) {
    return (
      <p className="text-sm" style={{ color: "var(--text-muted)" }}>
        Add standing members when the coordinator is running.
      </p>
    );
  }

  return (
    <div className="space-y-2" data-testid="team-roster">
      {members.map((member) => (
        <div
          key={member.id}
          className="flex items-center justify-between gap-3 rounded-lg border px-3 py-2"
          style={{
            backgroundColor: "var(--bg-base)",
            borderColor: "var(--border-subtle)",
            borderStyle: "solid",
            borderWidth: 1,
          }}
        >
          <div className="min-w-0">
            <p className="truncate text-sm font-medium" style={{ color: "var(--text-primary)" }}>
              {member.name}
            </p>
            <p className="truncate text-xs" style={{ color: "var(--text-muted)" }}>
              {member.roleSummary}
            </p>
          </div>
          <StatusPill
            label={member.status.replace(/_/g, " ")}
            tone={memberTone(member.status)}
            live={member.status === "working"}
            ariaLabel={`${member.name}: ${member.status}`}
          />
        </div>
      ))}
    </div>
  );
}
