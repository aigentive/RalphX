export type PullRequestDisplayStatus = "Draft" | "Open" | "Merged" | "Closed";

export function normalizePrStatus(
  status: string | null | undefined,
  isDraft?: boolean,
): PullRequestDisplayStatus {
  if (isDraft) {
    return "Draft";
  }
  const normalized = status?.trim().toLowerCase();
  if (normalized === "merged") {
    return "Merged";
  }
  if (normalized === "closed") {
    return "Closed";
  }
  if (normalized === "draft") {
    return "Draft";
  }
  return "Open";
}

export function formatPrDate(value: string | null | undefined): string | null {
  if (!value) {
    return null;
  }
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) {
    return value;
  }
  return new Intl.DateTimeFormat(undefined, {
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
  }).format(date);
}
