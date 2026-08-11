import { z } from "zod";

export const PersonaStatusSchema = z.enum(["draft", "active", "archived"]);

/** Frontend persona model after the snake_case IPC response is transformed. */
export const PersonaSchema = z.object({
  id: z.string().min(1),
  artifactId: z.string().min(1).nullable().default(null),
  slug: z.string().min(1),
  name: z.string().min(1),
  description: z.string(),
  content: z.string(),
  status: PersonaStatusSchema,
  version: z.number().int(),
  projectId: z.string().nullable(),
  contentHash: z.string().min(1),
  sourceSessionId: z.string().nullable().optional(),
  sourcePersonaId: z.string().nullable().optional(),
  sourceContentHash: z.string().nullable().optional(),
  createdAt: z.string().datetime({ offset: true }),
  updatedAt: z.string().datetime({ offset: true }),
});

export type Persona = z.infer<typeof PersonaSchema>;

/** Raw IPC response. `Persona` has no Rust `rename_all`, so it serializes snake_case. */
export const PersonaResponseSchema = z.object({
  id: z.string().min(1),
  artifact_id: z.string().min(1).nullable().optional(),
  slug: z.string().min(1),
  name: z.string().min(1),
  description: z.string(),
  content: z.string(),
  status: PersonaStatusSchema,
  version: z.number().int(),
  project_id: z.string().nullable(),
  content_hash: z.string().min(1),
  source_session_id: z.string().nullable().optional(),
  source_persona_id: z.string().nullable().optional(),
  source_content_hash: z.string().nullable().optional(),
  created_at: z.string().datetime({ offset: true }),
  updated_at: z.string().datetime({ offset: true }),
});

export type PersonaResponse = z.infer<typeof PersonaResponseSchema>;

export function transformPersona(raw: PersonaResponse): Persona {
  return PersonaSchema.parse({
    id: raw.id,
    artifactId: raw.artifact_id ?? null,
    slug: raw.slug,
    name: raw.name,
    description: raw.description,
    content: raw.content,
    status: raw.status,
    version: raw.version,
    projectId: raw.project_id,
    contentHash: raw.content_hash,
    sourceSessionId: raw.source_session_id,
    sourcePersonaId: raw.source_persona_id,
    sourceContentHash: raw.source_content_hash,
    createdAt: raw.created_at,
    updatedAt: raw.updated_at,
  });
}

/** Derived usage facts; Rust `PersonaUsage` serializes camelCase. */
export const PersonaUsageSchema = z.object({
  personaId: z.string().min(1),
  boundConversationCount: z.number().int(),
  lastRunAt: z.string().nullable(),
});

export type PersonaUsage = z.infer<typeof PersonaUsageSchema>;

/** Rendered overlay for the next send; Rust `PersonaOverlayPreview` serializes camelCase. */
export const PersonaOverlayPreviewSchema = z.object({
  personaId: z.string().min(1),
  slug: z.string().min(1),
  version: z.number().int(),
  renderedBlock: z.string(),
  skippedReason: z.string().nullable(),
});

export type PersonaOverlayPreview = z.infer<typeof PersonaOverlayPreviewSchema>;

/** Raw `persona:draft_updated` Tauri event payload; never contains persona content. */
export const PersonaDraftUpdatedEventSchema = z.object({
  draft_id: z.string().min(1),
  version: z.number().int(),
  content_hash: z.string().min(1),
  artifact_id: z.string().min(1).nullable().optional(),
  builder_conversation_id: z.string().min(1).optional(),
});

export type PersonaDraftUpdatedEvent = z.infer<
  typeof PersonaDraftUpdatedEventSchema
>;
