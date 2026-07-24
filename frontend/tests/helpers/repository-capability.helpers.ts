import type { Page } from "@playwright/test";

import type { RepositoryCapability } from "@/types/project";

const DEMO_PROJECT_ID = "project-mock-1";

export async function setMockProjectRepositoryCapability(
  page: Page,
  repositoryCapability: RepositoryCapability,
  githubPrEnabled: boolean,
) {
  await page.waitForFunction((projectId) => {
    const projects = window.__queryClient?.getQueryData(["projects", "list"]);
    return Array.isArray(projects) && projects.some((project) => project.id === projectId);
  }, DEMO_PROJECT_ID);

  await page.evaluate(
    async ({ projectId, nextCapability, nextGithubPrEnabled }) => {
      const { getStore } = await import("/src/api-mock/store");
      const { useProjectStore } = await import("/src/stores/projectStore");
      const store = getStore();
      const project = store.projects.get(projectId);
      if (!project) {
        throw new Error(`Mock project ${projectId} is unavailable`);
      }

      const changes = {
        githubPrEnabled: nextGithubPrEnabled,
        repositoryCapability: nextCapability,
      };
      store.projects.set(projectId, { ...project, ...changes });
      useProjectStore.getState().updateProject(projectId, changes);
      window.__queryClient?.setQueryData(
        ["projects", "list"],
        (projects: (typeof project)[] | undefined) =>
          projects?.map((existingProject) =>
            existingProject.id === projectId
              ? { ...existingProject, ...changes }
              : existingProject,
          ),
      );
    },
    {
      projectId: DEMO_PROJECT_ID,
      nextCapability: repositoryCapability,
      nextGithubPrEnabled: githubPrEnabled,
    },
  );
}
