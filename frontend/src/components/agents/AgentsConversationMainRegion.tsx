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
  attachedIdeationSessionId: ActiveConversationPanelProps["attachedIdeationSessionId"];
  availableArtifactTabs: ActiveConversationPanelProps["availableArtifactTabs"];
  chatFocus: ActiveConversationPanelProps["chatFocus"];
  chatFocusOptions: ActiveConversationPanelProps["chatFocusOptions"];
  defaultProjectId: StartConversationPanelProps["defaultProjectId"];
  defaultRuntime: StartConversationPanelProps["defaultRuntime"];
  hasAutoOpenArtifacts: ActiveConversationPanelProps["hasAutoOpenArtifacts"];
  isLoadingProjects: StartConversationPanelProps["isLoadingProjects"];
  normalizedActiveRuntime: ActiveConversationPanelProps["normalizedActiveRuntime"];
  onActiveConversationModeChange: ActiveConversationPanelProps["onActiveConversationModeChange"];
  onActiveModelChange: ActiveConversationPanelProps["onActiveModelChange"];
  onAgentUserMessageSent: ActiveConversationPanelProps["onAgentUserMessageSent"];
  onCreateProject: StartConversationPanelProps["onCreateProject"];
  onFocusIdeationSession: ActiveConversationPanelProps["onFocusIdeationSession"];
  onOpenPublishPane: ActiveConversationPanelProps["onOpenPublishPane"];
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
  publishingConversationId: ActiveConversationPanelProps["publishingConversationId"];
  selectedConversationId: string | null;
  setTerminalChatDockElement: ActiveConversationPanelProps["setTerminalChatDockElement"];
  switchingConversationModeId: ActiveConversationPanelProps["switchingConversationModeId"];
  terminalUnavailableReason: ActiveConversationPanelProps["terminalUnavailableReason"];
}

export const AgentsConversationMainRegion = memo(function AgentsConversationMainRegion({
  activeConversation,
  activeConversationMode,
  activeConversationModeLocked,
  activeProjectId,
  activeProjectOptions,
  activeWorkspace,
  attachedIdeationSessionId,
  availableArtifactTabs,
  chatFocus,
  chatFocusOptions,
  defaultProjectId,
  defaultRuntime,
  hasAutoOpenArtifacts,
  isLoadingProjects,
  normalizedActiveRuntime,
  onActiveConversationModeChange,
  onActiveModelChange,
  onAgentUserMessageSent,
  onCreateProject,
  onFocusIdeationSession,
  onOpenPublishPane,
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
  publishingConversationId,
  selectedConversationId,
  setTerminalChatDockElement,
  switchingConversationModeId,
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
        attachedIdeationSessionId={attachedIdeationSessionId}
        availableArtifactTabs={availableArtifactTabs}
        chatFocus={chatFocus}
        chatFocusOptions={chatFocusOptions}
        hasAutoOpenArtifacts={hasAutoOpenArtifacts}
        normalizedActiveRuntime={normalizedActiveRuntime}
        onActiveConversationModeChange={onActiveConversationModeChange}
        onActiveModelChange={onActiveModelChange}
        onAgentUserMessageSent={onAgentUserMessageSent}
        onFocusIdeationSession={onFocusIdeationSession}
        onOpenPublishPane={onOpenPublishPane}
        onPreloadArtifacts={onPreloadArtifacts}
        onPublishWorkspace={onPublishWorkspace}
        onRenameConversation={onRenameConversation}
        onSelectArtifact={onSelectArtifact}
        onToggleArtifacts={onToggleArtifacts}
        onSelectChatFocus={onSelectChatFocus}
        publishShortcutLabel={publishShortcutLabel}
        publishingConversationId={publishingConversationId}
        selectedConversationId={selectedConversationId}
        setTerminalChatDockElement={setTerminalChatDockElement}
        switchingConversationModeId={switchingConversationModeId}
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
      onCreateProject={onCreateProject}
      onRuntimePreferenceChange={onRuntimePreferenceChange}
      onStartAgentConversation={onStartAgentConversation}
    />
  );
});
