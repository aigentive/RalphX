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
      <div className="mx-auto flex w-full max-w-[1400px] flex-col gap-6 p-6">
        <div className="flex flex-col gap-1">
          <h1
            className="text-[1.375rem] font-semibold"
            style={{
              fontFamily: "system-ui",
              color: "var(--text-primary)",
              letterSpacing: "0",
            }}
          >
            Skills
          </h1>
          <p className="text-[0.8125rem]" style={{ color: "var(--text-secondary)" }}>
            {project.name}
          </p>
        </div>

        <ProjectSkillsCuratorPanel projectId={project.id} />
      </div>
    </div>
  );
}
