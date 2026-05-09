import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";

const ReleaseNotesResponseSchema = z.object({
  version: z.string(),
  body: z.string().nullable(),
  source: z.enum(["bundled_resource", "development_checkout", "missing"]),
});

export type ReleaseNotesResponse = z.infer<typeof ReleaseNotesResponseSchema>;

export async function getCurrentReleaseNotes(): Promise<ReleaseNotesResponse> {
  const response = await invoke<unknown>("get_current_release_notes");
  return ReleaseNotesResponseSchema.parse(response);
}

export async function getLastSeenReleaseNotesVersion(): Promise<string | null> {
  return invoke<string | null>("get_last_seen_release_notes_version");
}

export async function markReleaseNotesSeen(version: string): Promise<void> {
  await invoke("mark_release_notes_seen", { version });
}
