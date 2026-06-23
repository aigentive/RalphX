import { UserRound } from "lucide-react";
import { useState } from "react";

import type { TicketingPerson } from "@/api/ticketing";

import { assigneeInitials } from "./ticketing-presentation";

interface TicketAssigneeChipProps {
  person: TicketingPerson | null | undefined;
  size?: "sm" | "md";
  unassignedLabel?: string;
  className?: string;
  unassignedTone?: "muted" | "secondary";
}

/**
 * Presentational assignee chip: avatar (image or initials fallback) plus name.
 * Renders a muted placeholder when the ticket is unassigned. The visible name
 * is the accessible name; email (when present) is surfaced via the title tooltip.
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
    <span className={`${wrapperClass} text-[var(--text-secondary)]`} title={tooltip}>
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
      <span className="truncate">{name}</span>
    </span>
  );
}
