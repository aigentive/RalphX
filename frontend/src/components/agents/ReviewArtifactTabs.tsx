import { FileText, ListChecks } from "lucide-react";

import {
  Tabs,
  TabsContent,
  TabsList,
  TabsTrigger,
} from "@/components/ui/tabs";

export type ReviewArtifactBodyMode = "overview" | "requested_changes";

interface ReviewArtifactTabsProps {
  value: ReviewArtifactBodyMode;
  onValueChange: (value: ReviewArtifactBodyMode) => void;
}

export function ReviewArtifactTabs({
  value,
  onValueChange,
}: ReviewArtifactTabsProps) {
  return (
    <Tabs
      value={value}
      onValueChange={(next) =>
        onValueChange(next as ReviewArtifactBodyMode)
      }
    >
      <TabsList
        aria-label="Workspace Review documents"
        className="h-7 gap-1 bg-transparent p-0"
      >
        <TabsTrigger
          value="overview"
          className="h-7 gap-1.5 px-2.5 text-[0.6875rem]"
        >
          <FileText className="h-3 w-3" />
          Overview
        </TabsTrigger>
        <TabsTrigger
          value="requested_changes"
          className="h-7 gap-1.5 px-2.5 text-[0.6875rem]"
        >
          <ListChecks className="h-3 w-3" />
          Requested Changes
        </TabsTrigger>
      </TabsList>
      <TabsContent value="overview" className="hidden" />
      <TabsContent value="requested_changes" className="hidden" />
    </Tabs>
  );
}
