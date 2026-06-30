import type { TicketingColumn, TicketSummary } from "@/api/ticketing";
import type { TicketStatusGroup } from "./ticketing-read-state";

import { categoryToken } from "./ticketing-utils";

export function statusColor(
  status:
    | Pick<TicketingColumn, "category" | "color">
    | Pick<TicketSummary["state"], "category" | "color">
    | Pick<TicketStatusGroup, "category" | "color">,
): string {
  return status.color?.trim() || categoryToken(status.category);
}

export function normalizeStatusKey(value: string | null | undefined): string | null {
  const normalized = value
    ?.trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "_")
    .replace(/^_+|_+$/g, "");
  return normalized ? normalized : null;
}

function statusColumnKeys(column: TicketingColumn): Set<string> {
  return new Set(
    [normalizeStatusKey(column.id), normalizeStatusKey(column.name)]
      .filter((value): value is string => Boolean(value)),
  );
}

function editDistanceAtMostOne(left: string, right: string): boolean {
  if (left === right) {
    return true;
  }
  if (Math.abs(left.length - right.length) > 1) {
    return false;
  }
  let leftIndex = 0;
  let rightIndex = 0;
  let edits = 0;
  while (leftIndex < left.length && rightIndex < right.length) {
    if (left[leftIndex] === right[rightIndex]) {
      leftIndex += 1;
      rightIndex += 1;
      continue;
    }
    edits += 1;
    if (edits > 1) {
      return false;
    }
    if (left.length > right.length) {
      leftIndex += 1;
    } else if (right.length > left.length) {
      rightIndex += 1;
    } else {
      leftIndex += 1;
      rightIndex += 1;
    }
  }
  return edits + (left.length - leftIndex) + (right.length - rightIndex) <= 1;
}

function likelySameStatusColumn(left: TicketingColumn, right: TicketingColumn): boolean {
  const leftKeys = statusColumnKeys(left);
  const rightKeys = statusColumnKeys(right);
  for (const key of rightKeys) {
    if (leftKeys.has(key)) {
      return true;
    }
  }
  if (left.category !== right.category) {
    return false;
  }
  const leftName = normalizeStatusKey(left.name);
  const rightName = normalizeStatusKey(right.name);
  return Boolean(leftName && rightName && editDistanceAtMostOne(leftName, rightName));
}

function dedupeColumns(columns: TicketingColumn[]): TicketingColumn[] {
  const deduped: TicketingColumn[] = [];
  for (const column of columns) {
    if (deduped.some((existing) => likelySameStatusColumn(existing, column))) {
      continue;
    }
    deduped.push({ ...column, order: deduped.length });
  }
  return deduped;
}

export function mergeProviderAndTicketColumns(
  providerColumns: TicketingColumn[],
  ticketColumns: TicketingColumn[],
): TicketingColumn[] {
  const providerOrdered = [...providerColumns].sort((left, right) => left.order - right.order);
  if (providerOrdered.length === 0) {
    return dedupeColumns(ticketColumns);
  }
  const providerKnownColumns = dedupeColumns(providerOrdered);
  const merged = dedupeColumns(
    providerOrdered.filter((column) => column.isVisible !== false),
  );
  for (const column of ticketColumns) {
    if (
      providerKnownColumns.some((existing) => likelySameStatusColumn(existing, column))
      || merged.some((existing) => likelySameStatusColumn(existing, column))
    ) {
      continue;
    }
    merged.push({ ...column, order: merged.length });
  }
  return merged;
}
