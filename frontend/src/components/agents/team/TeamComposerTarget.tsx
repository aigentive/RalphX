import type { TeamMessageTarget } from "@/api/chat";
import type { ManagedTeamMember } from "@/api/managed-team";

export function TeamComposerTarget({
  members,
  value,
  onValueChange,
  disabled = false,
}: {
  members: readonly ManagedTeamMember[];
  value: TeamMessageTarget | null;
  onValueChange: (target: TeamMessageTarget | null) => void;
  disabled?: boolean;
}) {
  const selectedValue =
    value?.kind === "member"
      ? `member:${value.memberName ?? ""}`
      : (value?.kind ?? "coordinator");

  return (
    <label
      className="flex min-w-0 items-center gap-1.5 text-xs"
      style={{ color: "var(--text-muted)" }}
      data-testid="team-composer-target"
    >
      <span className="shrink-0 font-medium">To</span>
      <select
        value={selectedValue}
        disabled={disabled}
        onChange={(event) => {
          const next = event.target.value;
          if (next === "coordinator") {
            onValueChange(null);
          } else if (next === "broadcast") {
            onValueChange({ kind: "broadcast" });
          } else {
            onValueChange({ kind: "member", memberName: next.slice("member:".length) });
          }
        }}
        aria-label="Team message recipient"
        className="h-8 min-w-0 max-w-40 rounded-md border px-2 text-xs outline-none disabled:opacity-50"
        style={{
          color: "var(--text-primary)",
          backgroundColor: "var(--bg-base)",
          borderColor: "var(--form-border)",
          borderStyle: "solid",
          borderWidth: 1,
        }}
      >
        <option value="coordinator">Coordinator</option>
        <option value="broadcast">Broadcast</option>
        {members.map((member) => (
          <option key={member.id} value={`member:${member.name}`}>
            {member.name}
          </option>
        ))}
      </select>
    </label>
  );
}
