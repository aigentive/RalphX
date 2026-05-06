/**
 * ColumnGroup - Collapsible group within a kanban column
 *
 * Design: v29a Kanban
 * - Divider-separated groups with compact section headers
 * - Simple chevron rotation
 */

import { type ReactNode } from "react";
import { ChevronDown } from "lucide-react";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import { cn } from "@/lib/utils";

interface ColumnGroupProps {
  /** Group label (e.g., 'Fresh Tasks', 'Needs Revision') */
  label: string;
  /** Number of tasks in this group */
  count: number;
  /** Optional icon to display next to the label */
  icon?: ReactNode;
  /** Accent color for the left border when expanded (CSS color value) */
  accentColor?: string;
  /** Whether the group is collapsed */
  collapsed?: boolean;
  /** Callback when collapse state changes */
  onToggle?: () => void;
  /** Group contents (task cards) */
  children: ReactNode;
}

export function ColumnGroup({
  label,
  count,
  icon,
  accentColor,
  collapsed = false,
  onToggle,
  children,
}: ColumnGroupProps) {
  const isExpanded = !collapsed;

  // Build props conditionally to avoid exactOptionalPropertyTypes issues
  const handleOpenChange = onToggle ? () => onToggle() : undefined;

  return (
    <Collapsible
      open={isExpanded}
      {...(handleOpenChange && { onOpenChange: handleOpenChange })}
      className="mt-1 border-t pt-2 first:mt-0 first:border-t-0 first:pt-0"
      style={{
        borderTopColor: "var(--kanban-group-divider)",
        borderTopStyle: "solid",
      }}
    >
      <div className="overflow-hidden">
        {/* Group header - simple like Finder section headers */}
        <CollapsibleTrigger asChild>
          <button
            type="button"
            className={cn(
              "w-full flex items-center gap-1.5 rounded-md text-left",
              "transition-colors rounded-md",
              "focus:outline-none focus-visible:ring-1 focus-visible:ring-[var(--accent-primary)]/50"
            )}
            style={{
              background: "transparent",
              padding: "4px 4px 2px",
            }}
            onMouseEnter={(e) => {
              e.currentTarget.style.background = "var(--overlay-faint)";
            }}
            onMouseLeave={(e) => {
              e.currentTarget.style.background = "transparent";
            }}
          >
            {/* Chevron indicator */}
            <ChevronDown
              className={cn(
                "w-3 h-3 flex-shrink-0 transition-transform duration-200",
                collapsed && "-rotate-90"
              )}
              style={{ color: "var(--text-muted)" }}
            />

            {/* Optional icon */}
            {icon && (
              <span
                className="flex-shrink-0"
                style={{ color: accentColor || "var(--text-secondary)" }}
              >
                {icon}
              </span>
            )}

            {/* Group label - small, gray like Finder */}
            <span
              className="flex-1 truncate"
              style={{
                fontSize: "0.7188rem",
                fontWeight: 600,
                color: "var(--text-secondary)",
                letterSpacing: "0.02em",
              }}
            >
              {label}
            </span>

            {/* Count - simple */}
            <span
              style={{
                fontSize: "0.6875rem",
                fontWeight: 500,
                color: "var(--text-muted)",
                fontVariantNumeric: "tabular-nums",
                fontFamily: "var(--font-mono, ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace)",
              }}
            >
              ({count})
            </span>
          </button>
        </CollapsibleTrigger>

        {/* Group content */}
        <CollapsibleContent className="pt-1">
          <div className="flex flex-col gap-2">
            {children}
          </div>
        </CollapsibleContent>
      </div>
    </Collapsible>
  );
}
