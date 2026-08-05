import {
  useMutation,
  useQuery,
  useQueryClient,
  type QueryClient,
} from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";
import { chatKeys } from "@/hooks/useChat";
import {
  getTransportEnvironmentId,
  isRemoteEnvironmentId,
} from "@/lib/remote/active-environment";
import { personaArtifactKeys } from "@/hooks/personaArtifactQueries";
import {
  PersonaOverlayPreviewSchema,
  PersonaResponseSchema,
  PersonaUsageSchema,
  transformPersona,
  type Persona,
  type PersonaOverlayPreview,
  type PersonaUsage,
} from "@/types/persona";

export const personaKeys = {
  all: ["personas"] as const,
  list: (scope?: PersonaScope) =>
    scope
      ? ([...personaKeys.all, "list", scope] as const)
      : ([...personaKeys.all, "list"] as const),
  detail: (id: string) => [...personaKeys.all, "detail", id] as const,
  usage: () => [...personaKeys.all, "usage"] as const,
  overlayPreview: (conversationId: string) =>
    [...personaKeys.all, "overlayPreview", conversationId] as const,
};

export type CreatePersonaDraftInput = {
  slug: string;
  projectId?: string | null;
  content?: string;
  description?: string;
  body?: string;
  sourceSessionId?: string;
};
export type PersonaScope =
  | { type: "all" }
  | { type: "globalOnly" }
  | { type: "globalAndProject"; projectId: string };

export type UpdatePersonaInput = {
  id: string;
  content?: string;
  description?: string;
  body?: string;
};
export type UpdatePersonaDraftInput = {
  id: string;
  content: string;
  expectedContentHash: string;
};
export type ApprovePersonaAsNewInput = {
  id: string;
  newSlug?: string;
};
export type SwitchConversationPersonaInput = {
  conversationId: string;
  personaId: string | null;
};

function parsePersona(raw: unknown): Persona {
  return transformPersona(PersonaResponseSchema.parse(raw));
}

