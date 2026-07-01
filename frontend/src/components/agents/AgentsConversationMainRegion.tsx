import { memo, type ComponentProps } from "react";

import { AgentsActiveConversationPanel } from "./AgentsActiveConversationPanel";
import { AgentsStartConversationPanel } from "./AgentsStartConversationPanel";

type ActiveConversationPanelProps = ComponentProps<typeof AgentsActiveConversationPanel>;
type StartConversationPanelProps = ComponentProps<typeof AgentsStartConversationPanel>;

interface AgentsConversationMainRegionProps {
  activeConversation: ActiveConversationPanelProps["activeConversation"] | null;
  activeConversationMode: ActiveConversationPanelProps["activeConversationMode"];
  activeConversationModeLocked: ActiveConversationPanelProps["activeConversationModeLocked"];
  activeProjectId: string | null;
  activeProjectOptions: ActiveConversationPanelProps["activeProjectOptions"];
  activeWorkspace: ActiveConversationPanelProps["activeWorkspace"];
  activeWorkspaceFreshness: ActiveConversationPanelProps["activeWorkspaceFreshness"];
  attachedIdeationSessionId: ActiveConversationPanelProps["attachedIdeationSessionId"];
  availableArtifactTabs: ActiveConversationPanelProps["availableArtifactTabs"];
  chatFocus: ActiveConversationPanelProps["chatFocus"];
  chatFocusOptions: ActiveConversationPanelProps["chatFocusOptions"];
  defaultProjectId: StartConversationPanelProps["defaultProjectId"];
  defaultRuntime: StartConversationPanelProps["defaultRuntime"];
  hasAutoOpenArtifacts: ActiveConversationPanelProps["hasAutoOpenArtifacts"];
  isLoadingProjects: StartConversationPanelProps["isLoadingProjects"];
  modelRegistry: StartConversationPanelProps["modelRegistry"];
  normalizedActiveRuntime: ActiveConversationPanelProps["normalizedActiveRuntime"];
  workspaceReviewRuntimeStatus: ActiveConversationPanelProps["workspaceReviewRuntimeStatus"];
  onActiveConversationModeChange: ActiveConversationPanelProps["onActiveConversationModeChange"];
  onActiveConversationModeMenuOpen: ActiveConversationPanelProps["onActiveConversationModeMenuOpen"];
  onActiveEffortChange: ActiveConversationPanelProps["onActiveEffortChange"];
  onActiveModelChange: ActiveConversationPanelProps["onActiveModelChange"];
  onActiveProviderChange: ActiveConversationPanelProps["onActiveProviderChange"];
  onAgentUserMessageSent: ActiveConversationPanelProps["onAgentUserMessageSent"];
  onConversationModeSwitched: ActiveConversationPanelProps["onConversationModeSwitched"];
  onCreateProject: StartConversationPanelProps["onCreateProject"];
  onFocusIdeationSession: ActiveConversationPanelProps["onFocusIdeationSession"];
  onFocusWorkspaceReview: ActiveConversationPanelProps["onFocusWorkspaceReview"];
  onFocusVerificationSession: ActiveConversationPanelProps["onFocusVerificationSession"];
  onFocusTaskRuntime: ActiveConversationPanelProps["onFocusTaskRuntime"];
  onOpenTaskArtifact: ActiveConversationPanelProps["onOpenTaskArtifact"];
  onForkConversation: ActiveConversationPanelProps["onForkConversation"];
  onOpenPlanArtifact: ActiveConversationPanelProps["onOpenPlanArtifact"];
  onOpenPublishPane: ActiveConversationPanelProps["onOpenPublishPane"];
  onOpenPublishFile: ActiveConversationPanelProps["onOpenPublishFile"];
  onPreloadArtifacts: ActiveConversationPanelProps["onPreloadArtifacts"];
  onPublishWorkspace: ActiveConversationPanelProps["onPublishWorkspace"];
  onRenameConversation: ActiveConversationPanelProps["onRenameConversation"];
  onRuntimePreferenceChange: StartConversationPanelProps["onRuntimePreferenceChange"];
  onSelectArtifact: ActiveConversationPanelProps["onSelectArtifact"];
  onStartAgentConversation: StartConversationPanelProps["onStartAgentConversation"];
  onToggleArtifacts: ActiveConversationPanelProps["onToggleArtifacts"];
  onSelectChatFocus: ActiveConversationPanelProps["onSelectChatFocus"];
  projects: StartConversationPanelProps["projects"];
  publishShortcutLabel: ActiveConversationPanelProps["publishShortcutLabel"];
  promotePublishShortcut?: ActiveConversationPanelProps["promotePublishShortcut"];
  publishingConversationId: ActiveConversationPanelProps["publishingConversationId"];
  selectedConversationId: string | null;
  selectedTaskArtifactId: ActiveConversationPanelProps["selectedTaskArtifactId"];
  setTerminalChatDockElement: ActiveConversationPanelProps["setTerminalChatDockElement"];
  switchingConversationModeId: ActiveConversationPanelProps["switchingConversationModeId"];
  terminalArchivedReason: ActiveConversationPanelProps["terminalArchivedReason"];
  terminalUnavailableReason: ActiveConversationPanelProps["terminalUnavailableReason"];
}

