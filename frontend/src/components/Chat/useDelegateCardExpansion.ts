import { useCallback, useContext, useState } from "react";
import { useChatStore } from "@/stores/chatStore";
import { ToolCallStoreKeyContext } from "./tool-widgets/ToolCallStoreKeyContext";

export function useDelegateCardExpansion(delegateKey: string, enabled: boolean) {
  const [localExpanded, setLocalExpanded] = useState(false);
  const storeKey = useContext(ToolCallStoreKeyContext);
  const sharedEnabled = enabled && storeKey != null;
  const conversationId = useChatStore((state) => (
    storeKey ? state.activeConversationIds[storeKey] : null
  )) ?? storeKey ?? "delegate-card:unscoped";
  const sharedExpanded = useChatStore(
    (state) => state.delegateExpansionByConversation[conversationId]?.[delegateKey] === true,
  );
  const setDelegateExpanded = useChatStore((state) => state.setDelegateExpanded);
  const setIsExpanded = useCallback(
    (expanded: boolean) => {
      if (sharedEnabled) {
        setDelegateExpanded(conversationId, delegateKey, expanded);
      } else {
        setLocalExpanded(expanded);
      }
    },
    [conversationId, delegateKey, setDelegateExpanded, sharedEnabled],
  );

  return { isExpanded: sharedEnabled ? sharedExpanded : localExpanded, setIsExpanded };
}
