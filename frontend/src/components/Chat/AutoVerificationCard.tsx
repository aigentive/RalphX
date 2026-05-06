/**
 * AutoVerificationCard — Collapsed system card for auto-verification messages.
 * Renders inline in the chat stream, visually distinct from user bubbles.
 * Expandable to show the full verification prompt.
 */

import { useState } from "react";
import { Settings, ChevronRight } from "lucide-react";

interface AutoVerificationCardProps {
  content: string;
  createdAt: string;
}

export function AutoVerificationCard({ content }: AutoVerificationCardProps) {
  const [expanded, setExpanded] = useState(false);

  // Strip <auto-verification> wrapper tags if present
  const displayContent = content
    .replace(/^<auto-verification>\s*/i, "")
    .replace(/\s*<\/auto-verification>$/i, "")
    .trim();

  return (
    <div className="flex flex-col items-center py-1">
      <button
        type="button"
        onClick={() => setExpanded(!expanded)}
        className="flex items-center gap-1.5 px-2.5 py-[3px] rounded-xl transition-colors"
        style={{ background: expanded ? "var(--bg-surface)" : "transparent" }}
        onMouseEnter={(e) => {
          if (!expanded) e.currentTarget.style.background = "var(--bg-surface)";
        }}
        onMouseLeave={(e) => {
          if (!expanded) e.currentTarget.style.background = "transparent";
        }}
      >
        <Settings
          className="w-[11px] h-[11px]"
          style={{ color: "var(--text-muted)" }}
        />
        <span
          className="text-[0.6875rem] leading-none"
          style={{ color: "var(--text-muted)", fontFamily: "var(--font-body)" }}
        >
          Auto-verification
        </span>
        <ChevronRight
          className="w-[11px] h-[11px] transition-transform"
          style={{
            color: "var(--text-muted)",
            transform: expanded ? "rotate(90deg)" : "rotate(0deg)",
          }}
        />
      </button>
      {expanded && (
        <div
          className="mt-1 mx-4 w-full max-w-[480px] rounded-lg px-3 py-2 text-[0.6875rem] leading-relaxed"
          style={{
            background: "var(--bg-surface)",
            color: "var(--text-secondary)",
            fontFamily: "var(--font-body)",
            whiteSpace: "pre-wrap",
            wordBreak: "break-word",
          }}
        >
          {displayContent}
        </div>
      )}
    </div>
  );
}
