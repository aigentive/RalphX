import {
  closestCenter,
  DndContext,
  PointerSensor,
  useDraggable,
  useDroppable,
  useSensor,
  useSensors,
  type DragEndEvent,
} from "@dnd-kit/core";
import { CSS } from "@dnd-kit/utilities";
import {
  ChevronRight,
  Circle,
  CircleCheck,
  CircleDashed,
  CircleDot,
  MessageSquare,
  UserCheck,
} from "lucide-react";
import type { ReactNode } from "react";

import type { TicketingColumn, TicketSummary } from "@/api/ticketing";
import { Button } from "@/components/ui/button";
import { TicketAssigneeChip } from "./TicketAssigneeChip";
import { TicketLabels } from "./TicketLabels";
import { groupTicketsByStatus } from "./ticketing-read-state";
import { resolveTicketKanbanMove, ticketDragId } from "./ticketing-kanban-utils";
import { categoryToken, formatTicketDate, ticketButtonLabel, ticketKey } from "./ticketing-utils";

/** Colored status glyph (Linear-style) with the state name as tooltip/accessible name. */
function TicketStatusIcon({ state }: { state: TicketSummary["state"] }) {
  const iconClassName = "h-4 w-4";
  return (
    <span
      role="img"
      aria-label={`Status: ${state.name}`}
      title={state.name}
      className="inline-flex shrink-0"
      style={{ color: categoryToken(state.category) }}
    >
      {state.category === "done" ? (
        <CircleCheck className={iconClassName} />
      ) : state.category === "in_progress" ? (
        <CircleDot className={iconClassName} />
      ) : state.category === "other" ? (
        <CircleDashed className={iconClassName} />
      ) : (
        <Circle className={iconClassName} />
      )}
    </span>
  );
}

interface TicketListViewProps {
  tickets: TicketSummary[];
  columns?: TicketingColumn[] | undefined;
  hasNextPage: boolean;
  isFetchingNextPage: boolean;
  onLoadMore: () => void;
  onSelectTicket: (ticket: TicketSummary) => void;
  isUnread?: ((ticket: TicketSummary) => boolean) | undefined;
  canQuickAssign?: boolean | undefined;
  onQuickAssign?: ((ticket: TicketSummary) => void) | undefined;
}

/** Orange comment glyph shown when a ticket changed since the viewer last opened it. */
function UnreadCommentIndicator() {
  return (
    <span
      role="img"
      aria-label="Updated since you last opened this ticket"
      title="Updated since you last opened this ticket"
      className="inline-flex shrink-0"
      style={{ color: "var(--accent-primary)" }}
    >
      <MessageSquare className="h-3.5 w-3.5" aria-hidden="true" />
    </span>
  );
}

/**
 * Hover/focus-revealed "Assign to me" quick action. Rendered as a sibling overlay
 * (not nested inside the row/card button) to keep the markup a11y-valid.
 */
function QuickAssignButton({ onClick }: { onClick: () => void }) {
  return (
    <div className="pointer-events-none absolute inset-y-0 right-3 flex items-center opacity-0 transition-opacity group-hover:opacity-100 group-focus-within:opacity-100">
      <button
        type="button"
        className="pointer-events-auto inline-flex items-center gap-1 rounded-md px-2 py-1 text-xs font-medium"
        aria-label="Assign to me"
        title="Assign to me"
        onClick={(event) => {
          event.stopPropagation();
          onClick();
        }}
        style={{
          backgroundColor: "var(--bg-elevated)",
          borderColor: "var(--border-subtle)",
          borderStyle: "solid",
          borderWidth: "1px",
          boxShadow: "var(--shadow-sm)",
          color: "var(--text-primary)",
        }}
      >
        <UserCheck className="h-3.5 w-3.5" aria-hidden="true" />
        Assign to me
      </button>
    </div>
  );
}

