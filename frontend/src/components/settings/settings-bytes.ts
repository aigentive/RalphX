/** Byte/date formatting shared by the Database settings sections. */

export function formatBytes(value: number): string {
  if (value < 1024) return `${value} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let size = value / 1024;
  let unit = 0;
  while (size >= 1024 && unit < units.length - 1) { size /= 1024; unit += 1; }
  return `${size.toFixed(size >= 10 ? 0 : 1)} ${units[unit]}`;
}

const GIB = 1024 * 1024 * 1024;

export function gibToBytes(gib: number): number {
  return Math.round(gib * GIB);
}

export function bytesToGib(bytes: number): number {
  return Math.round((bytes / GIB) * 10) / 10;
}

/** Absolute local timestamp — retention deletes data, so "2 days ago" is not enough. */
export function formatTimestamp(value: string | null): string {
  if (!value) return "Never";
  const parsed = new Date(value);
  return Number.isNaN(parsed.getTime()) ? "Unknown" : parsed.toLocaleString();
}
