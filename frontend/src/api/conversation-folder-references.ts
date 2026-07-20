import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";

export const conversationFolderReferenceSchema = z.object({
  id: z.string(),
  conversationId: z.string(),
  folderPath: z.string(),
  displayName: z.string(),
  createdAt: z.string(),
});

export type ConversationFolderReference = z.infer<
  typeof conversationFolderReferenceSchema
>;

export const conversationFolderReferencesApi = {
  list: async (conversationId: string): Promise<ConversationFolderReference[]> =>
    z.array(conversationFolderReferenceSchema).parse(
      await invoke("list_conversation_folder_references", { conversationId }),
    ),
  add: async (input: {
    conversationId: string;
    folderPath: string;
    displayName: string;
  }): Promise<ConversationFolderReference> =>
    conversationFolderReferenceSchema.parse(
      await invoke("add_conversation_folder_reference", { input }),
    ),
  remove: async (input: {
    conversationId: string;
    folderReferenceId: string;
  }): Promise<void> => {
    await invoke("remove_conversation_folder_reference", { input });
  },
};
