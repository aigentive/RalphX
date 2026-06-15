import { ProjectSkillsCuratorPanel } from "@/components/project-skills/ProjectSkillsCuratorPanel";
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
      <div className="mx-auto flex w-full max-w-[1440px] flex-col gap-5 p-6">
        <div className="flex flex-wrap items-end justify-between gap-4 border-b border-[var(--border-subtle)] pb-5">
          <div className="min-w-0">
            <div className="text-xs font-medium uppercase text-[var(--text-tertiary)]">
              {project.name}
            </div>
            <h1
              className="mt-1 text-[1.375rem] font-semibold"
              style={{
                fontFamily: "system-ui",
                color: "var(--text-primary)",
                letterSpacing: "0",
              }}
            >
              Project Skills
            </h1>
            <p className="mt-2 max-w-[52rem] text-[0.8125rem] leading-5 text-[var(--text-secondary)]">
              Review learned procedures, approve what agents may reuse, and
              explicitly export selected skills to the target repository.
            </p>
          </div>
        </div>

        <ProjectSkillsCuratorPanel projectId={project.id} />
      </div>
    </div>
  );
}
