import { useEffect, useState } from "react";

import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import type { ProjectSettings } from "@/types/settings";

import { SettingsSection } from "../SettingsView.shared";
import ExecutionSection from "./ExecutionSection";
import WorkspaceReviewSection from "./WorkspaceReviewSection";

export type WorkspaceSettingsTab = "general" | "review";

export default function WorkspaceSettingsSection({
  settings,
  disabled,
  onSettingsChange,
  initialTab = "general",
}: {
  settings: ProjectSettings;
  disabled: boolean;
  onSettingsChange: (settings: ProjectSettings) => void;
  initialTab?: WorkspaceSettingsTab;
}) {
  const [tab, setTab] = useState<WorkspaceSettingsTab>(initialTab);
  useEffect(() => setTab(initialTab), [initialTab]);
  return (
    <SettingsSection>
      <Tabs value={tab} onValueChange={(value) => setTab(value as WorkspaceSettingsTab)}>
        <TabsList aria-label="Workspace settings">
          <TabsTrigger value="general">General</TabsTrigger>
          <TabsTrigger value="review">Review</TabsTrigger>
        </TabsList>
        <TabsContent value="general" className="mt-4">
          <ExecutionSection
            settings={settings.execution}
            onChange={(changes) =>
              onSettingsChange({
                ...settings,
                execution: { ...settings.execution, ...changes },
              })
            }
            disabled={disabled}
            content="workspace"
            embedded
          />
        </TabsContent>
        <TabsContent value="review" className="mt-4">
          <WorkspaceReviewSection embedded />
        </TabsContent>
      </Tabs>
    </SettingsSection>
  );
}
