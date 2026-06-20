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
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) {
    return "Unknown";
  }
  return new Intl.DateTimeFormat(undefined, {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
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

export function providerLabel(provider: string): string {
  return provider === "jira" ? "Jira" : "Linear";
}
