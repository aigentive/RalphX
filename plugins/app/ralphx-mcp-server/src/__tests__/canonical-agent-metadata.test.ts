import { describe, expect, it } from 'vitest';
import { SAFE_CANONICAL_PROFILE_NAME } from '../canonical-agent-metadata.js';

describe('SAFE_CANONICAL_PROFILE_NAME', () => {
  // Rust counterpart: trusted_canonical_profile_name in
  // src-tauri/src/infrastructure/agents/harness_agent_catalog.rs must accept exactly the same set.
  // This parity test prevents a cross-layer disagreement from breaking Team mode spawn.
  it.each([
    ['team_coordinator', true],
    ['plan', true],
    ['a1', true],
    ['a-b_c', true],
    ['', false],
    ['../x', false],
    ['a/b', false],
    ['a\\b', false],
    ['Team_Coordinator', false],
    ['_lead', false],
    ['lead_', false],
    ['team__lead', false],
    ['..', false],
  ])('accepts %j: %s', (profileName, isSafe) => {
    expect(SAFE_CANONICAL_PROFILE_NAME.test(profileName)).toBe(isSafe);
  });
});
