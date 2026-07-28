import { useMemo, useState } from "react";
import { UserPlus } from "lucide-react";
import { toast } from "sonner";

import type { AgentTaskSummary } from "@/api/agent-tasks";
import type { ManagedTeamMember } from "@/api/managed-team";
import { Button } from "@/components/ui/button";
import { useConfirmation } from "@/hooks/useConfirmation";
import { useManagedTeamMemberActions } from "@/hooks/useManagedTeam";
import { extractErrorMessage } from "@/lib/errors";

export function TeamMemberActions({
  conversationId,
  authority,
  members,
  tasks,
}: {
  conversationId: string;
  authority: { conversationId: string; agentRunId: string } | null;
  members: readonly ManagedTeamMember[];
  tasks: readonly AgentTaskSummary[];
}) {
  const { addMember, assignMember, stopMember } = useManagedTeamMemberActions(conversationId);
  const { confirm, confirmationDialogProps, ConfirmationDialog } = useConfirmation();
  const [showAddForm, setShowAddForm] = useState(false);
  const [name, setName] = useState("");
  const [canonicalAgentName, setCanonicalAgentName] = useState("");
  const [roleSummary, setRoleSummary] = useState("");
  const availableTasks = useMemo(
    () => tasks.filter((task) => task.state === "open" || task.state === "active"),
    [tasks],
  );
  const actionsDisabled = authority === null;

  const add = async () => {
    if (!authority || !name.trim() || !canonicalAgentName.trim() || !roleSummary.trim()) return;
    const accepted = await confirm({
      title: `Add ${name.trim()} to the Team?`,
      description: "This creates a standing idle member. It does not start a provider process.",
      confirmText: "Add member",
    });
    if (!accepted) return;
    try {
      await addMember.mutateAsync({
        authority,
        name: name.trim(),
        canonicalAgentName: canonicalAgentName.trim(),
        roleSummary: roleSummary.trim(),
      });
      setName("");
      setCanonicalAgentName("");
      setRoleSummary("");
      setShowAddForm(false);
    } catch (error) {
      toast.error(extractErrorMessage(error, "Could not add Team member."));
    }
  };

  const assign = async (member: ManagedTeamMember, task: AgentTaskSummary) => {
    if (!authority) return;
    const accepted = await confirm({
      title: `Assign #${task.taskNumber} to ${member.name}?`,
      description: "The coordinator task ledger remains the authoritative board.",
      confirmText: "Assign task",
    });
    if (!accepted) return;
    try {
      await assignMember.mutateAsync({
        authority,
        memberName: member.name,
        taskRef: String(task.taskNumber),
        workClassification: "read_only",
      });
    } catch (error) {
      toast.error(extractErrorMessage(error, "Could not assign Team member."));
    }
  };

  const stop = async (member: ManagedTeamMember) => {
    if (!authority) return;
    const accepted = await confirm({
      title: `Stop ${member.name}?`,
      description: "Stopping a member cancels its current turn and reopens any unsettled assignment.",
      confirmText: "Stop member",
      variant: "destructive",
    });
    if (!accepted) return;
    try {
      await stopMember.mutateAsync({ authority, memberName: member.name });
    } catch (error) {
      toast.error(extractErrorMessage(error, "Could not stop Team member."));
    }
  };

  return (
    <section className="space-y-3" data-testid="team-member-actions">
      <div className="flex items-center justify-between gap-2">
        <h3 className="text-sm font-semibold" style={{ color: "var(--text-primary)" }}>Member actions</h3>
        <Button
          type="button"
          size="sm"
          variant="outline"
          disabled={actionsDisabled}
          onClick={() => setShowAddForm((visible) => !visible)}
        >
          <UserPlus className="h-3.5 w-3.5" /> Add member
        </Button>
      </div>
      {actionsDisabled ? (
        <p className="text-xs" style={{ color: "var(--text-muted)" }}>
          Start the coordinator to manage the roster.
        </p>
      ) : null}
      {showAddForm ? (
        <div
          className="grid gap-2 rounded-lg border p-3"
          style={{
            backgroundColor: "var(--bg-base)",
            borderColor: "var(--border-subtle)",
            borderStyle: "solid",
            borderWidth: 1,
          }}
        >
          <input value={name} onChange={(event) => setName(event.target.value)} placeholder="Member name" aria-label="Team member name" className="h-8 rounded-md border px-2 text-sm" style={{ backgroundColor: "var(--bg-surface)", borderColor: "var(--form-border)", color: "var(--text-primary)" }} />
          <input value={canonicalAgentName} onChange={(event) => setCanonicalAgentName(event.target.value)} placeholder="Canonical agent name" aria-label="Canonical agent name" className="h-8 rounded-md border px-2 text-sm" style={{ backgroundColor: "var(--bg-surface)", borderColor: "var(--form-border)", color: "var(--text-primary)" }} />
          <input value={roleSummary} onChange={(event) => setRoleSummary(event.target.value)} placeholder="Standing role" aria-label="Team member role" className="h-8 rounded-md border px-2 text-sm" style={{ backgroundColor: "var(--bg-surface)", borderColor: "var(--form-border)", color: "var(--text-primary)" }} />
          <Button type="button" size="sm" onClick={() => void add()} disabled={addMember.isPending}>Add standing member</Button>
        </div>
      ) : null}
      {members.map((member) => {
        const task = availableTasks[0];
        return (
          <div key={member.id} className="flex flex-wrap items-center justify-between gap-2 text-xs">
            <span style={{ color: "var(--text-secondary)" }}>{member.name}</span>
            <div className="flex gap-1.5">
              <Button type="button" size="sm" variant="outline" disabled={!task || actionsDisabled || assignMember.isPending} onClick={() => task && void assign(member, task)}>Assign</Button>
              <Button type="button" size="sm" variant="outline" disabled={actionsDisabled || stopMember.isPending} onClick={() => void stop(member)}>Stop</Button>
            </div>
          </div>
        );
      })}
      <ConfirmationDialog {...confirmationDialogProps} />
    </section>
  );
}
