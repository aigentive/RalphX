import { useEffect, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";

import { fetchPersona, personaKeys } from "@/hooks/usePersonas";
import { personaArtifactKeys } from "@/hooks/personaArtifactQueries";
import { useEventBus } from "@/providers/EventProvider";
import { PersonaDraftUpdatedEventSchema } from "@/types/persona";

/** Refreshes the authoritative draft row after a persona-builder event. */
export function usePersonaDraftEvents(conversationId: string): string | null {
  const bus = useEventBus();
  const queryClient = useQueryClient();
  const [eventBinding, setEventBinding] = useState<{
    conversationId: string;
    draftId: string;
  } | null>(null);

  useEffect(() => {
    return bus.subscribe<unknown>("persona:draft_updated", (payload) => {
      const parsed = PersonaDraftUpdatedEventSchema.safeParse(payload);
      if (!parsed.success) {
        return;
      }

      const nextDraftId = parsed.data.draft_id;
      if (parsed.data.builder_conversation_id === conversationId) {
        setEventBinding({ conversationId, draftId: nextDraftId });
      }
      void queryClient.invalidateQueries({ queryKey: personaKeys.detail(nextDraftId) });
      void queryClient.fetchQuery({
        queryKey: personaKeys.detail(nextDraftId),
        queryFn: () => fetchPersona(nextDraftId),
      }).catch((error: unknown) => {
        console.error(
          "Failed to refresh persona draft after persona:draft_updated",
          error,
        );
      });
      if (parsed.data.artifact_id) {
        void queryClient.invalidateQueries({
          queryKey: personaArtifactKeys.detail(parsed.data.artifact_id),
        });
      }
    });
  }, [bus, conversationId, queryClient]);

  return eventBinding?.conversationId === conversationId
    ? eventBinding.draftId
    : null;
}
