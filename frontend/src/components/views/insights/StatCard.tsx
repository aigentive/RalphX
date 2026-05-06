import { HelpCircle } from "lucide-react";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";

interface StatCardProps {
  label: string;
  value: string;
  sub?: string;
  tooltip?: string;
}

export function StatCard({ label, value, sub, tooltip }: StatCardProps) {
  return (
    <div
      data-testid="insights-card"
      className="flex flex-col gap-1 rounded-xl"
      style={{
        backgroundColor: "var(--bg-surface)",
        border: "1px solid var(--overlay-faint)",
        padding: "14px 16px",
      }}
    >
      <span
        className="flex items-center gap-1 text-[0.6875rem] font-semibold uppercase tracking-wider"
        style={{ letterSpacing: "0.08em", color: "var(--text-secondary)" }}
      >
        {label}
        {tooltip !== undefined && (
          <TooltipProvider delayDuration={300}>
            <Tooltip>
              <TooltipTrigger asChild>
                <HelpCircle
                  className="inline w-3.5 h-3.5 cursor-help"
                  style={{ color: "var(--text-muted)" }}
                />
              </TooltipTrigger>
              <TooltipContent side="top" className="max-w-[240px] text-xs normal-case tracking-normal font-normal">
                {tooltip}
              </TooltipContent>
            </Tooltip>
          </TooltipProvider>
        )}
      </span>
      <span
        className="text-[1.375rem] font-semibold"
        style={{ fontFamily: "system-ui", color: "var(--text-primary)", letterSpacing: "-0.02em" }}
      >
        {value}
      </span>
      {sub !== undefined && (
        <span className="text-[0.75rem]" style={{ color: "var(--text-muted)" }}>
          {sub}
        </span>
      )}
    </div>
  );
}
