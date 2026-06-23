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
  Briefcase,
  ChevronDown,
  ChevronRight,
  Circle,
  CircleCheck,
  CircleDashed,
  CircleDot,
  GitPullRequestArrow,
  MessageSquare,
  UserCheck,
} from "lucide-react";
import { useState, type ReactNode } from "react";

import type { TicketingColumn, TicketSummary } from "@/api/ticketing";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { TicketAssigneeChip } from "./TicketAssigneeChip";
import { TicketLabels } from "./TicketLabels";
import { openExternalTicketUrl } from "./ticketing-open-external";
import { groupTicketsByStatus } from "./ticketing-read-state";
import { resolveTicketKanbanMove, ticketDragId } from "./ticketing-kanban-utils";
import { categoryToken, formatTicketDate, ticketButtonLabel, ticketKey } from "./ticketing-utils";

/**
 * Shared grid template for list rows and the column-aligned overlays.
 * Columns: Key | Status | Title | Assignee | RX | PR | Updated.
 * The PR column intentionally sits AFTER the RX (suitcase) column.
 */
const TICKET_ROW_GRID =
  "grid grid-cols-[88px_28px_minmax(200px,1fr)_140px_48px_64px_minmax(0,120px)_96px] items-center gap-3 px-4";

/** Status strings that mean a PR is still live (open/draft) → color the icon green. */
function isLivePrStatus(status: string | null | undefined): boolean {
  if (status == null) {
    // No status but a representative PR number means an open PR (backend default).
    return true;
  }
  const normalized = status.trim().toLowerCase();
  return normalized === "" || normalized === "open" || normalized === "draft";
}

/** Glyph picker shared by the read-only status icon and the interactive trigger. */
function statusGlyph(category: TicketSummary["state"]["category"], className: string) {
  if (category === "done") {
    return <CircleCheck className={className} />;
  }
  if (category === "in_progress") {
    return <CircleDot className={className} />;
  }
  if (category === "other") {
    return <CircleDashed className={className} />;
  }
  return <Circle className={className} />;
}

/** Colored status glyph (Linear-style) with the state name as tooltip/accessible name. */
function TicketStatusIcon({ state }: { state: TicketSummary["state"] }) {
  return (
    <span
      role="img"
      aria-label={`Status: ${state.name}`}
      title={state.name}
      className="inline-flex shrink-0"
      style={{ color: categoryToken(state.category) }}
    >
      {statusGlyph(state.category, "h-4 w-4")}
    </span>
  );
}

/**
 * Interactive status control for the list. Opens a dropdown of the available
 * status columns and moves the ticket on select via the shared `onMoveTicket`
 * pipeline (the same one the kanban uses). Rendered as a sibling overlay aligned
 * to the row's Status column (not nested inside the row button) so the markup
 * stays a11y-valid. The trigger is icon-only, so it carries an explicit
 * accessible name plus the app Tooltip (native title alone is not sufficient —
 * see .claude/rules/icon-only-buttons.md).
 */
