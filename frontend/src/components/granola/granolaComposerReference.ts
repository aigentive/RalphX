import type { ComposerIntegrationReference } from "@/api/chat";
import type { GranolaNoteDetail, GranolaNoteSummary } from "@/api/granola";

export function granolaComposerReference(
  note: GranolaNoteDetail | GranolaNoteSummary,
): ComposerIntegrationReference {
  return {
    provider: "granola",
    kind: "note",
    id: note.id,
    title: note.title ?? note.id,
    ...(note.url ? { url: note.url } : {}),
    ...(note.summary ? { summaryExcerpt: note.summary } : {}),
    includeTranscript: true,
  };
}
