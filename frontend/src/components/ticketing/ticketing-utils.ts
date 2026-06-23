import type { TicketRef, TicketStateCategory, TicketSummary } from "@/api/ticketing";

export function ticketKey(ref: TicketRef): string {
  return ref.key ?? ref.id;
}

export function ticketButtonLabel(ticket: TicketSummary): string {
  return `${ticketKey(ticket.ref)} ${ticket.title}`;
}

export function formatTicketDate(value: string | null | undefined): string {
  if (!value) {
    return "Unknown";
  }
  const trimmed = value.trim();
  const date = /^\d+$/.test(trimmed) ? new Date(Number(trimmed)) : new Date(trimmed);
  if (Number.isNaN(date.getTime())) {
    return "Unknown";
  }
  // Minimal, scannable date (e.g. "Jan 28") — no time-of-day. Include the year
  // only when it differs from the current year, to disambiguate older tickets
  // without cluttering the common (current-year) case.
  const sameYear = date.getFullYear() === new Date().getFullYear();
  return new Intl.DateTimeFormat(undefined, {
    month: "short",
    day: "numeric",
    ...(sameYear ? {} : { year: "numeric" }),
  }).format(date);
}

export function categoryToken(category: TicketStateCategory): string {
  switch (category) {
    case "done":
      return "var(--status-success)";
    case "in_progress":
      return "var(--accent-primary)";
    case "other":
      return "var(--status-warning)";
    case "todo":
    default:
      return "var(--text-muted)";
  }
}

const PROVIDER_LABELS: Record<string, string> = {
  jira: "Jira",
  linear: "Linear",
  clickup: "ClickUp",
};

export function providerLabel(provider: string): string {
  // 3-way map: a binary ternary would silently render ClickUp tickets as
  // "Linear". Unknown providers get a neutral, non-misleading fallback.
  return PROVIDER_LABELS[provider] ?? "Provider";
}