function TicketStatusControl({
  ticket,
  columns,
  onMoveTicket,
}: {
  ticket: TicketSummary;
  columns: TicketingColumn[];
  onMoveTicket: (ticket: TicketSummary, column: TicketingColumn) => void;
}) {
  const label = `Change status (current: ${ticket.state.name})`;
  return (
    <DropdownMenu>
      <Tooltip>
        <TooltipTrigger asChild>
          <DropdownMenuTrigger asChild>
            <button
              type="button"
              aria-label={label}
              className="pointer-events-auto inline-flex shrink-0 items-center justify-center rounded-md p-0.5 hover:bg-[var(--bg-hover)] focus-visible:outline-none focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:1px]"
              onClick={(event) => event.stopPropagation()}
              style={{ color: categoryToken(ticket.state.category) }}
            >
              {statusGlyph(ticket.state.category, "h-4 w-4")}
            </button>
          </DropdownMenuTrigger>
        </TooltipTrigger>
        <TooltipContent>{label}</TooltipContent>
      </Tooltip>
      <DropdownMenuContent align="start" onClick={(event) => event.stopPropagation()}>
        {columns.map((column) => (
          <DropdownMenuItem
            key={column.id}
            onSelect={() => onMoveTicket(ticket, column)}
          >
            <span
              className="mr-2 inline-block h-2 w-2 shrink-0 rounded-full"
              aria-hidden="true"
              style={{ backgroundColor: categoryToken(column.category) }}
            />
            {column.name}
          </DropdownMenuItem>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
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
  canMoveTickets?: boolean | undefined;
  onMoveTicket?: ((ticket: TicketSummary, column: TicketingColumn) => void) | undefined;
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

/** RalphX work badge: number of agent conversations/tasks linked to this ticket. */
function TicketAssociationBadge({ count }: { count: number }) {
  const hasConversations = count > 0;
  if (!hasConversations) {
    return (
      <span
        role="img"
        aria-label="No linked RalphX work"
        title="No linked RalphX work"
        className="inline-flex text-[var(--text-muted)] opacity-40"
      >
        <Briefcase className="h-3.5 w-3.5" aria-hidden="true" />
      </span>
    );
  }
  const conversationLabel = `${count} RalphX conversation${count === 1 ? "" : "s"}`;
  return (
    <span className="inline-flex items-center gap-2 text-xs font-medium">
      <span
        role="img"
        aria-label={conversationLabel}
        title={`${conversationLabel} (open the ticket to view them)`}
        className="inline-flex items-center gap-1"
        style={{ color: "var(--status-info)" }}
      >
        <Briefcase className="h-3.5 w-3.5" aria-hidden="true" />
        {count}
      </span>
    </span>
  );
}

/**
 * Interactive "open this ticket's representative PR in the browser" control.
 * Shows the representative PR regardless of status; the icon is colored green
 * for a live (open/draft) PR and muted for a terminal one (merged/closed). The
 * accessible name and tooltip name the status for non-live PRs. Rendered as a
 * sibling overlay aligned to the list row's PR column (not nested inside the row
 * button) so the markup stays a11y-valid. Icon-primary, so it carries an
 * explicit accessible name plus the app Tooltip (native title alone is not
 * sufficient — see .claude/rules/icon-only-buttons.md).
 */
function TicketPrOpenControl({
  prNumber,
  prUrl,
  prStatus,
}: {
  prNumber: number;
  prUrl: string;
  prStatus: string | null | undefined;
}) {
  const live = isLivePrStatus(prStatus);
  const label = live
    ? `Open pull request #${prNumber} in browser`
    : `Pull request #${prNumber} (${prStatus?.trim() ?? "closed"}) in browser`;
  const color = live ? "var(--status-success)" : "var(--text-muted)";
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <button
          type="button"
          aria-label={label}
          className="pointer-events-auto inline-flex items-center gap-1 rounded-md px-1.5 py-0.5 text-xs font-medium hover:bg-[var(--bg-hover)] focus-visible:outline-none focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:1px]"
          onClick={(event) => {
            event.stopPropagation();
            void openExternalTicketUrl(prUrl);
          }}
          style={{ color }}
        >
          <GitPullRequestArrow className="h-3.5 w-3.5" aria-hidden="true" />
          <span>#{prNumber}</span>
        </button>
      </TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
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
  canMoveTickets,
  onMoveTicket,
}: TicketListViewProps) {
  const groups = groupTicketsByStatus(tickets, columns);
  // The interactive status control only appears when ticket moves are writable
  // and we have both a move handler and status columns to choose from.
  const canMoveStatus = Boolean(canMoveTickets && onMoveTicket && columns.length > 0);
  const [collapsed, setCollapsed] = useState<Set<string>>(() => new Set());
  const toggleGroup = (id: string) =>
    setCollapsed((previous) => {
      const next = new Set(previous);
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
      }
      return next;
    });
  return (
    <div className="flex h-full min-h-0 w-full flex-1 flex-col overflow-hidden">
      <div className="min-h-0 flex-1 overflow-auto overscroll-contain" data-ticket-list>
        {groups.map((group) => (
          <section key={group.id}>
            <button
              type="button"
              className="sticky top-0 z-10 flex w-full items-center gap-2 px-4 py-1.5 text-left text-xs font-semibold hover:bg-[var(--bg-hover)]"
              style={{
                backgroundColor: "var(--bg-surface)",
                borderBottomColor: "var(--border-subtle)",
                borderBottomStyle: "solid",
                borderBottomWidth: "1px",
              }}
              aria-expanded={!collapsed.has(group.id)}
              onClick={() => toggleGroup(group.id)}
            >
              {collapsed.has(group.id) ? (
                <ChevronRight className="h-3.5 w-3.5 text-[var(--text-muted)]" aria-hidden="true" />
              ) : (
                <ChevronDown className="h-3.5 w-3.5 text-[var(--text-muted)]" aria-hidden="true" />
              )}
              <TicketStatusIcon state={{ id: group.id, name: group.name, category: group.category }} />
              <span className="text-[var(--text-secondary)]">{group.name}</span>
              <span className="text-[var(--text-muted)]">{group.tickets.length}</span>
            </button>
            {!collapsed.has(group.id) && group.tickets.map((ticket) => (
              <div key={`${ticket.ref.provider}:${ticket.ref.id}`} className="group relative">
                <button
                  type="button"
                  data-ticket-row
                  className={`${TICKET_ROW_GRID} w-full py-1.5 text-left text-sm hover:bg-[var(--bg-hover)] focus-visible:outline-none focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:-2px]`}
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
                  {/* When status is editable the interactive control is rendered in
                      the aligned overlay below; keep this cell a placeholder so the
                      two status icons don't overlap. */}
                  {canMoveStatus ? (
                    <span aria-hidden="true" />
                  ) : (
                    <TicketStatusIcon state={ticket.state} />
                  )}
                  <span className="flex min-w-0 items-center gap-2">
                    {isUnread?.(ticket) && <UnreadCommentIndicator />}
                    <span className="truncate font-medium">{ticket.title}</span>
                    <TicketLabels labels={ticket.labels} max={2} className="shrink-0 text-[var(--text-secondary)]" />
                  </span>
                  <span className="min-w-0">
                    <TicketAssigneeChip person={ticket.assignee} unassignedTone="secondary" />
                  </span>
                  {/* RX (suitcase) column, then the PR column placeholder; the
                      interactive PR control is rendered in the aligned overlay below. */}
                  <TicketAssociationBadge count={ticket.associationCount} />
                  <span aria-hidden="true" />
                  {/* Project (category) column, just left of the timestamp. */}
                  <span
                    className="truncate text-[11px] text-[var(--text-secondary)]"
                    title={ticket.project ?? undefined}
                  >
                    {ticket.project ?? ""}
                  </span>
                  <span className="text-xs text-[var(--text-secondary)]">{formatTicketDate(ticket.updatedAt)}</span>
                </button>
                {ticket.openPrNumber != null && ticket.openPrUrl != null && (
                  <div className={`${TICKET_ROW_GRID} pointer-events-none absolute inset-0 py-1.5`}>
                    {/* Key | Status | Title | Assignee | RX | PR | Project | Updated —
                        the PR control sits in the 6th cell, after the RX column. */}
                    <span aria-hidden="true" />
                    <span aria-hidden="true" />
                    <span aria-hidden="true" />
                    <span aria-hidden="true" />
                    <span aria-hidden="true" />
                    <span className="flex min-w-0 items-center">
                      <TicketPrOpenControl
                        prNumber={ticket.openPrNumber}
                        prUrl={ticket.openPrUrl}
                        prStatus={ticket.openPrStatus}
                      />
                    </span>
                    <span aria-hidden="true" />
                    <span aria-hidden="true" />
                  </div>
                )}
                {canMoveStatus && onMoveTicket && (
                  <div className={`${TICKET_ROW_GRID} pointer-events-none absolute inset-0 py-1.5`}>
                    {/* Interactive status control aligned to the Status (2nd) cell. */}
                    <span aria-hidden="true" />
                    <span className="flex items-center">
                      <TicketStatusControl
                        ticket={ticket}
                        columns={columns}
                        onMoveTicket={onMoveTicket}
                      />
                    </span>
                    <span aria-hidden="true" />
                    <span aria-hidden="true" />
                    <span aria-hidden="true" />
                    <span aria-hidden="true" />
                    <span aria-hidden="true" />
                    <span aria-hidden="true" />
                  </div>
                )}
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
      className="flex h-full min-h-0 w-[280px] shrink-0 flex-col overflow-hidden rounded-lg"
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
        <span className="flex items-center gap-2">
          {ticket.openPrNumber != null && (
            <span
              className="inline-flex items-center gap-1 text-xs font-medium"
              style={{
                color: isLivePrStatus(ticket.openPrStatus)
                  ? "var(--status-success)"
                  : "var(--text-muted)",
              }}
            >
              <GitPullRequestArrow className="h-3.5 w-3.5" aria-hidden="true" />
              #{ticket.openPrNumber}
            </span>
          )}
          <TicketAssociationBadge count={ticket.associationCount} />
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
      <div className="flex h-full min-h-0 w-full flex-1 gap-3 overflow-x-auto overflow-y-hidden p-4">
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
              <div className="min-h-0 flex-1 space-y-2 overflow-y-auto overscroll-contain p-2">
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
    <div className="flex h-full min-h-0 w-full flex-1 gap-3 overflow-hidden p-4" aria-label="Loading kanban view">
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
