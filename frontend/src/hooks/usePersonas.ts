import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";
import { chatKeys } from "@/hooks/useChat";
import {
  PersonaIngestManifestSchema,
  PersonaResponseSchema,
  transformPersona,
  type Persona,
  type PersonaIngestManifest,
} from "@/types/persona";

export const personaKeys = {
  all: ["personas"] as const,
  list: () => [...personaKeys.all, "list"] as const,
  detail: (id: string) => [...personaKeys.all, "detail", id] as const,
  ingestManifest: (conversationId: string) =>
    [...personaKeys.all, "ingest-manifest", conversationId] as const,
};

export type CreatePersonaDraftInput = {
  slug: string;
  content: string;
  sourceSessionId?: string;
};

export type UpdatePersonaInput = { id: string; content: string };
export type IngestPersonaContextInput = {
  conversationId: string;
  pickedPath: string;
};
export type SwitchConversationPersonaInput = {
  conversationId: string;
  personaId: string | null;
};

function parsePersona(raw: unknown): Persona {
  return transformPersona(PersonaResponseSchema.parse(raw));
}

export async function fetchPersonas(): Promise<Persona[]> {
  const raw = await invoke<unknown>("list_personas", { input: {} });
  return z.array(PersonaResponseSchema).parse(raw).map(transformPersona);
}

export async function fetchPersona(id: string): Promise<Persona> {
  const raw = await invoke<unknown>("get_persona", { input: { id } });
  return parsePersona(raw);
}

export async function createPersonaDraft(
  input: CreatePersonaDraftInput,
): Promise<Persona> {
  const raw = await invoke<unknown>("create_persona_draft", {
    input: {
      slug: input.slug,
      content: input.content,
      ...(input.sourceSessionId !== undefined && {
        sourceSessionId: input.sourceSessionId,
      }),
    },
  });
  return parsePersona(raw);
}

export async function updatePersona(input: UpdatePersonaInput): Promise<Persona> {
  const raw = await invoke<unknown>("update_persona", { input });
  return parsePersona(raw);
}

export async function approvePersona(id: string): Promise<Persona> {
  const raw = await invoke<unknown>("approve_persona", { input: { id } });
  return parsePersona(raw);
}

export async function archivePersona(id: string): Promise<Persona> {
  const raw = await invoke<unknown>("archive_persona", { input: { id } });
  return parsePersona(raw);
}

export async function deletePersonaDraft(id: string): Promise<void> {
  await invoke<void>("delete_persona_draft", { input: { id } });
}

export async function ingestPersonaContext(
  input: IngestPersonaContextInput,
): Promise<PersonaIngestManifest> {
  const raw = await invoke<unknown>("ingest_persona_context", { input });
  return PersonaIngestManifestSchema.parse(raw);
}

export async function switchConversationPersona(
  input: SwitchConversationPersonaInput,
): Promise<void> {
  await invoke<unknown>("switch_agent_conversation_persona", { input });
}

export function usePersonas() {
  return useQuery<Persona[], Error>({
    queryKey: personaKeys.list(),
    queryFn: fetchPersonas,
  });
}

export function usePersona(id: string) {
  return useQuery<Persona, Error>({
    queryKey: personaKeys.detail(id),
    queryFn: () => fetchPersona(id),
    enabled: Boolean(id),
  });
}

function usePersonaListInvalidation() {
  const queryClient = useQueryClient();
  return () => {
    void queryClient.invalidateQueries({ queryKey: personaKeys.list() });
  };
}

export function useCreatePersonaDraft() {
  const invalidateList = usePersonaListInvalidation();
  return useMutation<Persona, Error, CreatePersonaDraftInput>({
    mutationFn: createPersonaDraft,
    onSuccess: invalidateList,
  });
}

export function useUpdatePersona() {
  const invalidateList = usePersonaListInvalidation();
  return useMutation<Persona, Error, UpdatePersonaInput>({
    mutationFn: updatePersona,
    onSuccess: invalidateList,
  });
}

export function useApprovePersona() {
  const invalidateList = usePersonaListInvalidation();
  return useMutation<Persona, Error, string>({
    mutationFn: approvePersona,
    onSuccess: invalidateList,
  });
}

export function useArchivePersona() {
  const invalidateList = usePersonaListInvalidation();
  return useMutation<Persona, Error, string>({
    mutationFn: archivePersona,
    onSuccess: invalidateList,
  });
}

export function useDeletePersonaDraft() {
  const invalidateList = usePersonaListInvalidation();
  return useMutation<void, Error, string>({
    mutationFn: deletePersonaDraft,
    onSuccess: invalidateList,
  });
}

export function useIngestPersonaContext() {
  const queryClient = useQueryClient();
  return useMutation<PersonaIngestManifest, Error, IngestPersonaContextInput>({
    mutationFn: ingestPersonaContext,
    onSuccess: (manifest, input) => {
      queryClient.setQueryData(personaKeys.ingestManifest(input.conversationId), manifest);
    },
  });
}

export function useSwitchConversationPersona() {
  const queryClient = useQueryClient();
  return useMutation<void, Error, SwitchConversationPersonaInput>({
    mutationFn: switchConversationPersona,
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: personaKeys.list() });
      void queryClient.invalidateQueries({ queryKey: chatKeys.conversations() });
    },
  });
}
