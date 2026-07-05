import type { AgentComposerPlanReference } from "@/api/agent-composer";
import { planTitle, samePlanReference } from "./AgentPlanStartPanel.utils";

type AgentPlanStartPanelStatus = "idle" | "loading" | "error" | "pending";

export function AgentPlanStartPanelSearch({
  plans,
  status,
  isLoading,
  isError,
  selectedPlan,
  onSelectPlan,
}: {
  plans: AgentComposerPlanReference[];
  status: AgentPlanStartPanelStatus;
  isLoading: boolean;
  isError: boolean;
  selectedPlan: AgentComposerPlanReference | null;
  onSelectPlan: (plan: AgentComposerPlanReference) => void;
}) {
  if (status !== "idle") {
    return null;
  }
  if (isLoading) {
    return <PlanResultState label="Loading project plans..." />;
  }
  if (isError) {
    return <PlanResultState label="Plan search failed" role="alert" />;
  }
  if (plans.length === 0) {
    return <PlanResultState label="No plans found" />;
  }

  return (
    <div className="mt-3 flex flex-col gap-2">
      {plans.map((plan) => {
        const selected =
          selectedPlan !== null && samePlanReference(plan, selectedPlan);
        return (
          <button
            key={`${plan.sessionId}:${plan.artifactId}`}
            type="button"
            aria-label={`Select plan ${planTitle(plan)}`}
            onClick={() => onSelectPlan(plan)}
            className="rounded-md px-3 py-2 text-left text-sm transition-colors"
            style={{
              backgroundColor: selected ? "var(--accent-muted)" : "var(--bg-elevated)",
              borderColor: selected ? "var(--accent-primary)" : "var(--overlay-faint)",
              borderWidth: 1,
              borderStyle: "solid",
              color: "var(--text-primary)",
            }}
          >
            <span className="block truncate font-medium">{planTitle(plan)}</span>
            <span
              className="mt-1 flex flex-wrap gap-x-2 gap-y-1 text-xs"
              style={{ color: "var(--text-muted)" }}
            >
              <span>v{plan.artifactVersion}</span>
              <span>{plan.status}</span>
              <span>{new Date(plan.updatedAt).toLocaleDateString()}</span>
            </span>
          </button>
        );
      })}
    </div>
  );
}

function PlanResultState({
  label,
  role = "status",
}: {
  label: string;
  role?: "status" | "alert";
}) {
  return (
    <div
      className="mt-3 rounded-md px-3 py-3 text-sm"
      style={{
        backgroundColor: "var(--bg-elevated)",
        color: "var(--text-secondary)",
        borderColor: "var(--overlay-faint)",
        borderWidth: 1,
        borderStyle: "solid",
      }}
      role={role}
    >
      {label}
    </div>
  );
}
