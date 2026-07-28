import { useEffect, type ReactNode } from "react";
import { useQuery } from "@tanstack/react-query";

import { agentTaskApi } from "@/api/agent-tasks";
import { StatusPill } from "@/components/ui/status-pill";
import { agentWorkspaceKeys } from "@/components/agents/agentWorkspaceQueries";
import {
  useEnsureManagedTeam,
  useManagedTeamStatus,
} from "@/hooks/useManagedTeam";

import { TeamActivity } from "./TeamActivity";
import { TeamMemberActions } from "./TeamMemberActions";
import { TeamRoster } from "./TeamRoster";
import { TeamTaskBoard } from "./TeamTaskBoard";
import { TeamUsage } from "./TeamUsage";

export function TeamPanelContent({
  conversationId,
  projectId,
  activeAgentRunId,
}: {
  conversationId: string;
  projectId: string | null;
  activeAgentRunId: string | null;
}) {
  const status = useManagedTeamStatus(conversationId);
  const { mutate: ensureTeam, isPending: isEnsuring } = useEnsureManagedTeam(conversationId);
  const board = useQuery({
    queryKey: agentWorkspaceKeys.agentTasksForScope("conversation", conversationId),
    queryFn: () => agentTaskApi.listConversationTasks({ conversationId, projectId, includeDone: true }),
    staleTime: 5_000,
  });
  const team = status.data;

  useEffect(() => {
    if (!projectId || team !== null || !status.isSuccess || isEnsuring) return;
    ensureTeam(projectId);
  }, [ensureTeam, isEnsuring, projectId, status.isSuccess, team]);

  if (status.isLoading || (status.isSuccess && team === null && isEnsuring)) {
    return <TeamPanelLoading />;
  }
  if (status.isError) {
    return <TeamPanelMessage message="Could not load this Team." />;
  }
  if (!team) {
    return <TeamPanelMessage message="Team setup needs a project-backed conversation." />;
  }

  const authority = activeAgentRunId
    ? { conversationId, agentRunId: activeAgentRunId }
    : null;
  const activeTasks = board.data?.filter((task) => task.state === "active").length ?? 0;

  return (
    <div className="space-y-5 p-4" data-testid="agents-team-panel-content">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div>
          <h2 className="text-sm font-semibold" style={{ color: "var(--text-primary)" }}>Team</h2>
          <p className="text-xs" style={{ color: "var(--text-muted)" }}>
            {team.members.length} members · {activeTasks} active board tasks
          </p>
        </div>
        <StatusPill label={team.session.status} tone={team.session.status === "active" ? "accent" : "neutral"} live={team.session.status === "active"} />
      </div>

      <TeamUsage status={team} />
      <PanelSection title="Roster"><TeamRoster members={team.members} /></PanelSection>
      <PanelSection title="Board"><TeamTaskBoard conversationId={conversationId} projectId={projectId} /></PanelSection>
      <PanelSection title="Activity"><TeamActivity members={team.members} /></PanelSection>
      <TeamMemberActions conversationId={conversationId} authority={authority} members={team.members} tasks={board.data ?? []} />
    </div>
  );
}

function PanelSection({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="space-y-2">
      <h3 className="text-sm font-semibold" style={{ color: "var(--text-primary)" }}>{title}</h3>
      {children}
    </section>
  );
}

function TeamPanelLoading() {
  return <div className="p-4 text-sm" style={{ color: "var(--text-muted)" }} data-testid="agents-team-panel-loading">Loading Team…</div>;
}

function TeamPanelMessage({ message }: { message: string }) {
  return <div className="p-4 text-sm" style={{ color: "var(--text-muted)" }}>{message}</div>;
}
