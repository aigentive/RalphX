import type { ManagedTeamStatus } from "@/api/managed-team";

export function TeamUsage({ status }: { status: ManagedTeamStatus }) {
  const activeMembers = status.members.filter((member) => member.status === "working").length;
  const cards = [
    ["Active", `${activeMembers}/${status.session.effectiveConcurrency}`],
    ["Configured", String(status.session.configuredConcurrency)],
    ["Tokens", status.usage.tokens.toLocaleString()],
    ["Cost", `$${(status.usage.costMicros / 1_000_000).toFixed(4)}`],
  ] as const;

  return (
    <div className="grid grid-cols-2 gap-2" data-testid="team-usage">
      {cards.map(([label, value]) => (
        <div
          key={label}
          className="rounded-lg border px-2.5 py-2"
          style={{
            backgroundColor: "var(--bg-base)",
            borderColor: "var(--border-subtle)",
            borderStyle: "solid",
            borderWidth: 1,
          }}
        >
          <p className="text-[0.6875rem] uppercase tracking-wide" style={{ color: "var(--text-muted)" }}>
            {label}
          </p>
          <p className="mt-0.5 text-sm font-semibold" style={{ color: "var(--text-primary)" }}>
            {value}
          </p>
        </div>
      ))}
    </div>
  );
}
