import type {
  TicketComment,
  TicketingColumn,
  TicketRef,
  TicketStateCategory,
  TicketSummary,
} from "@/api/ticketing";
import type { TicketingFilterState } from "@/stores/ticketingStore";

/** Stable per-ticket key used for read-state bookkeeping. */
export function ticketRefKey(ref: TicketRef): string {
  return `${ref.provider}:${ref.id}`;
}

/** Whether any ticket filter (text, status, labels, assignee, sprint) is currently active. */
export function hasActiveTicketFilters(filters: TicketingFilterState): boolean {
  return (
    filters.text.trim() !== ""
    || filters.stateIds.length > 0
    || filters.labels.length > 0
    || filters.assignee !== null
    || filters.sprint !== null
  );
}

/** Sentinel assignee-filter value selecting tickets with no assignee. */
export const UNASSIGNED_ASSIGNEE = "__unassigned__";

/** Distinct, sorted assignee display names present in the given tickets. */
export function distinctAssigneeNames(tickets: TicketSummary[]): string[] {
  const names = new Set<string>();
  for (const ticket of tickets) {
    const name = ticket.assignee?.name?.trim();
    if (name) {
      names.add(name);
    }
  }
  return Array.from(names).sort((left, right) => left.localeCompare(right));
}

/** Distinct, sorted ClickUp list/sprint names present on current-user tickets. */
export function distinctCurrentUserSprintNames(tickets: TicketSummary[]): string[] {
  const names = new Set<string>();
  for (const ticket of tickets) {
    if (!ticket.currentUserAssigned) {
      continue;
    }
    const name = ticket.project?.trim();
    if (name) {
      names.add(name);
    }
  }
  return Array.from(names).sort((left, right) => left.localeCompare(right));
}

/**
 * Client-side assignee filter. null/empty = everyone, the UNASSIGNED_ASSIGNEE
 * sentinel = tickets with no assignee, any other value = exact name match.
 */
export function filterTicketsByAssignee(
  tickets: TicketSummary[],
  assignee: string | null,
): TicketSummary[] {
  if (!assignee) {
    return tickets;
  }
  if (assignee === UNASSIGNED_ASSIGNEE) {
    return tickets.filter((ticket) => !ticket.assignee);
  }
  return tickets.filter((ticket) => ticket.assignee?.name === assignee);
}

/**
 * Client-side container/project filter by display name. null = all containers.
 * Used until the provider search supports server-side container scoping.
 */
export function filterTicketsByProject(
  tickets: TicketSummary[],
  project: string | null,
): TicketSummary[] {
  if (!project) {
    return tickets;
  }
  return tickets.filter((ticket) => ticket.project === project);
}

export interface TicketStatusGroup {
  id: string;
  name: string;
  category: TicketStateCategory;
  tickets: TicketSummary[];
}

/**
 * Groups tickets by their workflow state for a Linear-style sectioned list.
 * Group order follows the provided columns; states absent from columns sort
 * last (alphabetically). Ticket order within a group is preserved.
 */
export function groupTicketsByStatus(
  tickets: TicketSummary[],
  columns: TicketingColumn[],
): TicketStatusGroup[] {
  const order = new Map(columns.map((column, index) => [column.id, index]));
  const groups = new Map<string, TicketStatusGroup>();
  for (const ticket of tickets) {
    const existing = groups.get(ticket.state.id);
    if (existing) {
      existing.tickets.push(ticket);
    } else {
      groups.set(ticket.state.id, {
        id: ticket.state.id,
        name: ticket.state.name,
        category: ticket.state.category,
        tickets: [ticket],
      });
    }
  }
  const fallback = Number.MAX_SAFE_INTEGER;
  return Array.from(groups.values()).sort((left, right) => {
    const leftOrder = order.get(left.id) ?? fallback;
    const rightOrder = order.get(right.id) ?? fallback;
    if (leftOrder !== rightOrder) {
      return leftOrder - rightOrder;
    }
    return left.name.localeCompare(right.name);
  });
}

function toTime(value: string | null | undefined): number | null {
  if (!value) {
    return null;
  }
  const trimmed = value.trim();
  const time = /^\d+$/.test(trimmed) ? Number(trimmed) : new Date(trimmed).getTime();
  return Number.isNaN(time) ? null : time;
}

/** Optimistic comments inserted locally before the provider round-trip completes. */
export function isOptimisticCommentId(id: string | null | undefined): boolean {
  return typeof id === "string" && (id.startsWith("local:") || id.startsWith("optimistic:"));
}

/** Oldest-first by createdAt; comments without a timestamp sort to the end. */
export function sortCommentsByCreatedAt(comments: TicketComment[]): TicketComment[] {
  return [...comments].map((comment) => ({
    ...comment,
    replies: sortCommentsByCreatedAt(comment.replies ?? []),
  })).sort((left, right) => {
    const leftTime = toTime(left.createdAt);
    const rightTime = toTime(right.createdAt);
    if (leftTime === null && rightTime === null) {
      return 0;
    }
    if (leftTime === null) {
      return 1;
    }
    if (rightTime === null) {
      return -1;
    }
    return leftTime - rightTime;
  });
}

/**
 * A comment is "new" when it was created after the moment the viewer last opened
 * this ticket. Never-opened tickets (no baseline) and the viewer's own optimistic
 * comments are never flagged.
 */
export function isCommentNewSince(
  comment: TicketComment,
  seenAt: string | null | undefined,
): boolean {
  if (isOptimisticCommentId(comment.id)) {
    return false;
  }
  const seen = toTime(seenAt);
  if (seen === null) {
    return false;
  }
  const created = toTime(comment.createdAt);
  if (created === null) {
    return false;
  }
  return created > seen;
}

export function countNewComments(
  comments: TicketComment[],
  seenAt: string | null | undefined,
): number {
  return comments.reduce(
    (count, comment) => (isCommentNewSince(comment, seenAt) ? count + 1 : count),
    0,
  );
}

/**
 * Row-level "updated since you last opened" signal. Requires a prior open
 * (a baseline) so freshly-discovered tickets are not all flagged as unread.
 */
export function isTicketUpdatedSince(
  updatedAt: string | null | undefined,
  seenAt: string | null | undefined,
): boolean {
  const seen = toTime(seenAt);
  if (seen === null) {
    return false;
  }
  const updated = toTime(updatedAt);
  if (updated === null) {
    return false;
  }
  return updated > seen;
}
