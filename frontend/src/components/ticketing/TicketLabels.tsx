import { splitLabels } from "./ticketing-presentation";

interface TicketLabelsProps {
  labels: string[];
  /** Maximum chips to show before collapsing the rest into a "+N" chip. */
  max?: number;
  size?: "sm" | "md";
  className?: string;
}

/**
 * Shared label-chip row for ticket list rows, kanban cards, and the detail
 * overlay. Collapses overflow into a "+N" chip whose title lists the hidden
 * labels. Renders nothing when there are no labels.
 */
export function TicketLabels({ labels, max = 3, size = "sm", className }: TicketLabelsProps) {
  if (labels.length === 0) {
    return null;
  }

  const { visible, overflow } = splitLabels(labels, max);
  const padding = size === "md" ? "px-2 py-1" : "px-1 py-0.5";
  const chipStyle = {
    borderColor: "var(--border-subtle)",
    borderStyle: "solid" as const,
    borderWidth: "1px",
  };

  return (
    <span className={`inline-flex min-w-0 flex-wrap items-center gap-1 text-[11px] ${className ?? ""}`.trim()}>
      {visible.map((label) => (
        <span key={label} className={`max-w-[140px] truncate rounded ${padding}`} style={chipStyle}>
          {label}
        </span>
      ))}
      {overflow > 0 && (
        <span
          className={`shrink-0 rounded ${padding} text-[var(--text-muted)]`}
          style={chipStyle}
          title={labels.slice(visible.length).join(", ")}
        >
          +{overflow}
        </span>
      )}
    </span>
  );
}
