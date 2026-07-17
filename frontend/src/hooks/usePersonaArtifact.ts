import { useQuery } from "@tanstack/react-query";

import { artifactApi } from "@/api/artifact";
import { PersonaArtifactVersionSummarySchema } from "@/types/artifact";

export const personaArtifactKeys = {
  all: ["persona-artifacts"] as const,
  history: (artifactId: string) =>
    [...personaArtifactKeys.all, "history", artifactId] as const,
  version: (artifactId: string, version: number) =>
    [...personaArtifactKeys.all, "version", artifactId, version] as const,
};

export function usePersonaArtifactHistory(artifactId: string | null) {
  return useQuery({
    queryKey: personaArtifactKeys.history(artifactId ?? ""),
    queryFn: async () =>
      PersonaArtifactVersionSummarySchema.array().parse(
        await artifactApi.getVersionHistory(artifactId!),
      ),
    enabled: Boolean(artifactId),
  });
}

export function usePersonaArtifactVersion(
  artifactId: string | null,
  version: number | null,
) {
  return useQuery({
    queryKey: personaArtifactKeys.version(artifactId ?? "", version ?? 0),
    queryFn: () => artifactApi.getAtVersion(artifactId!, version!),
    enabled: Boolean(artifactId && version != null),
  });
}
