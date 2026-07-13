# Agent Personas

## Overview

Agent Personas are prompt-only behavior profiles for Agent conversations. RalphX stores each persona in its local database and binds it to one conversation, so changing a persona affects that conversation's future sends without changing the underlying project or other conversations.

## How to Use Personas

1. Open **Settings → Personas** to create, edit, approve, archive, or delete personas.
2. Use **Build with agent** to draft a persona from selected project material. Drafts are refined through the builder, then approved before they can be bound.
3. In an eligible Project-context Agent conversation, choose a persona from the composer picker before sending.
4. Use the persona chip in the session toolbar to inspect or switch the bound persona. Switching stops an active agent before the new binding takes effect.

## What a Persona Changes

- The persona is appended to the agent's prompt; it is not a separate agent, model, skill, or project setting.
- The binding is per conversation and persists locally, including across app restarts.
- A conversation can have one bound active persona at a time.

## V1 Limitations

| Limitation | V1 behavior |
|---|---|
| Teammates, subagents, and pipeline agents | Teammates, in-process Task subagents, and pipeline agents (worker, reviewer, and merger) are persona-less. |
| Conversation scope | Personas are available only in Project-context Agent conversations; Ideation, Task chat, and Merge chat have no persona binding. |
| External MCP | **BLANKET SUPPRESS:** all external MCP sends have no persona in v1. |
| Native `--agent` paths | Native `--agent` paths bypass injection. RalphX emits `persona:injection_skipped` without persona content. |
| Codex continuity | Codex re-sends the persona each turn. The 10 KB cap bounds the added prompt size but does not solve transcript stacking. |
| Draft iteration | Drafts are iterated only through the builder agent; the manual editor edits active personas, not drafts. |
| Persona chip | The chip appears only in the session toolbar. Project-context hosts without that toolbar have no chip in v1. |

## Rollout and Next Steps

Before Agent Personas are enabled by default, RalphX must complete the [GA gate procedures](../development/persona-ga-gates.md), including live Claude resume and packaged-app ingestion smoke tests.

Planned expansions, including wider binding scopes, pipeline propagation, external-MCP controls, and a Codex compact-digest or clean-session approach, are tracked in [Phase 4 of the personas design](../../.artifacts/specs/agent-personas/design.md#v2-backlog).
