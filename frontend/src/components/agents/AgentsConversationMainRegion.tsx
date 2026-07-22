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
  hasAttachedPlanArtifact: ActiveConversationPanelProps["hasAttachedPlanArtifact"];
  hasAutoOpenArtifacts: ActiveConversationPanelProps["hasAutoOpenArtifacts"];
  isLoadingProjects: StartConversationPanelProps["isLoadingProjects"];
  modelRegistry: StartConversationPanelProps["modelRegistry"];
  normalizedActiveRuntime: ActiveConversationPanelProps["normalizedActiveRuntime"];
  onActiveConversationModeChange: ActiveConversationPanelProps["onActiveConversationModeChange"];
  onActiveConversationModeMenuOpen: ActiveConversationPanelProps["onActiveConversationModeMenuOpen"];
  onActiveCapabilityChange: ActiveConversationPanelProps["onActiveCapabilityChange"];
  onActiveEffortChange: ActiveConversationPanelProps["onActiveEffortChange"];
  onActiveModelChange: ActiveConversationPanelProps["onActiveModelChange"];
  onActiveProviderChange: ActiveConversationPanelProps["onActiveProviderChange"];
  onAgentUserMessageSent: ActiveConversationPanelProps["onAgentUserMessageSent"];
  onConversationModeSwitched: ActiveConversationPanelProps["onConversationModeSwitched"];
  onFocusIdeationSession: ActiveConversationPanelProps["onFocusIdeationSession"];
  onFocusIdeationSessionForConversation: ActiveConversationPanelProps[
    "onFocusIdeationSessionForConversation"
  ];
  onFocusWorkspaceReview: ActiveConversationPanelProps["onFocusWorkspaceReview"];
  onFocusVerificationSession: ActiveConversationPanelProps["onFocusVerificationSession"];
  onFocusTaskRuntime: ActiveConversationPanelProps["onFocusTaskRuntime"];
  onFocusAutomationRun: ActiveConversationPanelProps["onFocusAutomationRun"];
  onOpenTaskArtifact: ActiveConversationPanelProps["onOpenTaskArtifact"];
  onOpenAutomation?: ActiveConversationPanelProps["onOpenAutomation"];
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
  onStartPersonaBuilder: ActiveConversationPanelProps["onStartPersonaBuilder"];
  onToggleArtifacts: ActiveConversationPanelProps["onToggleArtifacts"];
  onSelectChatFocus: ActiveConversationPanelProps["onSelectChatFocus"];
  projects: StartConversationPanelProps["projects"];
  publishShortcutLabel: ActiveConversationPanelProps["publishShortcutLabel"];
  publishShortcutWorkspace?: ActiveConversationPanelProps["publishShortcutWorkspace"];
  suppressPublishShortcut?: ActiveConversationPanelProps["suppressPublishShortcut"];
  promotePublishShortcut?: ActiveConversationPanelProps["promotePublishShortcut"];
  publishingConversationId: ActiveConversationPanelProps["publishingConversationId"];
  selectedConversationId: string | null;
  selectedTaskArtifactId: ActiveConversationPanelProps["selectedTaskArtifactId"];
  setTerminalChatDockElement: ActiveConversationPanelProps["setTerminalChatDockElement"];
  switchingConversationModeId: ActiveConversationPanelProps["switchingConversationModeId"];
  updatingCapabilityConversationId: ActiveConversationPanelProps["updatingCapabilityConversationId"];
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
  hasAttachedPlanArtifact,
  hasAutoOpenArtifacts,
  isLoadingProjects,
  modelRegistry,
  normalizedActiveRuntime,
  onActiveConversationModeChange,
  onActiveConversationModeMenuOpen,
  onActiveCapabilityChange,
  onActiveEffortChange,
  onActiveModelChange,
  onActiveProviderChange,
  onAgentUserMessageSent,
  onConversationModeSwitched,
  onFocusIdeationSession,
  onFocusIdeationSessionForConversation,
  onFocusWorkspaceReview,
  onFocusVerificationSession,
  onFocusTaskRuntime,
  onFocusAutomationRun,
  onOpenTaskArtifact,
  onOpenAutomation,
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
  onStartPersonaBuilder,
  onToggleArtifacts,
  onSelectChatFocus,
  projects,
  publishShortcutLabel,
  publishShortcutWorkspace = null,
  suppressPublishShortcut = false,
  promotePublishShortcut = false,
  publishingConversationId,
  selectedConversationId,
  selectedTaskArtifactId,
  setTerminalChatDockElement,
  switchingConversationModeId,
  updatingCapabilityConversationId,
  terminalArchivedReason,
  terminalUnavailableReason,
}: AgentsConversationMainRegionProps) {
  if (selectedConversationId && activeConversation) {
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
        hasAttachedPlanArtifact={hasAttachedPlanArtifact}
        hasAutoOpenArtifacts={hasAutoOpenArtifacts}
        normalizedActiveRuntime={normalizedActiveRuntime}
        onActiveConversationModeChange={onActiveConversationModeChange}
        onActiveConversationModeMenuOpen={onActiveConversationModeMenuOpen}
        onActiveCapabilityChange={onActiveCapabilityChange}
        onActiveEffortChange={onActiveEffortChange}
        onActiveModelChange={onActiveModelChange}
        onActiveProviderChange={onActiveProviderChange}
        onAgentUserMessageSent={onAgentUserMessageSent}
        onConversationModeSwitched={onConversationModeSwitched}
        onFocusIdeationSession={onFocusIdeationSession}
        onFocusIdeationSessionForConversation={
          onFocusIdeationSessionForConversation
        }
        onFocusWorkspaceReview={onFocusWorkspaceReview}
        onFocusVerificationSession={onFocusVerificationSession}
        onFocusTaskRuntime={onFocusTaskRuntime}
        onFocusAutomationRun={onFocusAutomationRun}
        onOpenTaskArtifact={onOpenTaskArtifact}
        {...(onOpenAutomation ? { onOpenAutomation } : {})}
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
        onStartPersonaBuilder={onStartPersonaBuilder}
        publishShortcutLabel={publishShortcutLabel}
        publishShortcutWorkspace={publishShortcutWorkspace}
        suppressPublishShortcut={suppressPublishShortcut}
        promotePublishShortcut={promotePublishShortcut}
        publishingConversationId={publishingConversationId}
        selectedConversationId={selectedConversationId}
        selectedTaskArtifactId={selectedTaskArtifactId}
        setTerminalChatDockElement={setTerminalChatDockElement}
        switchingConversationModeId={switchingConversationModeId}
        updatingCapabilityConversationId={updatingCapabilityConversationId}
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
      onRuntimePreferenceChange={onRuntimePreferenceChange}
      onStartAgentConversation={onStartAgentConversation}
    />
  );
});
