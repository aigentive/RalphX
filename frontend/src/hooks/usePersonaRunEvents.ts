import { useEffect } from "react";
import { useQueryClient } from "@tanstack/react-query";

import { useEventBus } from "@/providers/EventProvider";

import { chatKeys } from "./useChat";

type PersonaRunEvent = {
  conversation_id?: unknown;
};

/** Refreshes the one conversation-level run query after persona delivery settles. */
export function usePersonaRunEvents(conversationId: string | null | undefined) {
  const bus = useEventBus();
  const queryClient = useQueryClient();

  useEffect(() => {
    const invalidateRun = (payload: PersonaRunEvent) => {
      if (
        typeof payload.conversation_id !== "string" ||
        payload.conversation_id !== conversationId
      ) {
        return;
      }
      void queryClient.invalidateQueries({
        queryKey: chatKeys.agentRun(payload.conversation_id),
      });
    };
    const unsubscribeApplied = bus.subscribe<PersonaRunEvent>(
      "persona:applied",
      invalidateRun,
    );
    const unsubscribeSkipped = bus.subscribe<PersonaRunEvent>(
      "persona:injection_skipped",
      invalidateRun,
    );

    return () => {
      unsubscribeApplied();
      unsubscribeSkipped();
    };
  }, [bus, conversationId, queryClient]);
}
