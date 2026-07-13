import { useEffect, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";

import { fetchPersona, personaKeys } from "@/hooks/usePersonas";
import { useEventBus } from "@/providers/EventProvider";
import { PersonaDraftUpdatedEventSchema } from "@/types/persona";

/** Refreshes the authoritative draft row after a persona-builder event. */
export function usePersonaDraftEvents(): string | null {
  const bus = useEventBus();
  const queryClient = useQueryClient();
  const [draftId, setDraftId] = useState<string | null>(null);

  useEffect(() => {
    return bus.subscribe<unknown>("persona:draft_updated", (payload) => {
      const parsed = PersonaDraftUpdatedEventSchema.safeParse(payload);
      if (!parsed.success) {
        return;
      }

      const nextDraftId = parsed.data.draft_id;
      setDraftId(nextDraftId);
      void queryClient.invalidateQueries({ queryKey: personaKeys.detail(nextDraftId) });
      void queryClient.fetchQuery({
        queryKey: personaKeys.detail(nextDraftId),
        queryFn: () => fetchPersona(nextDraftId),
      });
    });
  }, [bus, queryClient]);

  return draftId;
}
