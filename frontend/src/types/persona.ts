import { z } from "zod";

export const PersonaStatusSchema = z.enum(["draft", "active", "archived"]);

/** Frontend persona model after the snake_case IPC response is transformed. */
export const PersonaSchema = z.object({
  id: z.string().min(1),
  slug: z.string().min(1),
  name: z.string().min(1),
  description: z.string(),
  content: z.string(),
  status: PersonaStatusSchema,
  version: z.number().int(),
  contentHash: z.string().min(1),
  sourceSessionId: z.string().nullable().optional(),
  createdAt: z.string().datetime({ offset: true }),
  updatedAt: z.string().datetime({ offset: true }),
});

export type Persona = z.infer<typeof PersonaSchema>;

/** Raw IPC response. `Persona` has no Rust `rename_all`, so it serializes snake_case. */
export const PersonaResponseSchema = z.object({
  id: z.string().min(1),
  slug: z.string().min(1),
  name: z.string().min(1),
  description: z.string(),
  content: z.string(),
  status: PersonaStatusSchema,
  version: z.number().int(),
  content_hash: z.string().min(1),
  source_session_id: z.string().nullable().optional(),
  created_at: z.string().datetime({ offset: true }),
  updated_at: z.string().datetime({ offset: true }),
});

export type PersonaResponse = z.infer<typeof PersonaResponseSchema>;

export function transformPersona(raw: PersonaResponse): Persona {
  return PersonaSchema.parse({
    id: raw.id,
    slug: raw.slug,
    name: raw.name,
    description: raw.description,
    content: raw.content,
    status: raw.status,
    version: raw.version,
    contentHash: raw.content_hash,
    sourceSessionId: raw.source_session_id,
    createdAt: raw.created_at,
    updatedAt: raw.updated_at,
  });
}

export const PersonaIngestEntrySchema = z.object({
  path: z.string().min(1),
  reason: z.string().min(1).optional(),
});

export const PersonaIngestManifestSchema = z.object({
  copied: z.array(PersonaIngestEntrySchema),
  skipped: z.array(PersonaIngestEntrySchema),
  rejected: z.array(PersonaIngestEntrySchema),
});

export type PersonaIngestManifest = z.infer<typeof PersonaIngestManifestSchema>;

export const PersonaBuilderIngestStatusSchema = z.object({
  live: z.boolean(),
});

export type PersonaBuilderIngestStatus = z.infer<
  typeof PersonaBuilderIngestStatusSchema
>;

/** Raw `persona:draft_updated` Tauri event payload; never contains persona content. */
export const PersonaDraftUpdatedEventSchema = z.object({
  draft_id: z.string().min(1),
  version: z.number().int(),
  content_hash: z.string().min(1),
});

export type PersonaDraftUpdatedEvent = z.infer<
  typeof PersonaDraftUpdatedEventSchema
>;
