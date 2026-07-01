import { z } from 'zod';

/**
 * Agent roles in the RalphX system
 */
export const ProfileRoleSchema = z.enum([
  'worker',
  'reviewer',
  'supervisor',
  'orchestrator',
  'researcher',
]);

export type ProfileRole = z.infer<typeof ProfileRoleSchema>;

/**
 * Model short forms for Claude models
 * - opus: claude-opus-4-5-20251101
 * - sonnet: claude-sonnet-4-6
 * - haiku: claude-haiku-4-5-20251001
 * - fable: claude-fable-5
 */
export const ModelSchema = z.enum(['opus', 'sonnet', 'haiku', 'fable']);

export type Model = z.infer<typeof ModelSchema>;

/**
 * Get the full model ID from short form
 */
export function getModelId(model: Model): string {
  switch (model) {
    case 'opus':
      return 'claude-opus-4-5-20251101';
    case 'sonnet':
      return 'claude-sonnet-4-6';
    case 'haiku':
      return 'claude-haiku-4-5-20251001';
    case 'fable':
      return 'claude-fable-5';
  }
}

/**
 * Permission mode for agent execution
 */
export const PermissionModeSchema = z.enum([
  'default',
  'acceptEdits',
  'bypassPermissions',
]);

export type PermissionMode = z.infer<typeof PermissionModeSchema>;

/**
 * Autonomy level for agent behavior
 */
export const AutonomyLevelSchema = z.enum([
  'supervised',
  'semi_autonomous',
  'fully_autonomous',
]);

export type AutonomyLevel = z.infer<typeof AutonomyLevelSchema>;

/**
 * Claude Code component configuration
 */
export const ClaudeCodeConfigSchema = z.object({
  /** Agent name (resolved via --plugin-dir, e.g. "worker") */
  agent: z.string().min(1),
  /** Skills to inject at startup (resolved via plugin discovery) */
  skills: z.array(z.string()).default([]),
  /** Agent-scoped hooks configuration */
  hooks: z.unknown().optional(),
  /** MCP servers to enable */
  mcpServers: z.array(z.string()).default([]),
});

export type ClaudeCodeConfig = z.infer<typeof ClaudeCodeConfigSchema>;

/**
 * Execution configuration
 */
export const ExecutionConfigSchema = z.object({
  model: ModelSchema,
  maxIterations: z.number().int().positive(),
  timeoutMinutes: z.number().int().positive(),
  permissionMode: PermissionModeSchema.default('default'),
});

export type ExecutionConfig = z.infer<typeof ExecutionConfigSchema>;

/**
 * Artifact I/O configuration
 */
export const IoConfigSchema = z.object({
  inputArtifactTypes: z.array(z.string()).default([]),
  outputArtifactTypes: z.array(z.string()).default([]),
});

export type IoConfig = z.infer<typeof IoConfigSchema>;

/**
 * Behavioral configuration
 */
export const BehaviorConfigSchema = z.object({
  canSpawnSubAgents: z.boolean().default(false),
  autoCommit: z.boolean().default(false),
  autonomyLevel: AutonomyLevelSchema.default('supervised'),
});

export type BehaviorConfig = z.infer<typeof BehaviorConfigSchema>;

/**
 * Complete agent profile schema
 */
export const AgentProfileSchema = z.object({
  id: z.string().min(1),
  name: z.string().min(1),
  description: z.string().min(1),
  role: ProfileRoleSchema,
  claudeCode: ClaudeCodeConfigSchema,
  execution: ExecutionConfigSchema,
  io: IoConfigSchema,
  behavior: BehaviorConfigSchema,
});

export type AgentProfile = z.infer<typeof AgentProfileSchema>;

/**
 * Schema for creating a new agent profile
 */
export const CreateAgentProfileSchema = AgentProfileSchema;

export type CreateAgentProfile = z.infer<typeof CreateAgentProfileSchema>;

/**
 * Schema for updating an agent profile (all fields optional except id)
 */
export const UpdateAgentProfileSchema = z.object({
  id: z.string().min(1),
  name: z.string().min(1).optional(),
  description: z.string().min(1).optional(),
  role: ProfileRoleSchema.optional(),
  claudeCode: ClaudeCodeConfigSchema.partial().optional(),
  execution: ExecutionConfigSchema.partial().optional(),
  io: IoConfigSchema.partial().optional(),
  behavior: BehaviorConfigSchema.partial().optional(),
});

