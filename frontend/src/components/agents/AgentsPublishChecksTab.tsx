import type { PullRequestDetail } from "@/api/github";
import { CheckRow } from "@/components/pr/PullRequestDetailSections";

export function AgentsPublishChecksTab({
  detail,
  isError,
  isLoading,
  isReady,
}: {
  detail: PullRequestDetail | null;
  isError: boolean;
  isLoading: boolean;
  isReady: boolean;
}) {
  const checksUnavailable =
    isError ||
    detail?.state !== "loaded" ||
    detail.sourcesUnavailable.includes("checks");
  const checks = detail?.checks ?? [];

  return (
    <section
      className="rounded-lg p-4"
      data-testid="agents-publish-checks-shell"
      style={{
        backgroundColor: "var(--bg-subtle)",
        borderColor: "var(--border-subtle)",
        borderStyle: "solid",
        borderWidth: "1px",
      }}
    >
      <div className="space-y-1">
        <h3 className="text-sm font-semibold text-[var(--text-primary)]">
          Pull request checks
        </h3>
        <p className="text-xs text-[var(--text-muted)]">
          Read-only status from GitHub.
        </p>
      </div>
      <div className="mt-4">
        {!isReady ? null : isLoading && !detail ? (
          <p className="text-sm text-[var(--text-secondary)]">Loading checks…</p>
        ) : checksUnavailable ? (
          <p className="text-sm text-[var(--status-error)]">
            Checks are unavailable right now.
          </p>
        ) : checks.length === 0 ? (
          <p className="text-sm text-[var(--text-secondary)]">
            No checks reported for this PR yet.
          </p>
        ) : (
          <div className="space-y-2">
            {checks.map((check) => (
              <CheckRow
                key={`${check.name}:${check.detailsUrl ?? ""}`}
                check={check}
              />
            ))}
          </div>
        )}
      </div>
    </section>
  );
}
