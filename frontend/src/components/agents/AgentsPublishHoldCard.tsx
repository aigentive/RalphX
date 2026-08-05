import { Clock3, ListChecks, PauseCircle, Sparkles } from "lucide-react";

import type { AgentConversationWorkspace } from "@/api/chat";
import { Button } from "@/components/ui/button";
import {
  getAgentWorkspaceHoldPresentation,
  getAgentWorkspacePrAutofixFingerprintSpendPresentation,
} from "./agentWorkspacePublishState";
import { PublishFact } from "./AgentsPublishFact";

export function AgentsPublishHoldCard({
  workspace,
  onOpenChecks,
  onRecheck,
  onRetry,
  onStop,
  isPending = false,
}: {
  workspace: AgentConversationWorkspace;
  onOpenChecks: () => void;
  onRecheck: () => void;
  onRetry: () => void;
  onStop: () => void;
  isPending?: boolean;
}) {
  const hold = getAgentWorkspaceHoldPresentation(workspace);
  const spend = getAgentWorkspacePrAutofixFingerprintSpendPresentation(workspace);
  if (!hold) {
    return null;
  }

  return (
    <section
      className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto rounded-lg p-4"
      data-testid="agents-publish-hold-card"
      style={{
        backgroundColor: "var(--bg-surface, #1c1c22)",
        borderColor: "var(--status-warning-border, #8a5a16)",
        borderStyle: "solid",
        borderWidth: "1px",
      }}
    >
      <div className="flex items-start gap-3">
        <PauseCircle
          aria-hidden="true"
          className="mt-0.5 h-5 w-5 shrink-0 text-[var(--status-warning)]"
        />
        <div className="min-w-0">
          <h3 className="text-sm font-semibold text-[var(--text-primary)]">
            {hold.title}
          </h3>
          <p className="mt-1 text-xs leading-relaxed text-[var(--text-secondary)]">
            <span>{hold.agentStatus}</span>. {hold.waitingOn}
          </p>
        </div>
      </div>

      <div className="grid gap-2 @lg:grid-cols-2">
        <PublishFact
          icon={ListChecks}
          label="Failed checks"
          value="Open the Checks tab"
          description="The failing check is shown with its latest GitHub result."
          action={{
            label: "Open failed checks",
            testId: "agents-publish-hold-open-checks",
            onClick: onOpenChecks,
          }}
        />
        {spend && (
          <PublishFact
            icon={Clock3}
            label="Repair spend"
            value={spend.summary}
            description={spend.exhausted ? "Repair budget exhausted" : null}
          />
        )}
        <PublishFact
          icon={Sparkles}
          label="Last generation"
          value="Repair verdict"
          description={hold.generationVerdict}
        />
        <PublishFact
          icon={Clock3}
          label="Waiting on"
          value={hold.waitingOnLabel}
          description={hold.waitingOn}
        />
      </div>

      <div
        className="flex flex-wrap items-center gap-2 border-t pt-3"
        style={{
          borderColor: "var(--border-subtle, #3b3b43)",
          borderStyle: "solid",
          borderWidth: "1px 0 0",
        }}
      >
        <Button type="button" onClick={onRecheck} disabled={isPending}>
          Re-check PR health
        </Button>
        <Button
          type="button"
          variant="outline"
          onClick={onRetry}
          disabled={isPending}
        >
          Retry repair anyway{spend ? ` · ${spend.summary}` : ""}
        </Button>
        <Button
          type="button"
          variant="ghost"
          onClick={onStop}
          disabled={isPending}
        >
          Stop auto-repair
        </Button>
      </div>
    </section>
  );
}
