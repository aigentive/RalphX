export function PersonaArtifactFrontmatter({
  name,
  kind,
  description,
}: {
  name: string;
  kind: string;
  description: string;
}) {
  return (
    <dl
      data-testid="persona-frontmatter"
      className="mb-4 grid gap-3 rounded-md px-3 py-3 sm:grid-cols-[minmax(0,1fr)_auto]"
      style={{
        backgroundColor: "var(--bg-surface)",
        borderColor: "var(--border-subtle)",
        borderStyle: "solid",
        borderWidth: 1,
      }}
    >
      <div className="min-w-0 sm:col-span-2">
        <dt className="text-[0.6875rem] font-medium uppercase tracking-[0.08em] text-[var(--text-muted)]">
          Description
        </dt>
        <dd className="mt-1 text-[0.8125rem] leading-relaxed text-[var(--text-secondary)]">
          {description}
        </dd>
      </div>
      <div className="min-w-0">
        <dt className="text-[0.6875rem] font-medium uppercase tracking-[0.08em] text-[var(--text-muted)]">
          Name
        </dt>
        <dd className="mt-1 truncate font-mono text-[0.75rem] text-[var(--text-primary)]">
          {name}
        </dd>
      </div>
      <div>
        <dt className="text-[0.6875rem] font-medium uppercase tracking-[0.08em] text-[var(--text-muted)]">
          Kind
        </dt>
        <dd className="mt-1 text-[0.75rem] capitalize text-[var(--text-primary)]">
          {kind}
        </dd>
      </div>
    </dl>
  );
}
