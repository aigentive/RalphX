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
      className="rounded-md p-4"
      style={{
        backgroundColor: "var(--bg-surface)",
        borderColor: "var(--border-default)",
        borderStyle: "solid",
        borderWidth: "1px",
      }}
      data-testid="automation-details-card"
    >
      <Tabs defaultValue="config">
        <TabsList
          className="h-9 justify-start rounded-md bg-transparent p-0"
          aria-label="Automation details"
        >
          <TabsTrigger className="rounded-sm px-3 py-1 text-xs" value="config">
            Config
          </TabsTrigger>
          <TabsTrigger className="rounded-sm px-3 py-1 text-xs" value="spec">
            Spec
            {hasSpec ? <span className="ml-1" aria-hidden="true">•</span> : null}
          </TabsTrigger>
          <TabsTrigger className="rounded-sm px-3 py-1 text-xs" value="inputs">
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
