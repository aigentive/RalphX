import { Drama } from "lucide-react";

interface PersonaBuildBannerProps {
  projectName: string | null;
  sourcePersonaName: string | null;
}

export function PersonaBuildBanner({
  projectName,
  sourcePersonaName,
}: PersonaBuildBannerProps) {
  const title = sourcePersonaName
    ? `Refining '${sourcePersonaName}'`
    : projectName
      ? `Building a persona for ${projectName}`
      : "Building a Global persona · private workspace";
  const caption = projectName
    ? "Describe the persona, and attach extra context if useful."
    : "Attach files/folders below, or just describe the persona.";

  return (
    <div
      data-testid="persona-build-banner"
      className="mx-auto mb-3 flex max-w-[620px] items-start gap-3 rounded-lg border border-[var(--accent-border)] bg-[var(--accent-muted)] px-3 py-2.5 text-left"
    >
      <Drama className="mt-0.5 h-4 w-4 shrink-0 text-[var(--accent-primary)]" aria-hidden="true" />
      <div>
        <p className="text-[13px] font-medium text-[var(--text-primary)]">{title}</p>
        <p className="mt-0.5 text-xs text-[var(--text-secondary)]">{caption}</p>
      </div>
    </div>
  );
}
