export const githubBranchOverviewKeys = {
  all: ["github", "branch-overview"] as const,
  project: (projectId: string) => [...githubBranchOverviewKeys.all, projectId] as const,
};
