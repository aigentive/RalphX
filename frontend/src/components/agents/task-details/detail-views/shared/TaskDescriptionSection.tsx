/**
 * TaskDescriptionSection - Reusable description block with a section heading.
 *
 * Used by registry-driven task detail views to surface task.description in a
 * consistent location, typically above the per-state body sections (commits,
 * progress, etc.). Skips render entirely when there's no description so views
 * can drop it in unconditionally.
 */
import { DescriptionBlock } from "./DescriptionBlock";
import { SectionTitle } from "./SectionTitle";

interface TaskDescriptionSectionProps {
  description: string | null | undefined;
  heading?: string;
}

export function TaskDescriptionSection({
  description,
  heading = "Description",
}: TaskDescriptionSectionProps) {
  if (!description) return null;

  return (
    <section data-testid="task-description-section">
      <SectionTitle muted>{heading}</SectionTitle>
      <DescriptionBlock description={description} />
    </section>
  );
}