function focusTicketRow(currentTarget: HTMLButtonElement, direction: 1 | -1) {
  const rows = Array.from(
    currentTarget
      .closest("[data-ticket-list]")
      ?.querySelectorAll<HTMLButtonElement>("[data-ticket-row]") ?? [],
  );
  const currentIndex = rows.indexOf(currentTarget);
  const nextRow = rows[currentIndex + direction];
  nextRow?.focus();
}

export function TicketListView({
  tickets,
  columns = [],
  hasNextPage,
  isFetchingNextPage,
  onLoadMore,
  onSelectTicket,
  isUnread,
  canQuickAssign,
  onQuickAssign,
}: TicketListViewProps) {
  const groups = groupTicketsByStatus(tickets, columns);
  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
      <div className="min-h-0 flex-1 overflow-auto" data-ticket-list>
        {groups.map((group) => (
          <section key={group.id}>
            <div
              className="sticky top-0 z-10 flex items-center gap-2 px-4 py-1.5 text-xs font-semibold"
              style={{
                backgroundColor: "var(--bg-surface)",
                borderBottomColor: "var(--border-subtle)",
                borderBottomStyle: "solid",
                borderBottomWidth: "1px",
              }}
            >
              <TicketStatusIcon state={{ id: group.id, name: group.name, category: group.category }} />
              <span className="text-[var(--text-secondary)]">{group.name}</span>
              <span className="text-[var(--text-muted)]">{group.tickets.length}</span>
            </div>
            {group.tickets.map((ticket) => (
              <div key={`${ticket.ref.provider}:${ticket.ref.id}`} className="group relative">
                <button
                  type="button"
                  data-ticket-row
                  className="grid w-full grid-cols-[88px_28px_minmax(200px,1fr)_140px_48px_96px] items-center gap-3 px-4 py-1.5 text-left text-sm hover:bg-[var(--bg-hover)] focus-visible:outline-none focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:-2px]"
                  aria-label={ticketButtonLabel(ticket)}
                  onClick={() => onSelectTicket(ticket)}
                  onKeyDown={(event) => {
                    if (event.key === "ArrowDown") {
                      event.preventDefault();
                      focusTicketRow(event.currentTarget, 1);
                    }
                    if (event.key === "ArrowUp") {
                      event.preventDefault();
                      focusTicketRow(event.currentTarget, -1);
                    }
                  }}
                  style={{
                    borderBottomColor: "var(--border-subtle)",
                    borderBottomStyle: "solid",
                    borderBottomWidth: "1px",
                    color: "var(--text-primary)",
                  }}
                >
                  <span className="font-mono text-xs text-[var(--text-secondary)]">{ticketKey(ticket.ref)}</span>
                  <TicketStatusIcon state={ticket.state} />
                  <span className="flex min-w-0 items-center gap-2">
                    {isUnread?.(ticket) && <UnreadCommentIndicator />}
                    <span className="truncate font-medium">{ticket.title}</span>
                    {ticket.project && (
                      <span className="shrink-0 truncate text-[11px] text-[var(--text-muted)]">{ticket.project}</span>
                    )}
                    <TicketLabels labels={ticket.labels} max={2} className="shrink-0 text-[var(--text-muted)]" />
                  </span>
                  <span className="min-w-0">
                    <TicketAssigneeChip person={ticket.assignee} />
                  </span>
                  <span className="text-xs text-[var(--text-secondary)]">
                    {ticket.associationCount > 0 ? `●${ticket.associationCount}` : "○"}
                  </span>
                  <span className="text-xs text-[var(--text-muted)]">{formatTicketDate(ticket.updatedAt)}</span>
                </button>
                {canQuickAssign && onQuickAssign && !ticket.assignee && (
                  <QuickAssignButton onClick={() => onQuickAssign(ticket)} />
                )}
              </div>
            ))}
          </section>
        ))}
      </div>
      {hasNextPage && (
        <div className="flex justify-center px-4 py-3">
          <Button type="button" variant="outline" size="sm" disabled={isFetchingNextPage} onClick={onLoadMore}>
            {isFetchingNextPage ? "Loading" : "Load more"}
          </Button>
        </div>
      )}
    </div>
  );
}

