/**
 * The component-facing `ui:agent` gate (PR 2.6-b).
 *
 * Synchronous zustand selectors only — a gate must not put a fetch on a click path
 * (rule 24). Both inputs are already in `environmentStore`, so this adds no
 * subscription beyond the one store the app is already wired to.
 *
 * P-28 consumption: the scopes read here are the LIVE confirmed set the supervisor
 * wrote after `GET /remote/v1/session`. The pair-time snapshot on
 * `remoteEnvironmentSummarySchema` (`entry.remote.scopes`) is deliberately NOT
 * consulted — it records what was granted when the environment was added, so a host
 * that has since revoked `ui:agent` would still present every steering control as
 * live until someone re-paired. `agent-gate.consumption.test.ts` pins that with a
 * negative test.
 */

import { useEnvironmentStore } from "@/stores/environmentStore";
import {
  resolveAffordanceGate,
  resolveAgentGate,
  resolveFieldGate,
  type AgentGateState,
  type AgentGatedAffordance,
} from "@/lib/remote/agent-gate";

/** The live confirmed scopes for the active environment; `null` when unconfirmed. */
export function useActiveEffectiveScopes(): readonly string[] | null {
  return useEnvironmentStore(
    (state) => state.effectiveScopes[state.activeEnvironmentId] ?? null
  );
}

function useIsRemote(): boolean {
  return useEnvironmentStore((state) => {
    const entry = state.environments.find(
      (candidate) => candidate.id === state.activeEnvironmentId
    );
    return (entry?.kind ?? "remote") !== "local";
  });
}

/**
 * Whether an affordance must be disabled, and the copy to explain it.
 *
 * Pass the affordance name wherever one exists: it is the only way to distinguish
 * `unavailable` (the host does not expose this op remotely — no host setting helps)
 * from `gated` (the host can grant `ui:agent`). Omitting it falls back to the
 * scope-only answer, which is correct but always says "enable it on the host".
 *
 * Local environments are never gated. Remote environments are gated unless the live
 * confirmed scopes include `ui:agent` — including when they have never been
 * confirmed, which reads as "unknown", not "unrestricted".
 */
export function useAgentGate(affordance?: AgentGatedAffordance): AgentGateState {
  const isRemote = useIsRemote();
  const scopes = useActiveEffectiveScopes();
  return affordance === undefined
    ? resolveAgentGate(isRemote, scopes)
    : resolveAffordanceGate(affordance, isRemote, scopes);
}

/**
 * Field-level gate for an argument-sensitive op (today: `update_task`).
 *
 * Lets a form lock the fields that need `ui:agent` while leaving the inert ones
 * editable, matching what the host will actually enforce.
 */
export function useFieldGate(command: string, field: string): AgentGateState {
  const isRemote = useIsRemote();
  const scopes = useActiveEffectiveScopes();
  return resolveFieldGate(command, field, isRemote, scopes);
}
