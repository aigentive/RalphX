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
import { resolveAgentGate, type AgentGateState } from "@/lib/remote/agent-gate";

/** The live confirmed scopes for the active environment; `null` when unconfirmed. */
export function useActiveEffectiveScopes(): readonly string[] | null {
  return useEnvironmentStore(
    (state) => state.effectiveScopes[state.activeEnvironmentId] ?? null
  );
}

/**
 * Whether agent-steering affordances must be disabled, and the copy to explain it.
 *
 * Local environments are never gated. Remote environments are gated unless the live
 * confirmed scopes include `ui:agent` — including when they have never been
 * confirmed, which reads as "unknown", not "unrestricted".
 */
export function useAgentGate(): AgentGateState {
  return useEnvironmentStore((state) => {
    const entry = state.environments.find(
      (candidate) => candidate.id === state.activeEnvironmentId
    );
    const isRemote = (entry?.kind ?? "remote") !== "local";
    return resolveAgentGate(
      isRemote,
      state.effectiveScopes[state.activeEnvironmentId] ?? null
    );
  });
}
