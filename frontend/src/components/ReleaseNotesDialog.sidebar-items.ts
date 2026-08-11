import type { ReleaseMetadata } from "@/api/release-notes";

function formatMonthYear(dateStr: string): string {
  const d = new Date(dateStr);
  return d.toLocaleDateString("en-US", { month: "long", year: "numeric" });
}

export function formatDay(dateStr: string): string {
  const d = new Date(dateStr);
  return d.toLocaleDateString("en-US", { month: "short", day: "numeric" });
}

export type SidebarItem =
  | { kind: "header"; label: string }
  | { kind: "version"; version: string; date: string | null; isCurrent: boolean };

export function buildSidebarItems(
  versions: string[],
  metadata: Map<string, ReleaseMetadata>,
  currentAppVersion: string | null,
): SidebarItem[] {
  const items: SidebarItem[] = [];
  let currentMonth = "";

  for (const version of versions) {
    const meta = metadata.get(version);
    const monthYear = meta ? formatMonthYear(meta.publishedAt) : null;

    if (monthYear && monthYear !== currentMonth) {
      currentMonth = monthYear;
      items.push({ kind: "header", label: monthYear });
    }

    items.push({
      kind: "version",
      version,
      date: meta ? formatDay(meta.publishedAt) : null,
      isCurrent: version === currentAppVersion,
    });
  }

  return items;
}
