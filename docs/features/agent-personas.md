# Agent Personas

## Overview

Agent Personas are reusable, prompt-only behavior profiles for Agent conversations. A persona is either global or scoped to one project, while its binding remains local to a single eligible conversation.

## Create and Build

- **Manual:** Open **Settings → Personas**, choose **New**, select Global or a project, edit the draft, then approve it.
- **Build with Agent:** In **Settings → Personas**, choose **Build with Agent**, select Global or a project, and RalphX opens the standard Agents composer with **Persona** mode locked.
- **From a project conversation:** Open the conversation's persona chip and choose **Create persona for this project** to open a new project-locked **Persona** builder without returning to Settings.
- **Refine with Agent:** Use a persona's Refine action to open a scope-locked Persona conversation seeded from that persona.

Persona building now uses the normal Agents conversation. Add text files or folder references from the composer, describe and name the desired persona, and answer the builder's questions. The builder saves the result directly to the conversation-bound draft; a Markdown-only chat response is not a completed build. The Persona tab opens with the conversation, shows the saved draft, and provides **Approve persona**. There is no copy/paste step or separate Settings ingestion screen.

Each Persona conversation owns one persona lineage. If a request describes several personas, choose one to build first and start a separate Persona conversation for each additional persona.

Folder references are default-off during rollout. Enable **Settings → Capabilities → Folder context** to show **Add folder** directly below **Add files** in supported Agent composers. Standalone conversations remain controlled by the `standalone_conversations` config/env flag.

## Scope and Context

| Build or use case | Behavior |
|---|---|
| Global build | Runs as a standalone conversation in a private app-owned workspace. |
| Project build | Runs with the selected project's repository plus its private builder workspace. |
| Attached text file | Materialized into the private workspace and exposed to the builder by path. |
| Attached folder | Stored as a live folder reference; contents are read in place and are never copied. Each send records the effective folder as an immutable message-history reference. |
| Persona picker | Offers global personas plus personas scoped to the current project. |
| Refine | Keeps the source persona's scope; approval updates the source lineage. |

The builder's filesystem tools are constrained to the resolved project/workspace/folder roots. Missing or moved referenced content fails closed instead of adding a dangling path to the prompt.

## Versioned Persona Artifacts

Every persona content write appends an immutable artifact version. In a Persona conversation, open the **Persona** artifact tab to inspect the draft or approved result, select historical versions, see who created each version, approve a draft, or open/refine the resulting persona.

Each builder conversation owns at most one draft and retains a result pointer after approval. Plain approvals keep their draft history; seeded refinement appends the approved result to the source persona's lineage.

## Using a Persona

1. Start or open an eligible project Agent conversation.
2. Choose a persona before sending, or use the persona chip to inspect or switch the current binding.
3. RalphX injects the active persona into future sends for that conversation.

Switching a persona stops an active agent before the new binding takes effect. Archiving a persona clears active bindings; deleting a project deletes its drafts, archives its active personas, and clears affected bindings.

## Boundaries

- A persona changes prompt behavior; it is not a model, skill, project setting, or separate agent.
- Manual runtime confirmation may bind a persona only for eligible Workspace-family roles; the selection is an exact role launch override for that project conversation, not persona inheritance for other roles.
- Teammates, delegated agents, pipeline workers/reviewers/mergers, Ideation, Task, Merge, and external MCP sends remain persona-less.
- Persona controls for persona-less roles stay visible but disabled so the backend reason remains visible; Settings also keeps its persona-management route available.
- Standalone chat does not support persona binding; standalone **Persona** mode is the global builder flow.
- Standalone Chat and Standalone PersonaBuilder both support Claude and Codex; PersonaBuilder keeps each provider's MCP-compatible launch policy plus enforced filesystem roots.
- Native agent paths that cannot accept prompt injection report `persona:injection_skipped` without exposing persona content.
- Builder attachments must decode as UTF-8 text (binary files are rejected); file-type filtering in the picker is advisory.

The `agent_personas` and `standalone_conversations` flags gate their corresponding surfaces. Folder references are always available in supported Project and Persona Builder conversations. Global Refine remains unavailable when standalone conversations are disabled.
