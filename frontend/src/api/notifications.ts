import { typedInvoke } from "@/lib/tauri";
import { AttentionItemListSchema, type AttentionItem } from "@/types/notifications";

export const notificationsApi = {
  listAttentionItems: (projectId?: string): Promise<AttentionItem[]> =>
    typedInvoke(
      "list_attention_items",
      projectId ? { projectId } : {},
      AttentionItemListSchema,
    ),
} as const;
