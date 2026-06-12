import { describe, it, expect } from 'vitest';
import {
  ProfileRoleSchema,
  ModelSchema,
  PermissionModeSchema,
  AutonomyLevelSchema,
  ClaudeCodeConfigSchema,
  ExecutionConfigSchema,
  IoConfigSchema,
  BehaviorConfigSchema,
  AgentProfileSchema,
  UpdateAgentProfileSchema,
  WORKER_PROFILE,
  REVIEWER_PROFILE,
  ORCHESTRATOR_PROFILE,
  DEEP_RESEARCHER_PROFILE,
  BUILTIN_PROFILES,
  getBuiltinProfile,
  getBuiltinProfileByRole,
  getModelId,
  parseAgentProfile,
  safeParseAgentProfile,
  type AgentProfile,
  type ProfileRole,
} from './agent-profile';

describe('ProfileRoleSchema', () => {
  it('should validate all role values', () => {
    expect(ProfileRoleSchema.parse('worker')).toBe('worker');
    expect(ProfileRoleSchema.parse('reviewer')).toBe('reviewer');
    expect(ProfileRoleSchema.parse('supervisor')).toBe('supervisor');
    expect(ProfileRoleSchema.parse('orchestrator')).toBe('orchestrator');
    expect(ProfileRoleSchema.parse('researcher')).toBe('researcher');
  });

  it('should reject invalid role', () => {
    expect(() => ProfileRoleSchema.parse('invalid')).toThrow();
  });
});

describe('ModelSchema', () => {
  it('should validate all model values', () => {
    expect(ModelSchema.parse('opus')).toBe('opus');
    expect(ModelSchema.parse('sonnet')).toBe('sonnet');
    expect(ModelSchema.parse('haiku')).toBe('haiku');
    expect(ModelSchema.parse('fable')).toBe('fable');
  });

  it('should reject invalid model', () => {
    expect(() => ModelSchema.parse('gpt-4')).toThrow();
  });
});

describe('getModelId', () => {
  it('should return correct model IDs', () => {
    expect(getModelId('opus')).toBe('claude-opus-4-5-20251101');
    expect(getModelId('sonnet')).toBe('claude-sonnet-4-5-20250929');
    expect(getModelId('haiku')).toBe('claude-haiku-4-5-20251001');
    expect(getModelId('fable')).toBe('claude-fable-5');
  });
});

describe('PermissionModeSchema', () => {
  it('should validate all permission modes', () => {
    expect(PermissionModeSchema.parse('default')).toBe('default');
    expect(PermissionModeSchema.parse('acceptEdits')).toBe('acceptEdits');
    expect(PermissionModeSchema.parse('bypassPermissions')).toBe('bypassPermissions');
  });

  it('should reject invalid mode', () => {
    expect(() => PermissionModeSchema.parse('unsafe')).toThrow();
  });
});

describe('AutonomyLevelSchema', () => {
  it('should validate all autonomy levels', () => {
    expect(AutonomyLevelSchema.parse('supervised')).toBe('supervised');
    expect(AutonomyLevelSchema.parse('semi_autonomous')).toBe('semi_autonomous');
    expect(AutonomyLevelSchema.parse('fully_autonomous')).toBe('fully_autonomous');
  });

  it('should reject invalid level', () => {
    expect(() => AutonomyLevelSchema.parse('autonomous')).toThrow();
  });
});

describe('ClaudeCodeConfigSchema', () => {
  it('should validate minimal config', () => {
    const config = { agent: './agents/test.md' };
    const result = ClaudeCodeConfigSchema.parse(config);
    expect(result.agent).toBe('./agents/test.md');
    expect(result.skills).toEqual([]);
    expect(result.mcpServers).toEqual([]);
  });

  it('should validate full config', () => {
    const config = {
      agent: './agents/test.md',
      skills: ['skill1', 'skill2'],
      hooks: { preToolUse: [] },
      mcpServers: ['server1'],
    };
    const result = ClaudeCodeConfigSchema.parse(config);
    expect(result.skills).toHaveLength(2);
    expect(result.mcpServers).toHaveLength(1);
  });

  it('should reject empty agent', () => {
    expect(() => ClaudeCodeConfigSchema.parse({ agent: '' })).toThrow();
  });
});

