import { useState } from "react";
import { ChevronRight, TriangleAlert } from "lucide-react";

import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { useAgentModels } from "@/hooks/useAgentModels";
import { useHarnessProviders } from "@/hooks/useHarnessProviders";
import {
  useReviewSettings,
  useUpdateReviewSettings,
} from "@/hooks/useReviewSettings";
import { useWorkspaceReviewRuntimeSettings } from "@/hooks/useWorkspaceReviewSettings";
import { selectActiveProject, useProjectStore } from "@/stores/projectStore";
import { useUiStore } from "@/stores/uiStore";
import {
  NumberSettingRow,
  SettingsSection,
  ToggleSettingRow,
} from "../SettingsView.shared";
import { WorkspaceReviewScopeRows } from "./WorkspaceReviewRuntimeRows";
import { isKnownHarness } from "./workspaceReviewHarness";

function InlineNotice({
  title,
  children,
}: {
  title: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex items-start gap-2 rounded-md border border-[var(--notice-warn-border)] bg-[var(--notice-warn-bg)] px-3 py-2 text-[0.6875rem] leading-relaxed text-[var(--notice-warn-text)]">
      <TriangleAlert className="mt-0.5 h-3.5 w-3.5 shrink-0 text-[var(--notice-warn-icon)]" />
      <div className="min-w-0 flex-1">
        <div className="font-medium text-[var(--text-primary)]">{title}</div>
        <div className="mt-0.5">{children}</div>
      </div>
    </div>
  );
}

export default function WorkspaceReviewSection({ embedded = false }: { embedded?: boolean }) {
  const activeProject = useProjectStore(selectActiveProject);
  const projectId = activeProject?.id ?? null;
  const projectName = activeProject?.name ?? null;
  const openModal = useUiStore((state) => state.openModal);
  const { registry: modelRegistry } = useAgentModels();
  const { data: reviewSettings, isLoading: isReviewLoading } =
    useReviewSettings();
  const { mutate: updateReviewSettings, isPending: isReviewUpdating } =
    useUpdateReviewSettings();
  const {
    settings: providerSettings,
    isPlaceholderData: isProviderPlaceholderData,
  } = useHarnessProviders({ refreshRuntime: true });
  const { rows: globalRows } = useWorkspaceReviewRuntimeSettings(null);
  const [activeTab, setActiveTab] = useState<"global" | "project">("global");

  const enabledProviders = providerSettings.providers.filter(
    (provider) =>
      isKnownHarness(provider.provider) &&
      provider.enabled &&
      provider.available,
  );
  const requiresProviderSetup =
    !isProviderPlaceholderData &&
    (providerSettings.requiresOnboarding || enabledProviders.length === 0);
  const disabledPublishGate =
    isReviewLoading || isReviewUpdating || !reviewSettings;

  const content = (
    <>
      <p className="mb-3 text-xs text-[var(--text-muted)]">
        Legacy runtime fallbacks apply only when the Reviewer role in Agents follows provider defaults.
      </p>
      {reviewSettings && (
        <>
          <ToggleSettingRow
            id="workspace-review-require-before-publish"
            label="Require Workspace Review before publishing"
            description="Block Commit & Publish until the workspace Review passes"
            checked={reviewSettings.require_workspace_review}
            disabled={disabledPublishGate}
            onChange={() =>
              updateReviewSettings({
                requireWorkspaceReview: !reviewSettings.require_workspace_review,
              })
            }
          />
          <ToggleSettingRow
            id="workspace-review-autofix-blocking-findings"
            label="Autofix Blocking Review Findings"
            description="Spawn the workspace repair agent when a Workspace Review returns blocking findings."
            checked={reviewSettings.autofix_workspace_review_blocking_findings}
            disabled={disabledPublishGate}
            onChange={() =>
              updateReviewSettings({
                autofixWorkspaceReviewBlockingFindings:
                  !reviewSettings.autofix_workspace_review_blocking_findings,
              })
            }
          />
          <NumberSettingRow
            id="workspace-review-fixer-cycle-cap"
            label="Maximum automatic fixer cycles"
            description="Maximum times RalphX automatically starts a Workspace Review fixer. Set 0 to turn off automatic fixing; manual fixes remain available."
            value={reviewSettings.workspace_review_fixer_cycle_cap}
            min={0}
            max={Number.MAX_SAFE_INTEGER}
            step={1}
            unit=""
            disabled={disabledPublishGate}
            onChange={(value) =>
              updateReviewSettings({
                workspaceReviewFixerCycleCap: Math.max(0, value),
              })
            }
          />
        </>
      )}
      {isProviderPlaceholderData && (
        <InlineNotice title="Loading Providers">
          Checking configured providers before showing Workspace Review defaults.
        </InlineNotice>
      )}
      {requiresProviderSetup && (
        <InlineNotice title="Provider Setup Required">
          <div className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
            <span>
              Enable and validate at least one provider before configuring legacy Workspace Review fallbacks.
            </span>
            <button
              type="button"
              onClick={() => openModal("settings", { section: "providers" })}
              className="inline-flex h-8 w-fit items-center gap-1.5 rounded-md border border-[var(--border-default)] bg-[var(--bg-elevated)] px-2.5 text-[0.6875rem] font-medium text-[var(--text-primary)] transition-colors hover:border-[var(--accent-primary)] hover:text-[var(--accent-primary)]"
            >
              Open Providers
              <ChevronRight className="h-3 w-3" />
            </button>
          </div>
        </InlineNotice>
      )}
      {!isProviderPlaceholderData && !requiresProviderSetup && (
        <Tabs
          value={activeTab}
          onValueChange={(value) => setActiveTab(value as "global" | "project")}
          className="w-full"
        >
          <TabsList className="inline-flex h-9 items-center rounded-md border border-[var(--border-subtle)] bg-[var(--bg-surface)] p-1 text-[var(--text-secondary)]">
            <TabsTrigger value="global" className="rounded-sm px-3 py-1 text-xs font-medium data-[state=active]:bg-[var(--bg-elevated)] data-[state=active]:text-[var(--text-primary)] data-[state=active]:shadow-sm">
              Global Defaults
            </TabsTrigger>
            <TabsTrigger value="project" className="rounded-sm px-3 py-1 text-xs font-medium data-[state=active]:bg-[var(--bg-elevated)] data-[state=active]:text-[var(--text-primary)] data-[state=active]:shadow-sm">
              Project Overrides
            </TabsTrigger>
          </TabsList>
          <TabsContent value="global" className="mt-4">
            <WorkspaceReviewScopeRows projectId={null} projectName={null} isGlobal={true} providers={enabledProviders} modelRegistry={modelRegistry} globalRows={globalRows} />
          </TabsContent>
          <TabsContent value="project" className="mt-4">
            <WorkspaceReviewScopeRows projectId={projectId} projectName={projectName} isGlobal={false} providers={enabledProviders} modelRegistry={modelRegistry} globalRows={globalRows} />
          </TabsContent>
        </Tabs>
      )}
    </>
  );
  return embedded ? content : (
    <SettingsSection>
      {content}
    </SettingsSection>
  );
}
