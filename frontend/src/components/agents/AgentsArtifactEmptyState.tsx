export function EmptyArtifactState({
  title,
  detail,
  testId,
}: {
  title: string;
  detail?: string | undefined;
  testId?: string | undefined;
}) {
  return (
    <div
      className="h-full min-h-[220px] flex items-center justify-center p-6 text-center"
      data-testid={testId}
    >
      <div className="max-w-sm">
        <div className="text-sm font-medium text-[var(--text-primary)]">{title}</div>
        {detail && (
          <div className="mt-2 text-xs leading-relaxed text-[var(--text-muted)]">{detail}</div>
        )}
      </div>
    </div>
  );
}

export function ArtifactLoadingState({
  title,
  detail,
}: {
  title: string;
  detail?: string | undefined;
}) {
  return (
    <div
      className="h-full min-h-[220px] flex items-center justify-center p-6 text-center"
      role="status"
      aria-label={title}
      data-testid="agents-artifact-loading-state"
    >
      <div className="w-full max-w-sm">
        <div className="text-sm font-medium text-[var(--text-primary)]">{title}</div>
        {detail && (
          <div className="mt-2 text-xs leading-relaxed text-[var(--text-muted)]">{detail}</div>
        )}
        <div className="mx-auto mt-5 max-w-[240px] space-y-2">
          {["100%", "82%", "62%"].map((width) => (
            <div
              key={width}
              data-testid="agents-artifact-loading-line"
              className="h-3 animate-pulse rounded"
              style={{
                backgroundColor: "var(--bg-hover)",
                width,
              }}
            />
          ))}
        </div>
      </div>
    </div>
  );
}
