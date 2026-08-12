export type ManualServiceTier = "provider_default" | "standard" | "fast";

/** Atlassian MCP tool tier for a routing role. */
export type AtlassianMcpAccess = "none" | "read" | "read_write";

export interface ManualRoleDefault {
  provider: string;
  model: string | null;
  effort: string | null;
  serviceTier: ManualServiceTier;
  coordinationMode: string | null;
  personaId: string | null;
  approvalPolicy: string | null;
  sandboxMode: string | null;
  /** `null` means "use this role's built-in Atlassian tier". */
  atlassianAccess: AtlassianMcpAccess | null;
}

export type ManualRoleRuntimeSelection = Pick<
  ManualRoleDefault,
  | "provider"
  | "model"
  | "effort"
  | "serviceTier"
  | "coordinationMode"
  | "personaId"
>;

export interface ManualRoleControlOption {
  value: string;
  enabled: boolean;
  disabledReason: string | null;
}

export interface ManualRoleControls {
  capabilities: ManualRoleControlOption[];
  speeds: ManualRoleControlOption[];
  persona: {
    enabled: boolean;
    disabledReason: string | null;
  };
}

export interface ManualRoleCatalogEntry {
  role: string;
  displayName: string;
  description: string;
  family: string;
  familyDisplayName: string;
  requiresTasks: boolean;
  configured: ManualRoleDefault | null;
  effective: ManualRoleDefault | null;
  source: string | null;
  diagnostics: string[];
  controls: ManualRoleControls;
}

export interface ManualRoleCatalog {
  projectId: string | null;
  roles: ManualRoleCatalogEntry[];
}

export interface ComposerRoleDefault {
  role: string;
  source: string;
  value: ManualRoleDefault;
}

export interface StartComposerRoleDefaultInput {
  projectId: string;
  mode: string;
}

export interface AgentConversationRoleDefaultInput {
  conversationId: string;
}

export interface UpdateManualRoleDefaultInput {
  projectId: string | null;
  role: string;
  value: ManualRoleDefault;
}

export interface ClearManualRoleDefaultInput {
  projectId: string | null;
  role: string;
}
