export type ChatInputDelivery = "interactive" | "queued" | "unknown";

export function resolveChatInputDelivery(
  harness: string | null | undefined,
): ChatInputDelivery {
  switch (harness) {
    case "claude":
      return "interactive";
    case "codex":
      return "queued";
    default:
      return "unknown";
  }
}
