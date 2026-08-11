export const personaArtifactKeys = {
  all: ["persona-artifact"] as const,
  detail: (artifactId: string) =>
    [...personaArtifactKeys.all, artifactId] as const,
};