export type UpdateAgentProfile = z.infer<typeof UpdateAgentProfileSchema>;

/**
 * Built-in worker profile
 */
export const WORKER_PROFILE: AgentProfile = {
  id: 'worker',
  name: 'Worker',
  description: 'Executes implementation tasks autonomously',
  role: 'worker',
  claudeCode: {
    agent: 'worker',
    skills: ['coding-standards', 'testing-patterns', 'git-workflow'],
    mcpServers: [],
  },
  execution: {
    model: 'sonnet',
    maxIterations: 30,
    timeoutMinutes: 30,
    permissionMode: 'acceptEdits',
  },
  io: {
    inputArtifactTypes: [],
    outputArtifactTypes: [],
  },
  behavior: {
    canSpawnSubAgents: false,
    autoCommit: true,
    autonomyLevel: 'semi_autonomous',
  },
};

/**
 * Built-in reviewer profile
 */
export const REVIEWER_PROFILE: AgentProfile = {
  id: 'reviewer',
  name: 'Reviewer',
  description: 'Reviews code changes for quality and correctness',
  role: 'reviewer',
  claudeCode: {
    agent: 'reviewer',
    skills: ['code-review-checklist'],
    mcpServers: [],
  },
  execution: {
    model: 'sonnet',
    maxIterations: 10,
    timeoutMinutes: 30,
    permissionMode: 'default',
  },
  io: {
    inputArtifactTypes: [],
    outputArtifactTypes: [],
  },
  behavior: {
    canSpawnSubAgents: false,
    autoCommit: false,
    autonomyLevel: 'supervised',
  },
};

/**
 * Built-in orchestrator profile
 */
export const ORCHESTRATOR_PROFILE: AgentProfile = {
  id: 'orchestrator',
  name: 'Orchestrator',
  description: 'Plans and coordinates complex multi-step tasks',
  role: 'orchestrator',
  claudeCode: {
    agent: 'orchestrator',
    skills: [],
    mcpServers: [],
  },
  execution: {
    model: 'opus',
    maxIterations: 50,
    timeoutMinutes: 60,
    permissionMode: 'default',
  },
  io: {
    inputArtifactTypes: [],
    outputArtifactTypes: [],
  },
  behavior: {
    canSpawnSubAgents: true,
    autoCommit: false,
    autonomyLevel: 'fully_autonomous',
  },
};

/**
 * Built-in deep-researcher profile
 */
export const DEEP_RESEARCHER_PROFILE: AgentProfile = {
  id: 'deep-researcher',
  name: 'Deep Researcher',
  description: 'Conducts thorough research and analysis',
  role: 'researcher',
  claudeCode: {
    agent: 'deep-researcher',
    skills: ['research-methodology'],
    mcpServers: [],
  },
  execution: {
    model: 'opus',
    maxIterations: 200,
    timeoutMinutes: 120,
    permissionMode: 'default',
  },
  io: {
    inputArtifactTypes: [],
    outputArtifactTypes: [],
  },
  behavior: {
    canSpawnSubAgents: false,
    autoCommit: false,
    autonomyLevel: 'fully_autonomous',
  },
};

/**
 * All built-in profiles
 */
export const BUILTIN_PROFILES: AgentProfile[] = [
  WORKER_PROFILE,
  REVIEWER_PROFILE,
  ORCHESTRATOR_PROFILE,
  DEEP_RESEARCHER_PROFILE,
];

/**
 * Get a built-in profile by ID
 */
export function getBuiltinProfile(id: string): AgentProfile | undefined {
  return BUILTIN_PROFILES.find((p) => p.id === id);
}

/**
 * Get a built-in profile by role
 */
export function getBuiltinProfileByRole(role: ProfileRole): AgentProfile | undefined {
  return BUILTIN_PROFILES.find((p) => p.role === role);
}

/**
 * Parse and validate an agent profile
 */
export function parseAgentProfile(json: unknown): AgentProfile {
  return AgentProfileSchema.parse(json);
}

/**
 * Safely parse an agent profile, returning null on failure
 */
export function safeParseAgentProfile(json: unknown): AgentProfile | null {
  const result = AgentProfileSchema.safeParse(json);
  return result.success ? result.data : null;
}
