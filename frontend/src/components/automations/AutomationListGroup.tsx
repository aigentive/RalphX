import type { ReactNode } from "react";

interface AutomationListGroupProps {
  title: string;
  hint: string;
  count: number;
  children: ReactNode;
  testId: string;
}

export function AutomationListGroup({
  title,
  hint,
  count,
  children,
  testId,
}: AutomationListGroupProps) {
  return (
    <section data-testid={testId}>
      <div className="mb-2 flex flex-wrap items-baseline gap-x-2 gap-y-1 px-1">
        <h2
          className="text-[0.6875rem] font-semibold uppercase tracking-[0.1em]"
          style={{ color: "var(--text-muted, #8e8e96)" }}
        >
          {title} <span className="tabular-nums">{count}</span>
        </h2>
        <span className="text-xs" style={{ color: "var(--text-subtle, #6a6a72)" }}>
          {hint}
        </span>
      </div>
      <div
        className="overflow-hidden rounded-lg"
        style={{
          backgroundColor: "var(--bg-surface, #1e1e23)",
          borderColor: "var(--border-subtle, #2e2e36)",
          borderStyle: "solid",
          borderWidth: "1px",
        }}
      >
        {children}
      </div>
    </section>
  );
}