describe('ExecutionConfigSchema', () => {
  it('should validate config with all fields', () => {
    const config = {
      model: 'sonnet' as const,
      maxIterations: 30,
      timeoutMinutes: 30,
      permissionMode: 'default' as const,
    };
    expect(ExecutionConfigSchema.parse(config)).toEqual(config);
  });

  it('should apply default permission mode', () => {
    const config = {
      model: 'sonnet' as const,
      maxIterations: 30,
      timeoutMinutes: 30,
    };
    const result = ExecutionConfigSchema.parse(config);
    expect(result.permissionMode).toBe('default');
  });

  it('should reject non-positive maxIterations', () => {
    const config = {
      model: 'sonnet' as const,
      maxIterations: 0,
      timeoutMinutes: 30,
    };
    expect(() => ExecutionConfigSchema.parse(config)).toThrow();
  });

  it('should reject non-integer maxIterations', () => {
    const config = {
      model: 'sonnet' as const,
      maxIterations: 10.5,
      timeoutMinutes: 30,
    };
    expect(() => ExecutionConfigSchema.parse(config)).toThrow();
  });
});

describe('IoConfigSchema', () => {
  it('should apply defaults for empty object', () => {
    const result = IoConfigSchema.parse({});
    expect(result.inputArtifactTypes).toEqual([]);
    expect(result.outputArtifactTypes).toEqual([]);
  });

  it('should validate with artifact types', () => {
    const config = {
      inputArtifactTypes: ['prd', 'spec'],
      outputArtifactTypes: ['code', 'tests'],
    };
    expect(IoConfigSchema.parse(config)).toEqual(config);
  });
});

describe('BehaviorConfigSchema', () => {
  it('should apply all defaults', () => {
    const result = BehaviorConfigSchema.parse({});
    expect(result.canSpawnSubAgents).toBe(false);
    expect(result.autoCommit).toBe(false);
    expect(result.autonomyLevel).toBe('supervised');
  });

  it('should validate full config', () => {
    const config = {
      canSpawnSubAgents: true,
      autoCommit: true,
      autonomyLevel: 'fully_autonomous' as const,
    };
    expect(BehaviorConfigSchema.parse(config)).toEqual(config);
  });
});

