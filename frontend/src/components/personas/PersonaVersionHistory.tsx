import { useQuery } from "@tanstack/react-query";

import { artifactApi } from "@/api/artifact";
import { ArtifactLoadingState } from "@/components/agents/AgentsArtifactEmptyState";
import { VersionedArtifactDisplay } from "@/components/Ideation/PlanDisplay";
import { personaArtifactKeys } from "@/hooks/personaArtifactQueries";

import { preparePersonaArtifactContent } from "./personaArtifactContent";

export function PersonaVersionHistory({ artifactId }: { artifactId: string }) {
  const artifactQuery = useQuery({
    queryKey: personaArtifactKeys.detail(artifactId),
    queryFn: () => artifactApi.get(artifactId),
    enabled: Boolean(artifactId),
    staleTime: 30_000,
  });

  if (artifactQuery.isPending) {
    return <ArtifactLoadingState title="Loading persona history..." />;
  }

  if (artifactQuery.error || !artifactQuery.data) {
    return (
      <div
        role="alert"
        className="rounded-md px-3 py-2 text-sm text-[var(--status-error)]"
        style={{
          borderColor: "var(--status-error-border)",
          borderStyle: "solid",
          borderWidth: 1,
        }}
      >
        Persona artifact unavailable
      </div>
    );
  }

  return (
    <VersionedArtifactDisplay
      artifact={artifactQuery.data}
      artifactLabel="Persona"
      excerptSelectionEnabled={false}
      prepareContent={preparePersonaArtifactContent}
      linkedProposalsCount={0}
      chromeless
    />
  );
}
