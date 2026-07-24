import { FileText, GitPullRequestArrow, ListTree } from "lucide-react";

import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { planBundlePanelId, planBundleTabId } from "./planBundleTabIds";

export type PlanBundleBodyMode = "overview" | "blueprint" | "proposals";

interface PlanBundleTabsProps {
  idPrefix: string;
  value: PlanBundleBodyMode;
  onValueChange: (value: PlanBundleBodyMode) => void;
  linkedProposalsCount: number;
}

export function PlanBundleTabs({
  idPrefix,
  value,
  onValueChange,
  linkedProposalsCount,
}: PlanBundleTabsProps) {
  return (
    <Tabs
      value={value}
      onValueChange={(next) => onValueChange(next as PlanBundleBodyMode)}
    >
      <TabsList
        aria-label="Plan documents"
        className="h-7 gap-1 bg-transparent p-0"
      >
        <TabsTrigger
          value="overview"
          id={planBundleTabId(idPrefix, "overview")}
          aria-controls={planBundlePanelId(idPrefix, "overview")}
          data-testid="plan-overview-tab"
          className="h-7 gap-1.5 px-2.5 text-[0.6875rem]"
        >
          <FileText className="h-3 w-3" />
          Overview
        </TabsTrigger>
        <TabsTrigger
          value="blueprint"
          id={planBundleTabId(idPrefix, "blueprint")}
          aria-controls={planBundlePanelId(idPrefix, "blueprint")}
          data-testid="plan-blueprint-tab"
          className="h-7 gap-1.5 px-2.5 text-[0.6875rem]"
        >
          <ListTree className="h-3 w-3" />
          Blueprint
        </TabsTrigger>
        {linkedProposalsCount > 0 ? (
          <TabsTrigger
            value="proposals"
            id={planBundleTabId(idPrefix, "proposals")}
            aria-controls={planBundlePanelId(idPrefix, "proposals")}
            data-testid="plan-proposals-tab"
            className="h-7 gap-1.5 px-2.5 text-[0.6875rem]"
          >
            <GitPullRequestArrow className="h-3 w-3" />
            Proposals ({linkedProposalsCount})
          </TabsTrigger>
        ) : null}
      </TabsList>
    </Tabs>
  );
}