describe('AgentProfileSchema', () => {
  const validProfile: AgentProfile = {
    id: 'test',
    name: 'Test Agent',
    description: 'A test agent',
    role: 'worker',
    claudeCode: {
      agent: './agents/test.md',
      skills: [],
      mcpServers: [],
    },
    execution: {
      model: 'sonnet',
      maxIterations: 30,
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

  it('should validate complete profile', () => {
    expect(AgentProfileSchema.parse(validProfile)).toEqual(validProfile);
  });

  it('should reject missing id', () => {
    const { id: _id, ...noId } = validProfile;
    expect(() => AgentProfileSchema.parse(noId)).toThrow();
  });

  it('should reject empty name', () => {
    expect(() => AgentProfileSchema.parse({ ...validProfile, name: '' })).toThrow();
  });

  it('should reject invalid role', () => {
    expect(() => AgentProfileSchema.parse({ ...validProfile, role: 'invalid' })).toThrow();
  });
});

describe('UpdateAgentProfileSchema', () => {
  it('should validate with only id', () => {
    const result = UpdateAgentProfileSchema.parse({ id: 'test' });
    expect(result.id).toBe('test');
    expect(result.name).toBeUndefined();
  });

  it('should validate with partial update', () => {
    const update = {
      id: 'test',
      name: 'Updated Name',
      execution: { maxIterations: 50 },
    };
    const result = UpdateAgentProfileSchema.parse(update);
    expect(result.name).toBe('Updated Name');
    expect(result.execution?.maxIterations).toBe(50);
  });
});

describe('Built-in profiles', () => {
  it('WORKER_PROFILE should be valid', () => {
    expect(AgentProfileSchema.parse(WORKER_PROFILE)).toEqual(WORKER_PROFILE);
    expect(WORKER_PROFILE.role).toBe('worker');
    expect(WORKER_PROFILE.execution.model).toBe('sonnet');
    expect(WORKER_PROFILE.execution.maxIterations).toBe(30);
    expect(WORKER_PROFILE.behavior.autoCommit).toBe(true);
  });

  it('REVIEWER_PROFILE should be valid', () => {
    expect(AgentProfileSchema.parse(REVIEWER_PROFILE)).toEqual(REVIEWER_PROFILE);
    expect(REVIEWER_PROFILE.role).toBe('reviewer');
    expect(REVIEWER_PROFILE.execution.model).toBe('sonnet');
    expect(REVIEWER_PROFILE.execution.maxIterations).toBe(10);
  });

  it('ORCHESTRATOR_PROFILE should be valid', () => {
    expect(AgentProfileSchema.parse(ORCHESTRATOR_PROFILE)).toEqual(ORCHESTRATOR_PROFILE);
    expect(ORCHESTRATOR_PROFILE.role).toBe('orchestrator');
    expect(ORCHESTRATOR_PROFILE.execution.model).toBe('opus');
    expect(ORCHESTRATOR_PROFILE.execution.maxIterations).toBe(50);
    expect(ORCHESTRATOR_PROFILE.behavior.canSpawnSubAgents).toBe(true);
  });

  it('DEEP_RESEARCHER_PROFILE should be valid', () => {
    expect(AgentProfileSchema.parse(DEEP_RESEARCHER_PROFILE)).toEqual(DEEP_RESEARCHER_PROFILE);
    expect(DEEP_RESEARCHER_PROFILE.role).toBe('researcher');
    expect(DEEP_RESEARCHER_PROFILE.execution.model).toBe('opus');
    expect(DEEP_RESEARCHER_PROFILE.execution.maxIterations).toBe(200);
  });

  it('BUILTIN_PROFILES should contain the 4 live built-in agent profiles', () => {
    expect(BUILTIN_PROFILES).toHaveLength(4);
    expect(BUILTIN_PROFILES.map((p) => p.id)).toEqual([
      'worker',
      'reviewer',
      'orchestrator',
      'deep-researcher',
    ]);
  });
});

describe('getBuiltinProfile', () => {
  it('should return profile by id', () => {
    expect(getBuiltinProfile('worker')).toEqual(WORKER_PROFILE);
    expect(getBuiltinProfile('supervisor')).toBeUndefined();
  });

  it('should return undefined for unknown id', () => {
    expect(getBuiltinProfile('unknown')).toBeUndefined();
  });
});

describe('getBuiltinProfileByRole', () => {
  it('should return profile by role', () => {
    expect(getBuiltinProfileByRole('worker')).toEqual(WORKER_PROFILE);
    expect(getBuiltinProfileByRole('orchestrator')).toEqual(ORCHESTRATOR_PROFILE);
  });

  it('should return undefined for unmatched role', () => {
    const profiles: ProfileRole[] = ['worker', 'reviewer', 'orchestrator', 'researcher'];
    for (const role of profiles) {
      expect(getBuiltinProfileByRole(role)).toBeDefined();
    }
    expect(getBuiltinProfileByRole('supervisor')).toBeUndefined();
  });
});

describe('parseAgentProfile', () => {
  it('should parse valid profile', () => {
    const result = parseAgentProfile(WORKER_PROFILE);
    expect(result.id).toBe('worker');
  });

  it('should throw on invalid profile', () => {
    expect(() => parseAgentProfile({})).toThrow();
  });
});

describe('safeParseAgentProfile', () => {
  it('should return profile on valid input', () => {
    const result = safeParseAgentProfile(WORKER_PROFILE);
    expect(result).not.toBeNull();
    expect(result?.id).toBe('worker');
  });

  it('should return null on invalid input', () => {
    expect(safeParseAgentProfile({})).toBeNull();
    expect(safeParseAgentProfile(null)).toBeNull();
    expect(safeParseAgentProfile(undefined)).toBeNull();
  });
});
