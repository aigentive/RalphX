import type { ReactNode } from "react";
import { FileDown } from "lucide-react";
import { withAlpha } from "@/lib/theme-colors";

interface TabEmptyStateProps {
  icon: ReactNode;
  heading: string;
  description: string;
  onBrowse?: () => void;
}

export function TabEmptyState({ icon, heading, description, onBrowse }: TabEmptyStateProps) {
  return (
    <div
      data-testid="tab-empty-state"
      className="flex flex-col items-center justify-center h-full p-6"
    >
      <div
        className="w-full max-w-[340px] rounded-2xl p-6"
        style={{
          background: "var(--overlay-faint)",
          border: "1px solid var(--border-subtle)",
        }}
      >
        {/* Icon */}
        <div className="flex justify-center mb-4">
          {icon}
        </div>

        {/* Text */}
        <div className="text-center">
          <h3
            className="text-sm font-semibold mb-1.5 tracking-tight"
            style={{ color: "var(--text-primary)" }}
          >
            {heading}
          </h3>
          <p className="text-xs leading-relaxed" style={{ color: "var(--text-secondary)" }}>
            {description}
          </p>
        </div>

        {/* Arrow hint — points RIGHT toward chat panel */}
        <div className="flex justify-center mt-5">
          <div
            className="flex items-center gap-2 px-3 py-1.5 rounded-full"
            style={{
              background: "var(--bg-elevated)",
              border: "1px solid var(--border-subtle)",
            }}
          >
            <svg
              width="14"
              height="14"
              viewBox="0 0 14 14"
              fill="none"
              style={{ color: "var(--text-muted)" }}
            >
              <path
                d="M2 7h10m0 0l-3-3m3 3l-3 3"
                stroke="currentColor"
                strokeWidth="1.5"
                strokeLinecap="round"
                strokeLinejoin="round"
              />
            </svg>
            <span
              className="text-[0.625rem] uppercase tracking-wider"
              style={{ color: "var(--text-muted)" }}
            >
              From chat
            </span>
          </div>
        </div>

        {onBrowse !== undefined && (
          <>
            {/* Divider with "or" */}
            <div className="flex items-center gap-3 mt-6">
              <div
                className="flex-1 h-px"
                style={{ background: "var(--overlay-moderate)" }}
              />
              <span
                className="text-[0.625rem] uppercase tracking-wider"
                style={{ color: "var(--text-muted)" }}
              >
                or
              </span>
              <div
                className="flex-1 h-px"
                style={{ background: "var(--overlay-moderate)" }}
              />
            </div>

            {/* Browse button — filled with a soft gray tint (overlay-weak)
                so the drop zone reads as a nested frame inside the card,
                slightly lighter than the card's overlay-faint fill. Hover
                bumps to overlay-moderate for affordance. */}
            <button
              data-testid="drop-hint"
              type="button"
              onClick={onBrowse}
              className="w-full flex flex-col items-center gap-2 mt-4 p-3 rounded-lg transition-colors cursor-pointer focus:outline-none"
              style={{ color: "var(--text-muted)", background: "var(--overlay-weak)" }}
              onMouseEnter={(e) => {
                e.currentTarget.style.background = "var(--overlay-moderate)";
              }}
              onMouseLeave={(e) => {
                e.currentTarget.style.background = "var(--overlay-weak)";
              }}
            >
              <FileDown className="w-4 h-4" />
              <p className="text-xs text-center leading-relaxed">
                Drag a markdown file here
                <br />
                or{" "}
                <span
                  style={{ color: withAlpha("var(--accent-primary)", 80) }}
                  className="underline underline-offset-2"
                >
                  click to browse
                </span>
              </p>
            </button>
          </>
        )}
      </div>
    </div>
  );
}
