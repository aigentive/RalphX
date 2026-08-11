import { lazy, Suspense, useEffect, useState } from "react";

const LazyTeamPanelContent = lazy(() =>
  import("./team/TeamPanelContent").then((module) => ({ default: module.TeamPanelContent })),
);

export function AgentsTeamPanel({
  conversationId,
  projectId,
  activeAgentRunId,
}: {
  conversationId: string;
  projectId: string | null;
  activeAgentRunId: string | null;
}) {
  const [hydrated, setHydrated] = useState(false);

  useEffect(() => {
    const frame = window.requestAnimationFrame(() => {
      window.setTimeout(() => setHydrated(true), 0);
    });
    return () => window.cancelAnimationFrame(frame);
  }, []);

  return (
    <section
      className="h-full min-h-0 overflow-y-auto"
      style={{ backgroundColor: "var(--bg-surface)" }}
      data-testid="agents-team-panel"
      data-hydrated={hydrated ? "true" : "false"}
    >
      {!hydrated ? (
        <div className="p-4" data-testid="agents-team-panel-shell">
          <div className="h-4 w-20 rounded" style={{ backgroundColor: "var(--overlay-faint)" }} />
          <div className="mt-3 h-20 rounded-lg border" style={{ backgroundColor: "var(--bg-base)", borderColor: "var(--border-subtle)", borderStyle: "solid", borderWidth: 1 }} />
        </div>
      ) : (
        <Suspense fallback={<div className="p-4 text-sm" style={{ color: "var(--text-muted)" }}>Loading Team…</div>}>
          <LazyTeamPanelContent conversationId={conversationId} projectId={projectId} activeAgentRunId={activeAgentRunId} />
        </Suspense>
      )}
    </section>
  );
}
