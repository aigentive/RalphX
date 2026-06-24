/**
 * Pure presentational helpers for ticketing components. Kept separate from the
 * components themselves so React Fast Refresh keeps working and the helpers stay
 * unit-testable.
 */

/** First+last initial for multi-word names, first two letters otherwise. */
export function assigneeInitials(name: string): string {
  const parts = name.trim().split(/\s+/).filter(Boolean);
  const first = parts[0];
  if (!first) {
    return "?";
  }
  if (parts.length === 1) {
    return first.slice(0, 2).toUpperCase();
  }
  const last = parts[parts.length - 1] ?? first;
  return (first.charAt(0) + last.charAt(0)).toUpperCase();
}

export interface LabelOverflow {
  visible: string[];
  overflow: number;
}

/** Splits labels into a visible slice plus an overflow count for a "+N" chip. */
export function splitLabels(labels: string[], max: number): LabelOverflow {
  if (max <= 0 || labels.length <= max) {
    return { visible: max <= 0 ? [] : labels, overflow: max <= 0 ? labels.length : 0 };
  }
  return { visible: labels.slice(0, max), overflow: labels.length - max };
}
