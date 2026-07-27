import { ProjectSkillsCuratorPanel } from "@/components/project-skills/ProjectSkillsCuratorPanel";
import { ProjectSelector } from "@/components/projects/ProjectSelector";
import { selectActiveProject, useProjectStore } from "@/stores/projectStore";

export function SkillsView() {
  const project = useProjectStore(selectActiveProject);

  if (!project) {
    return (
      <div
        className="flex flex-1 items-center justify-center"
        style={{ color: "var(--text-muted)" }}
      >
        <p className="text-[0.875rem]">Select a project to view skills</p>
      </div>
    );
  }

  return (
    <div
      data-testid="skills-view"
      className="flex flex-1 flex-col overflow-auto"
      style={{ backgroundColor: "var(--bg-base)" }}
    >
      <div className="mx-auto flex w-full max-w-[1440px] min-w-0 flex-col gap-5 px-4 py-5 sm:p-6">
        <div className="flex flex-wrap items-start justify-between gap-4 border-b border-[var(--border-subtle)] pb-5 sm:items-end">
          <div className="min-w-0">
            <div className="text-[0.6875rem] font-semibold uppercase tracking-[0.08em] text-[var(--text-muted)]">
              Project-scoped skills
            </div>
            <h1
              className="mt-1 text-[1.375rem] font-semibold"
              style={{
                fontFamily: "system-ui",
                color: "var(--text-primary)",
                letterSpacing: "-0.01em",
              }}
            >
              Project Skills
            </h1>
            <p className="mt-2 max-w-[44rem] text-[0.8125rem] leading-5 text-[var(--text-secondary)]">
              Review what agents learned, approve what they can reuse, and export
              selected skills to the repo.
            </p>
          </div>
          <div className="grid w-full gap-1 sm:w-auto">
            <span className="text-[0.6875rem] font-semibold uppercase tracking-[0.08em] text-[var(--text-muted)]">
              Project
            </span>
            <ProjectSelector
              onNewProject={() => undefined}
              align="end"
              className="w-full max-w-[320px]"
            />
          </div>
        </div>

        <ProjectSkillsCuratorPanel projectId={project.id} />
      </div>
    </div>
  );
}
