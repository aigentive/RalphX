import { useMemo, type ComponentProps } from "react";

import { AgentsSidebar } from "./AgentsSidebar";

type AgentsSidebarShellProps = Omit<ComponentProps<typeof AgentsSidebar>, "onCollapse">;

interface UseAgentsSidebarPropsParams {
  defaultProjectId: string | null;
  focusedProjectId: string | null;
  onArchiveConversation: AgentsSidebarShellProps["onArchiveConversation"];
  onBulkArchiveConversations: AgentsSidebarShellProps["onBulkArchiveConversations"];
  onBulkMuteConversations: AgentsSidebarShellProps["onBulkMuteConversations"];
  onSetConversationMuted: AgentsSidebarShellProps["onSetConversationMuted"];
  onArchiveProject: AgentsSidebarShellProps["onArchiveProject"];
  onAutoRenameConversation: AgentsSidebarShellProps["onAutoRenameConversation"];
  onCreateAgent: AgentsSidebarShellProps["onCreateAgent"];
  onCreateProject: AgentsSidebarShellProps["onCreateProject"];
  onForkConversation: AgentsSidebarShellProps["onForkConversation"];
  onFocusProject: AgentsSidebarShellProps["onFocusProject"];
  onRenameConversation: AgentsSidebarShellProps["onRenameConversation"];
  onRestoreConversation: AgentsSidebarShellProps["onRestoreConversation"];
  onSelectConversation: AgentsSidebarShellProps["onSelectConversation"];
  onShowArchivedChange: AgentsSidebarShellProps["onShowArchivedChange"];
  pinnedConversation: AgentsSidebarShellProps["pinnedConversation"];
  projects: AgentsSidebarShellProps["projects"];
  selectedConversationId: AgentsSidebarShellProps["selectedConversationId"];
  showArchived: AgentsSidebarShellProps["showArchived"];
}

export function useAgentsSidebarProps({
  defaultProjectId,
  focusedProjectId,
  onArchiveConversation,
  onBulkArchiveConversations,
  onBulkMuteConversations,
  onSetConversationMuted,
  onArchiveProject,
  onAutoRenameConversation,
  onCreateAgent,
  onCreateProject,
  onForkConversation,
  onFocusProject,
  onRenameConversation,
  onRestoreConversation,
  onSelectConversation,
  onShowArchivedChange,
  pinnedConversation,
  projects,
  selectedConversationId,
  showArchived,
}: UseAgentsSidebarPropsParams): AgentsSidebarShellProps {
  return useMemo(
    () => ({
      projects,
      focusedProjectId: focusedProjectId ?? defaultProjectId,
      selectedConversationId,
      pinnedConversation: pinnedConversation ?? null,
      onFocusProject,
      onSelectConversation,
      onCreateAgent,
      onCreateProject,
      onForkConversation,
      onArchiveProject,
      onAutoRenameConversation,
      onRenameConversation,
      onArchiveConversation,
      onBulkArchiveConversations,
      onBulkMuteConversations,
      onSetConversationMuted,
      onRestoreConversation,
      showArchived,
      onShowArchivedChange,
    } as const),
    [
      defaultProjectId,
      focusedProjectId,
      onArchiveConversation,
      onBulkArchiveConversations,
      onBulkMuteConversations,
      onSetConversationMuted,
      onArchiveProject,
      onAutoRenameConversation,
      onCreateAgent,
      onCreateProject,
      onForkConversation,
      onFocusProject,
      onRenameConversation,
      onRestoreConversation,
      onSelectConversation,
      onShowArchivedChange,
      pinnedConversation,
      projects,
      selectedConversationId,
      showArchived,
    ],
  );
}
