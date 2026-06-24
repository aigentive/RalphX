import type { TicketingColumn, TicketSummary } from "@/api/ticketing";

export function ticketDragId(ticket: TicketSummary): string {
  return `${ticket.ref.provider}:${ticket.ref.id}`;
}

export function resolveTicketKanbanMove(
  activeId: string,
  overId: string | null | undefined,
  tickets: TicketSummary[],
  columns: TicketingColumn[],
): { ticket: TicketSummary; column: TicketingColumn } | null {
  if (!overId) {
    return null;
  }
  const ticket = tickets.find((item) => ticketDragId(item) === activeId);
  const column = columns.find((item) => item.id === overId);
  if (!ticket || !column || ticket.state.id === column.id) {
    return null;
  }
  return { ticket, column };
}
