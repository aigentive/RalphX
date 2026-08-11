import { z } from "zod";

import type {
  ComposerRoleDefault,
  ManualRoleCatalog,
  ManualRoleCatalogEntry,
  ManualRoleControlOption,
  ManualRoleDefault,
} from "./manual-role-defaults.types";
import {
  ComposerRoleDefaultResponseSchema,
  ManualRoleCatalogEntryResponseSchema,
  ManualRoleCatalogResponseSchema,
  ManualRoleControlOptionResponseSchema,
  ManualRoleDefaultResponseSchema,
} from "./manual-role-defaults.schemas";

export function transformManualRoleDefault(
  raw: z.infer<typeof ManualRoleDefaultResponseSchema>,
): ManualRoleDefault {
  return {
    provider: raw.provider,
    model: raw.model ?? null,
    effort: raw.effort ?? null,
    serviceTier: raw.service_tier,
    coordinationMode: raw.coordination_mode ?? null,
    personaId: raw.persona_id ?? null,
    approvalPolicy: raw.approval_policy ?? null,
    sandboxMode: raw.sandbox_mode ?? null,
    atlassianAccess: raw.atlassian_access ?? null,
  };
}

function transformControlOption(
  raw: z.infer<typeof ManualRoleControlOptionResponseSchema>,
): ManualRoleControlOption {
  return {
    value: raw.value,
    enabled: raw.enabled,
    disabledReason: raw.disabled_reason ?? null,
  };
}

export function transformManualRoleCatalogEntry(
  raw: z.infer<typeof ManualRoleCatalogEntryResponseSchema>,
): ManualRoleCatalogEntry {
  return {
    role: raw.role,
    displayName: raw.display_name,
    description: raw.description,
    family: raw.family,
    familyDisplayName: raw.family_display_name,
    requiresTasks: raw.requires_tasks,
    configured: raw.configured ? transformManualRoleDefault(raw.configured) : null,
    effective: raw.effective ? transformManualRoleDefault(raw.effective) : null,
    source: raw.source ?? null,
    diagnostics: raw.diagnostics,
    controls: {
      capabilities: raw.controls.capabilities.map(transformControlOption),
      speeds: raw.controls.speeds.map(transformControlOption),
      persona: {
        enabled: raw.controls.persona.enabled,
        disabledReason: raw.controls.persona.disabled_reason ?? null,
      },
    },
  };
}

export function transformManualRoleCatalog(
  raw: z.infer<typeof ManualRoleCatalogResponseSchema>,
): ManualRoleCatalog {
  return {
    projectId: raw.project_id ?? null,
    roles: raw.roles.map(transformManualRoleCatalogEntry),
  };
}

export function transformComposerRoleDefault(
  raw: z.infer<typeof ComposerRoleDefaultResponseSchema>,
): ComposerRoleDefault {
  return {
    role: raw.role,
    source: raw.source,
    value: transformManualRoleDefault(raw.value),
  };
}
