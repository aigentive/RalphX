import type { LucideIcon } from "lucide-react";
import { ChevronRight } from "lucide-react";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";

export interface SessionGroupHeaderProps {
  icon: LucideIcon;
  label: string;
  count: number;
  isOpen: boolean;
  onToggle: (open: boolean) => void;
  /** True when one of the sessions inside this group is the currently selected plan. */
  isActive?: boolean;
  /** Reserved for future use; ignored to keep parity with AgentsSidebar styling. */
  accentColor?: string;
  children: React.ReactNode;
}

export function SessionGroupHeader({
  icon: Icon,
  label,
  count,
  isOpen,
  onToggle,
  isActive = false,
  children,
}: SessionGroupHeaderProps) {
  return (
    <Collapsible
      open={isOpen}
      onOpenChange={onToggle}
      className="my-1"
      data-testid={`session-group-${label.toLowerCase()}`}
    >
      <div className="px-3">
        <CollapsibleTrigger asChild>
          <button
            data-testid="session-group-trigger"
            aria-current={isActive ? "true" : undefined}
            className="agents-project-row relative grid w-full grid-cols-[12px_14px_minmax(0,1fr)_auto] items-center gap-[7px] rounded-[6px] px-2 py-1.5 text-left text-[0.8438rem] transition-colors duration-[120ms] ease-[cubic-bezier(.2,.8,.2,1)] hover:bg-[var(--bg-elevated)] outline-none focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:2px]"
          >
            <ChevronRight
              className={`agents-project-chevron h-2.5 w-2.5 transition-transform duration-[120ms] ${isOpen ? "rotate-90" : ""}`}
              strokeWidth={2}
            />
            <Icon
              className="agents-project-icon h-3.5 w-3.5 shrink-0"
              strokeWidth={1.8}
            />
            <span className="min-w-0 truncate">
              {label}
            </span>
            {count > 0 && (
              <span
                className="agents-project-count grid min-w-[18px] place-items-center rounded-full border px-1.5 text-[0.6562rem] leading-[1.6]"
                style={{ borderStyle: "solid", borderWidth: "1px" }}
              >
                {count}
              </span>
            )}
          </button>
        </CollapsibleTrigger>
      </div>
      <CollapsibleContent className="mt-1 space-y-1">
        {children}
      </CollapsibleContent>
    </Collapsible>
  );
}
