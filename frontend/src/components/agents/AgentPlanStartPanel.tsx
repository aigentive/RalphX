import { FileText, Search, Upload } from "lucide-react";

type AgentPlanStartPanelStatus = "idle" | "loading" | "error" | "pending";

interface AgentPlanStartPanelProps {
  status?: AgentPlanStartPanelStatus;
  errorMessage?: string | null;
}

const STATUS_COPY: Record<
  AgentPlanStartPanelStatus,
  { label: string; detail: string }
> = {
  idle: {
    label: "No plan selected",
    detail: "Search project plans or import markdown to create a draft plan.",
  },
  loading: {
    label: "Loading plans...",
    detail: "Checking for an existing plan before showing draft options.",
  },
  error: {
    label: "Plan setup unavailable",
    detail: "The plan surface is available, but plan setup could not finish.",
  },
  pending: {
    label: "Preparing draft plan...",
    detail: "Plan setup is still settling for this conversation.",
  },
};

const surfaceStyle = {
  backgroundColor: "var(--bg-surface)",
  borderColor: "var(--border-subtle)",
  borderWidth: 1,
  borderStyle: "solid",
} as const;

const elevatedSurfaceStyle = {
  backgroundColor: "var(--bg-elevated)",
  borderColor: "var(--overlay-faint)",
  borderWidth: 1,
  borderStyle: "solid",
} as const;

export function AgentPlanStartPanel({
  status = "idle",
  errorMessage = null,
}: AgentPlanStartPanelProps) {
  const statusCopy = STATUS_COPY[status];
  const detail =
    status === "error" && errorMessage?.trim()
      ? errorMessage.trim()
      : statusCopy.detail;

  return (
    <div
      className="min-h-full px-4 py-4"
      data-testid="agent-plan-start-panel"
    >
      <div className="mx-auto flex max-w-3xl flex-col gap-4">
        <section
          className="rounded-lg px-4 py-4"
          style={surfaceStyle}
          aria-labelledby="agent-plan-start-heading"
        >
          <div className="flex items-start gap-3">
            <div
              className="flex h-9 w-9 shrink-0 items-center justify-center rounded-md"
              style={{
                backgroundColor: "var(--accent-muted)",
                color: "var(--accent-primary)",
              }}
            >
              <FileText className="h-4 w-4" aria-hidden="true" />
            </div>
            <div className="min-w-0">
              <h2
                id="agent-plan-start-heading"
                className="text-sm font-semibold"
                style={{ color: "var(--text-primary)" }}
              >
                Start a Plan
              </h2>
              <p
                className="mt-1 max-w-2xl text-sm leading-5"
                style={{ color: "var(--text-secondary)" }}
              >
                Create a draft from an existing project plan or a markdown file.
              </p>
            </div>
          </div>
        </section>

        <div className="grid gap-4 lg:grid-cols-[minmax(0,1fr)_minmax(220px,0.72fr)]">
          <section className="rounded-lg p-4" style={surfaceStyle}>
            <label
              htmlFor="agent-plan-start-search"
              className="text-xs font-medium uppercase tracking-[0.08em]"
              style={{ color: "var(--text-muted)" }}
            >
              Project plans
            </label>
            <div
              className="mt-2 flex h-10 items-center gap-2 rounded-md px-3"
              style={elevatedSurfaceStyle}
            >
              <Search
                className="h-4 w-4 shrink-0"
                style={{ color: "var(--text-muted)" }}
                aria-hidden="true"
              />
              <input
                id="agent-plan-start-search"
                type="search"
                aria-label="Search project plans"
                disabled
                className="min-w-0 flex-1 bg-transparent text-sm outline-none"
                placeholder="Search existing plans"
                style={{ color: "var(--text-primary)" }}
              />
            </div>
            <div
              className="mt-3 rounded-md px-3 py-3 text-sm"
              style={{
                backgroundColor: "var(--bg-elevated)",
                color: "var(--text-secondary)",
                borderColor: "var(--overlay-faint)",
                borderWidth: 1,
                borderStyle: "solid",
              }}
              role={status === "error" ? "alert" : "status"}
              aria-live="polite"
              data-testid={`agent-plan-start-status-${status}`}
            >
              <div className="font-medium" style={{ color: "var(--text-primary)" }}>
                {statusCopy.label}
              </div>
              <div className="mt-1 leading-5">{detail}</div>
            </div>
          </section>

          <section className="rounded-lg p-4" style={surfaceStyle}>
            <div className="flex items-center gap-2">
              <Upload
                className="h-4 w-4"
                style={{ color: "var(--accent-primary)" }}
                aria-hidden="true"
              />
              <h3
                className="text-sm font-semibold"
                style={{ color: "var(--text-primary)" }}
              >
                Import markdown
              </h3>
            </div>
            <div
              className="mt-3 rounded-md px-3 py-5 text-center text-sm"
              style={{
                backgroundColor: "var(--bg-elevated)",
                color: "var(--text-secondary)",
                borderColor: "var(--overlay-faint)",
                borderWidth: 1,
                borderStyle: "dashed",
              }}
            >
              Markdown drop area
            </div>
          </section>
        </div>
      </div>
    </div>
  );
}