export const AgentsConversationMainRegion = memo(function AgentsConversationMainRegion({
  activeConversation,
  activeConversationMode,
  activeConversationModeLocked,
  activeProjectId,
  activeProjectOptions,
  activeWorkspace,
  activeWorkspaceFreshness,
  attachedIdeationSessionId,
  availableArtifactTabs,
  chatFocus,
  chatFocusOptions,
  defaultProjectId,
  defaultRuntime,
  hasAutoOpenArtifacts,
  isLoadingProjects,
  modelRegistry,
  normalizedActiveRuntime,
  workspaceReviewRuntimeStatus = null,
  onActiveConversationModeChange,
  onActiveConversationModeMenuOpen,
  onActiveEffortChange,
  onActiveModelChange,
  onActiveProviderChange,
  onAgentUserMessageSent,
  onConversationModeSwitched,
  onCreateProject,
  onFocusIdeationSession,
  onFocusWorkspaceReview,
  onFocusVerificationSession,
  onFocusTaskRuntime,
  onOpenTaskArtifact,
  onForkConversation,
  onOpenPlanArtifact,
  onOpenPublishPane,
  onOpenPublishFile,
  onPreloadArtifacts,
  onPublishWorkspace,
  onRenameConversation,
  onRuntimePreferenceChange,
  onSelectArtifact,
  onStartAgentConversation,
  onToggleArtifacts,
  onSelectChatFocus,
  projects,
  publishShortcutLabel,
  promotePublishShortcut = false,
  publishingConversationId,
  selectedConversationId,
  selectedTaskArtifactId,
  setTerminalChatDockElement,
  switchingConversationModeId,
  terminalArchivedReason,
  terminalUnavailableReason,
}: AgentsConversationMainRegionProps) {
  if (activeProjectId && selectedConversationId && activeConversation) {
    return (
      <AgentsActiveConversationPanel
        activeConversation={activeConversation}
        activeConversationMode={activeConversationMode}
        activeConversationModeLocked={activeConversationModeLocked}
        activeProjectId={activeProjectId}
        activeProjectOptions={activeProjectOptions}
        activeWorkspace={activeWorkspace}
        activeWorkspaceFreshness={activeWorkspaceFreshness}
        attachedIdeationSessionId={attachedIdeationSessionId}
        availableArtifactTabs={availableArtifactTabs}
        chatFocus={chatFocus}
        chatFocusOptions={chatFocusOptions}
        hasAutoOpenArtifacts={hasAutoOpenArtifacts}
        normalizedActiveRuntime={normalizedActiveRuntime}
        workspaceReviewRuntimeStatus={workspaceReviewRuntimeStatus}
        onActiveConversationModeChange={onActiveConversationModeChange}
        onActiveConversationModeMenuOpen={onActiveConversationModeMenuOpen}
        onActiveEffortChange={onActiveEffortChange}
        onActiveModelChange={onActiveModelChange}
        onActiveProviderChange={onActiveProviderChange}
        onAgentUserMessageSent={onAgentUserMessageSent}
        onConversationModeSwitched={onConversationModeSwitched}
        onFocusIdeationSession={onFocusIdeationSession}
        onFocusWorkspaceReview={onFocusWorkspaceReview}
        onFocusVerificationSession={onFocusVerificationSession}
        onFocusTaskRuntime={onFocusTaskRuntime}
        onOpenTaskArtifact={onOpenTaskArtifact}
        onForkConversation={onForkConversation}
        onOpenPlanArtifact={onOpenPlanArtifact}
        onOpenPublishPane={onOpenPublishPane}
        onOpenPublishFile={onOpenPublishFile}
        onPreloadArtifacts={onPreloadArtifacts}
        onPublishWorkspace={onPublishWorkspace}
        onRenameConversation={onRenameConversation}
        onSelectArtifact={onSelectArtifact}
        onToggleArtifacts={onToggleArtifacts}
        onSelectChatFocus={onSelectChatFocus}
        publishShortcutLabel={publishShortcutLabel}
        promotePublishShortcut={promotePublishShortcut}
        publishingConversationId={publishingConversationId}
        selectedConversationId={selectedConversationId}
        selectedTaskArtifactId={selectedTaskArtifactId}
        setTerminalChatDockElement={setTerminalChatDockElement}
        switchingConversationModeId={switchingConversationModeId}
        terminalArchivedReason={terminalArchivedReason}
        terminalUnavailableReason={terminalUnavailableReason}
      />
    );
  }

  return (
    <AgentsStartConversationPanel
      projects={projects}
      defaultProjectId={defaultProjectId}
      defaultRuntime={defaultRuntime}
      isLoadingProjects={isLoadingProjects}
      modelRegistry={modelRegistry}
      onCreateProject={onCreateProject}
      onRuntimePreferenceChange={onRuntimePreferenceChange}
      onStartAgentConversation={onStartAgentConversation}
    />
  );
});
