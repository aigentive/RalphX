import { UserRound } from "lucide-react";
import { useState } from "react";

import type { TicketingPerson } from "@/api/ticketing";

import { assigneeInitials } from "./ticketing-presentation";

interface TicketAssigneeChipProps {
  person: TicketingPerson | null | undefined;
  size?: "sm" | "md";
  unassignedLabel?: string;
  className?: string | undefined;
  unassignedTone?: "muted" | "secondary" | undefined;
}

/**
 * Presentational assignee chip: avatar (image or initials fallback) with the
 * assignee surfaced through native hover/accessibility text.
 */
export function TicketAssigneeChip({
  person,
  size = "sm",
  unassignedLabel = "Unassigned",
  className,
  unassignedTone = "muted",
}: TicketAssigneeChipProps) {
  // Track an avatar URL that failed to load so we fall back to initials instead
  // of a blank circle (provider avatars frequently fail under WKWebView).
  const [failedAvatarUrl, setFailedAvatarUrl] = useState<string | null>(null);
  const avatarSize = size === "md" ? "h-6 w-6" : "h-5 w-5";
  const textSize = size === "md" ? "text-sm" : "text-xs";
  const wrapperClass = `inline-flex min-w-0 items-center gap-1.5 ${textSize} ${className ?? ""}`.trim();
  const unassignedColor = unassignedTone === "secondary" ? "var(--text-secondary)" : "var(--text-muted)";

  if (!person) {
    return (
      <span className={wrapperClass} style={{ color: unassignedColor }}>
        <span
          className={`inline-flex ${avatarSize} shrink-0 items-center justify-center rounded-full`}
          aria-hidden="true"
          style={{
            backgroundColor: "var(--bg-surface)",
            borderColor: "var(--border-subtle)",
            borderStyle: "dashed",
            borderWidth: "1px",
            color: unassignedColor,
          }}
        >
          <UserRound className="h-3 w-3" aria-hidden="true" />
        </span>
        <span className="truncate">{unassignedLabel}</span>
      </span>
    );
  }

  const name = (person.name ?? "").trim() || "Unknown";
  const tooltip = person.email ? `${name} · ${person.email}` : name;

  return (
    <span
      className={`${wrapperClass} text-[var(--text-secondary)]`}
      title={tooltip}
      aria-label={tooltip}
    >
      {person.avatarUrl && person.avatarUrl !== failedAvatarUrl ? (
        <img
          src={person.avatarUrl}
          alt=""
          aria-hidden="true"
          loading="lazy"
          onError={() => setFailedAvatarUrl(person.avatarUrl ?? null)}
          className={`${avatarSize} shrink-0 rounded-full object-cover`}
          style={{
            borderColor: "var(--border-subtle)",
            borderStyle: "solid",
            borderWidth: "1px",
          }}
        />
      ) : (
        <span
          className={`inline-flex ${avatarSize} shrink-0 items-center justify-center rounded-full text-[10px] font-semibold`}
          aria-hidden="true"
          style={{
            backgroundColor: "var(--accent-muted)",
            borderColor: "var(--accent-border)",
            borderStyle: "solid",
            borderWidth: "1px",
            color: "var(--text-primary)",
          }}
        >
          {assigneeInitials(name)}
        </span>
      )}
    </span>
  );
}

export function TicketAssigneeChips({
  people,
  size = "sm",
  maxVisible = 3,
  unassignedLabel = "Unassigned",
  className,
  unassignedTone,
}: {
  people: TicketingPerson[];
  size?: "sm" | "md";
  maxVisible?: number;
  unassignedLabel?: string;
  className?: string | undefined;
  unassignedTone?: "muted" | "secondary" | undefined;
}) {
  if (people.length === 0) {
    return (
      <TicketAssigneeChip
        person={null}
        size={size}
        unassignedLabel={unassignedLabel}
        className={className}
        unassignedTone={unassignedTone}
      />
    );
  }
  const visible = people.slice(0, maxVisible);
  const overflow = people.length - visible.length;
  return (
    <span className={`inline-flex min-w-0 items-center gap-1 ${className ?? ""}`.trim()}>
      {visible.map((person, index) => (
        <TicketAssigneeChip
          key={`${person.id ?? person.email ?? person.name}:${index}`}
          person={person}
          size={size}
          className="min-w-0"
          unassignedTone={unassignedTone}
        />
      ))}
      {overflow > 0 && (
        <span
          className="shrink-0 rounded-full px-1.5 py-0.5 text-[10px] font-medium text-[var(--text-secondary)]"
          title={people.slice(visible.length).map((person) => person.name).join(", ")}
          style={{
            backgroundColor: "var(--bg-surface)",
            borderColor: "var(--border-subtle)",
            borderStyle: "solid",
            borderWidth: "1px",
          }}
        >
          +{overflow}
        </span>
      )}
    </span>
  );
}
