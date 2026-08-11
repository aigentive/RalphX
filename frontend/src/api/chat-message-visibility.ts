type ChatMessageVisibilityInput = {
  content?: string | null;
  metadata?: string | null;
  role?: string | null;
};

export function isVisibleChatMessage(message: ChatMessageVisibilityInput) {
  if (message.metadata) {
    try {
      const metadata = JSON.parse(message.metadata) as Record<string, unknown>;
      if (metadata.hidden_from_ui === true) return false;
      if (metadata.recovery_context === true) return false;
    } catch {
      // Malformed metadata should not hide a legitimate transcript row.
    }
  }

  return !(
    message.role === "user" &&
    typeof message.content === "string" &&
    message.content.startsWith("<instructions>")
  );
}