interface TicketKanbanViewProps {
  columns: TicketingColumn[];
  tickets: TicketSummary[];
  canMoveTickets?: boolean | undefined;
  onMoveTicket?: ((ticket: TicketSummary, column: TicketingColumn) => void) | undefined;
  onSelectTicket: (ticket: TicketSummary) => void;
  isUnread?: ((ticket: TicketSummary) => boolean) | undefined;
  canQuickAssign?: boolean | undefined;
  onQuickAssign?: ((ticket: TicketSummary) => void) | undefined;
}

function TicketColumn({
  column,
  children,
}: {
  column: TicketingColumn;
  children: ReactNode;
}) {
  const { setNodeRef, isOver } = useDroppable({ id: column.id });

  return (
    <section
      ref={setNodeRef}
      data-testid={`ticket-column-${column.id}`}
      className="flex h-full min-h-0 w-[280px] shrink-0 flex-col rounded-lg"
      style={{
        backgroundColor: isOver ? "var(--bg-hover)" : "var(--bg-surface)",
        borderColor: "var(--border-subtle)",
        borderStyle: "solid",
        borderWidth: "1px",
      }}
    >
      {children}
    </section>
  );
}

function TicketKanbanCard({
  ticket,
  canMove,
  unread,
  canQuickAssign,
  onQuickAssign,
  onSelectTicket,
}: {
  ticket: TicketSummary;
  canMove: boolean;
  unread: boolean;
  canQuickAssign: boolean;
  onQuickAssign?: ((ticket: TicketSummary) => void) | undefined;
  onSelectTicket: (ticket: TicketSummary) => void;
}) {
  const { attributes, listeners, setNodeRef, transform, isDragging } = useDraggable({
    id: ticketDragId(ticket),
    disabled: !canMove,
  });

  return (
    <div className="group relative">
    <button
      ref={setNodeRef}
      type="button"
      className="w-full rounded-md p-3 text-left hover:bg-[var(--bg-hover)] focus-visible:outline-none focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:2px]"
      aria-label={ticketButtonLabel(ticket)}
      onClick={() => onSelectTicket(ticket)}
      style={{
        backgroundColor: "var(--bg-elevated)",
        borderColor: "var(--border-subtle)",
        borderStyle: "solid",
        borderWidth: "1px",
        color: "var(--text-primary)",
        opacity: isDragging ? 0.72 : 1,
        transform: CSS.Translate.toString(transform),
      }}
      {...attributes}
      {...listeners}
    >
      <div className="flex items-center justify-between gap-2">
        <span className="flex items-center gap-1.5 font-mono text-xs text-[var(--text-muted)]">
          {unread && <UnreadCommentIndicator />}
          {ticketKey(ticket.ref)}
        </span>
        <span className="text-xs text-[var(--text-muted)]">
          {ticket.associationCount > 0 ? `●${ticket.associationCount}` : "○"}
        </span>
      </div>
      <p className="mt-2 line-clamp-2 text-sm font-medium">{ticket.title}</p>
      {ticket.labels.length > 0 && (
        <TicketLabels labels={ticket.labels} max={2} className="mt-2 text-[var(--text-muted)]" />
      )}
      <div className="mt-3 flex items-center justify-between gap-2 text-xs text-[var(--text-muted)]">
        <TicketAssigneeChip person={ticket.assignee} />
        <ChevronRight className="h-3.5 w-3.5 shrink-0" aria-hidden="true" />
      </div>
    </button>
      {canQuickAssign && onQuickAssign && !ticket.assignee && (
        <QuickAssignButton onClick={() => onQuickAssign(ticket)} />
      )}
    </div>
  );
}