export async function fetchPersonas(scope?: PersonaScope): Promise<Persona[]> {
  const raw = await invoke<unknown>("list_personas", {
    input: { ...(scope !== undefined && { scope }) },
  });
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
      ...(input.projectId !== undefined && { projectId: input.projectId }),
      ...(input.content !== undefined && { content: input.content }),
      ...(input.description !== undefined && { description: input.description }),
      ...(input.body !== undefined && { body: input.body }),
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

export async function updatePersonaDraft(
  input: UpdatePersonaDraftInput,
): Promise<Persona> {
  const raw = await invoke<unknown>("update_persona_draft", { input });
  return parsePersona(raw);
}

export async function approvePersona(id: string): Promise<Persona> {
  const raw = await invoke<unknown>("approve_persona", { input: { id } });
  return parsePersona(raw);
}

export async function approvePersonaAsNew(
  input: ApprovePersonaAsNewInput,
): Promise<Persona> {
  const raw = await invoke<unknown>("approve_persona_as_new", {
    input: {
      id: input.id,
      ...(input.newSlug !== undefined && { newSlug: input.newSlug }),
    },
  });
  return parsePersona(raw);
}

export async function archivePersona(id: string): Promise<Persona> {
  const raw = await invoke<unknown>("archive_persona", { input: { id } });
  return parsePersona(raw);
}

export async function unarchivePersona(id: string): Promise<Persona> {
  const raw = await invoke<unknown>("unarchive_persona", { input: { id } });
  return parsePersona(raw);
}

export async function reseedPersonaDraft(id: string): Promise<Persona> {
  const raw = await invoke<unknown>("reseed_persona_draft", { input: { id } });
  return parsePersona(raw);
}

export async function fetchPersonaUsage(): Promise<PersonaUsage[]> {
  const raw = await invoke<unknown>("list_persona_usage");
  return z.array(PersonaUsageSchema).parse(raw);
}

export async function fetchPersonaOverlayPreview(
  conversationId: string,
): Promise<PersonaOverlayPreview | null> {
  const raw = await invoke<unknown>("preview_persona_overlay", {
    input: { conversationId },
  });
  return raw === null ? null : PersonaOverlayPreviewSchema.parse(raw);
}

export async function deletePersonaDraft(id: string): Promise<void> {
  await invoke<void>("delete_persona_draft", { input: { id } });
}

export async function switchConversationPersona(
  input: SwitchConversationPersonaInput,
): Promise<void> {
  if (isRemoteEnvironmentId(getTransportEnvironmentId())) {
    await invoke<unknown>("switch_remote_agent_conversation_persona", { input });
    return;
  }
  await invoke<unknown>("switch_agent_conversation_persona", { input });
}

export function usePersonas(scope?: PersonaScope) {
  return useQuery<Persona[], Error>({
    queryKey: personaKeys.list(scope),
    queryFn: () => fetchPersonas(scope),
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

function publishPersonaUpdate(
  queryClient: QueryClient,
  persona: Persona,
) {
  queryClient.setQueryData(personaKeys.detail(persona.id), persona);
  if (!persona.artifactId) return;
  void queryClient.invalidateQueries({
    queryKey: personaArtifactKeys.detail(persona.artifactId),
  });
}

export function useCreatePersonaDraft() {
  const invalidateList = usePersonaListInvalidation();
  return useMutation<Persona, Error, CreatePersonaDraftInput>({
    mutationFn: createPersonaDraft,
    onSuccess: invalidateList,
  });
}

export function useUpdatePersona() {
  const queryClient = useQueryClient();
  const invalidateList = usePersonaListInvalidation();
  return useMutation<Persona, Error, UpdatePersonaInput>({
    mutationFn: updatePersona,
    onSuccess: (persona) => {
      invalidateList();
      publishPersonaUpdate(queryClient, persona);
    },
  });
}

export function useUpdatePersonaDraft() {
  const queryClient = useQueryClient();
  const invalidateList = usePersonaListInvalidation();
  return useMutation<Persona, Error, UpdatePersonaDraftInput>({
    mutationFn: updatePersonaDraft,
    onSuccess: (persona) => {
      invalidateList();
      publishPersonaUpdate(queryClient, persona);
    },
  });
}

export function useApprovePersona() {
  const queryClient = useQueryClient();
  const invalidateList = usePersonaListInvalidation();
  return useMutation<Persona, Error, string>({
    mutationFn: approvePersona,
    onSuccess: (persona) => {
      publishPersonaUpdate(queryClient, persona);
      invalidateList();
      void queryClient.invalidateQueries({ queryKey: chatKeys.conversations() });
    },
  });
}

export function useApprovePersonaAsNew() {
  const queryClient = useQueryClient();
  const invalidateList = usePersonaListInvalidation();
  return useMutation<Persona, Error, ApprovePersonaAsNewInput>({
    mutationFn: approvePersonaAsNew,
    onSuccess: (persona) => {
      publishPersonaUpdate(queryClient, persona);
      invalidateList();
      void queryClient.invalidateQueries({ queryKey: chatKeys.conversations() });
    },
  });
}

export function useArchivePersona() {
  const queryClient = useQueryClient();
  const invalidateList = usePersonaListInvalidation();
  return useMutation<Persona, Error, string>({
    mutationFn: archivePersona,
    onSuccess: () => {
      invalidateList();
      void queryClient.invalidateQueries({ queryKey: personaKeys.usage() });
    },
  });
}

export function useUnarchivePersona() {
  const queryClient = useQueryClient();
  const invalidateList = usePersonaListInvalidation();
  return useMutation<Persona, Error, string>({
    mutationFn: unarchivePersona,
    onSuccess: () => {
      invalidateList();
      void queryClient.invalidateQueries({ queryKey: personaKeys.usage() });
    },
  });
}

export function useReseedPersonaDraft() {
  const queryClient = useQueryClient();
  const invalidateList = usePersonaListInvalidation();
  return useMutation<Persona, Error, string>({
    mutationFn: reseedPersonaDraft,
    onSuccess: (persona) => {
      invalidateList();
      publishPersonaUpdate(queryClient, persona);
    },
  });
}

/** Derived usage for settings rows; errors surface as query errors (em-dash UI), never zeros. */
export function usePersonaUsage(enabled = true) {
  return useQuery<PersonaUsage[], Error>({
    queryKey: personaKeys.usage(),
    queryFn: fetchPersonaUsage,
    enabled,
  });
}

/** Fetch-on-open preview of the exact overlay the next send would inject. */
export function usePersonaOverlayPreview(conversationId: string, enabled: boolean) {
  return useQuery<PersonaOverlayPreview | null, Error>({
    queryKey: personaKeys.overlayPreview(conversationId),
    queryFn: () => fetchPersonaOverlayPreview(conversationId),
    enabled: enabled && Boolean(conversationId),
    staleTime: 30_000,
  });
}

export function useDeletePersonaDraft() {
  const invalidateList = usePersonaListInvalidation();
  return useMutation<void, Error, string>({
    mutationFn: deletePersonaDraft,
    onSuccess: invalidateList,
  });
}

export function useSwitchConversationPersona() {
  const queryClient = useQueryClient();
  return useMutation<void, Error, SwitchConversationPersonaInput>({
    mutationFn: switchConversationPersona,
    onSuccess: (_result, input) => {
      void queryClient.invalidateQueries({ queryKey: personaKeys.list() });
      void queryClient.invalidateQueries({ queryKey: chatKeys.conversations() });
      void queryClient.invalidateQueries({ queryKey: personaKeys.usage() });
      void queryClient.invalidateQueries({
        queryKey: personaKeys.overlayPreview(input.conversationId),
      });
    },
  });
}
