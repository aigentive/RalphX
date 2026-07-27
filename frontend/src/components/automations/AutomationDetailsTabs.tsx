import type { ReactNode } from "react";

import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";

interface AutomationDetailsTabsProps {
  config: ReactNode;
  spec: ReactNode;
  inputs: ReactNode;
  hasSpec: boolean;
  inputCount: number;
}

export function AutomationDetailsTabs({
  config,
  spec,
  inputs,
  hasSpec,
  inputCount,
}: AutomationDetailsTabsProps) {
  return (
    <section
      className="rounded-lg p-5 shadow-[var(--shadow-xs)]"
      style={{
        backgroundColor: "var(--bg-elevated, #232329)",
        borderColor: "var(--border-subtle, #2e2e36)",
        borderStyle: "solid",
        borderWidth: "1px",
      }}
      data-testid="automation-details-card"
    >
      <Tabs defaultValue="config">
        <TabsList
          className="h-9 justify-start rounded-md p-1"
          style={{
            backgroundColor: "var(--bg-surface, #1e1e23)",
            borderColor: "var(--border-subtle, #2e2e36)",
            borderStyle: "solid",
            borderWidth: "1px",
          }}
          aria-label="Automation details"
        >
          <TabsTrigger className="rounded-sm px-3 py-1 text-xs text-[var(--text-secondary)] data-[state=active]:bg-[var(--bg-elevated)] data-[state=active]:text-[var(--text-primary)] data-[state=active]:shadow-sm" value="config">
            Config
          </TabsTrigger>
          <TabsTrigger className="rounded-sm px-3 py-1 text-xs text-[var(--text-secondary)] data-[state=active]:bg-[var(--bg-elevated)] data-[state=active]:text-[var(--text-primary)] data-[state=active]:shadow-sm" value="spec">
            Spec
            {hasSpec ? <span className="ml-1" aria-hidden="true">•</span> : null}
          </TabsTrigger>
          <TabsTrigger className="rounded-sm px-3 py-1 text-xs text-[var(--text-secondary)] data-[state=active]:bg-[var(--bg-elevated)] data-[state=active]:text-[var(--text-primary)] data-[state=active]:shadow-sm" value="inputs">
            Inputs{inputCount > 0 ? ` (${inputCount})` : ""}
          </TabsTrigger>
        </TabsList>
        <TabsContent value="config" className="mt-3" data-testid="automation-config-panel">
          {config}
        </TabsContent>
        <TabsContent value="spec" className="mt-3" data-testid="automation-spec-card">
          {spec}
        </TabsContent>
        <TabsContent value="inputs" className="mt-3" data-testid="automation-inputs-panel">
          {inputs}
        </TabsContent>
      </Tabs>
    </section>
  );
}