export function TicketKanbanView({
  columns,
  tickets,
  canMoveTickets = false,
  onMoveTicket,
  onSelectTicket,
  isUnread,
  canQuickAssign,
  onQuickAssign,
}: TicketKanbanViewProps) {
  const sensors = useSensors(
    useSensor(PointerSensor, {
      activationConstraint: { distance: 6 },
    }),
  );
  const effectiveColumns = columns.length > 0
    ? columns
    : Array.from(
        new Map(tickets.map((ticket) => [ticket.state.id, {
          id: ticket.state.id,
          name: ticket.state.name,
          category: ticket.state.category,
          order: 0,
        }])).values(),
      );
  const canMove = canMoveTickets && Boolean(onMoveTicket);

  function handleDragEnd(event: DragEndEvent) {
    const move = resolveTicketKanbanMove(
      String(event.active.id),
      event.over ? String(event.over.id) : null,
      tickets,
      effectiveColumns,
    );
    if (!move) {
      return;
    }
    onMoveTicket?.(move.ticket, move.column);
  }

  return (
    <DndContext sensors={sensors} collisionDetection={closestCenter} onDragEnd={handleDragEnd}>
      <div className="flex min-h-0 flex-1 gap-3 overflow-x-auto p-4">
        {effectiveColumns.map((column) => {
          const columnTickets = tickets.filter((ticket) => ticket.state.id === column.id);
          return (
            <TicketColumn key={column.id} column={column}>
              <div
                className="flex items-center justify-between px-3 py-2"
                style={{
                  borderBottomColor: "var(--border-subtle)",
                  borderBottomStyle: "solid",
                  borderBottomWidth: "1px",
                }}
              >
                <div className="flex items-center gap-2">
                  <span
                    className="h-2 w-2 rounded-full"
                    aria-hidden="true"
                    style={{ backgroundColor: categoryToken(column.category) }}
                  />
                  <h2 className="text-sm font-semibold text-[var(--text-primary)]">{column.name}</h2>
                </div>
                <span className="text-xs text-[var(--text-muted)]">{columnTickets.length}</span>
              </div>
              <div className="flex min-h-0 flex-1 flex-col gap-2 overflow-y-auto p-2">
                {columnTickets.map((ticket) => (
                  <TicketKanbanCard
                    key={`${ticket.ref.provider}:${ticket.ref.id}`}
                    ticket={ticket}
                    canMove={canMove}
                    unread={isUnread?.(ticket) ?? false}
                    canQuickAssign={canQuickAssign ?? false}
                    onQuickAssign={onQuickAssign}
                    onSelectTicket={onSelectTicket}
                  />
                ))}
              </div>
            </TicketColumn>
          );
        })}
      </div>
    </DndContext>
  );
}

export function TicketKanbanShell({ columns }: { columns: TicketingColumn[] }) {
  const shellColumns = columns.length > 0
    ? columns.slice(0, 4)
    : [
        { id: "todo", name: "To Do", category: "todo" as const, order: 0 },
        { id: "in_progress", name: "In Progress", category: "in_progress" as const, order: 1 },
        { id: "done", name: "Done", category: "done" as const, order: 2 },
      ];

  return (
    <div className="flex min-h-0 flex-1 gap-3 overflow-hidden p-4" aria-label="Loading kanban view">
      {shellColumns.map((column) => (
        <section
          key={column.id}
          className="flex min-h-[320px] w-[280px] shrink-0 flex-col rounded-lg"
          style={{
            backgroundColor: "var(--bg-surface)",
            borderColor: "var(--border-subtle)",
            borderStyle: "solid",
            borderWidth: "1px",
          }}
        >
          <div className="px-3 py-2">
            <h2 className="text-sm font-semibold text-[var(--text-primary)]">{column.name}</h2>
          </div>
          <div className="space-y-2 p-2">
            {[0, 1, 2].map((item) => (
              <div
                key={item}
                className="h-20 rounded-md"
                style={{
                  backgroundColor: "var(--bg-elevated)",
                  borderColor: "var(--border-subtle)",
                  borderStyle: "solid",
                  borderWidth: "1px",
                }}
              />
            ))}
          </div>
        </section>
      ))}
    </div>
  );
}
