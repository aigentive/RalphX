# Internal Skills

Internal skills are RalphX-owned instruction packs that can be injected into an agent's runtime prompt when a turn needs specialized guidance.

They are different from Claude or Codex native skills:

| Capability | Internal skills |
|---|---|
| Source of truth | RalphX repo/runtime assets |
| Harness support | Provider-neutral: Claude, Codex, and future harnesses use the same resolver |
| Loading control | Per-agent allowlists in canonical agent config |
| User home dependency | None required |
| Main purpose | Give an agent narrow, versioned guidance for a specific RalphX workflow |

Internal skills are designed for RalphX product behavior. Native provider skills can still exist for external agent workflows, but RalphX does not depend on provider-specific skill discovery for internal orchestration.

---

## Where Skills Live

RalphX loads internal skills from trusted RalphX-owned roots:

| Root | Purpose |
|---|---|
| `plugins/app/skills/<skill>/SKILL.md` | App-internal skills used by RalphX-owned agents |
| `plugins/shared/skills/<skill>/SKILL.md` | Shared skills that can also support external/plugin workflows |

Each skill is a directory with a `SKILL.md` file. The file may include frontmatter:

```markdown
---
name: ralphx-agent-workspace-swe
description: Agent Workspace workflow bridge guidance
trigger: agent workspace workflow event
disable-model-invocation: true
user-invocable: false
---

# RalphX Agent Workspace SWE

...
```

Supported frontmatter fields:

| Field | Meaning |
|---|---|
| `name` | Canonical skill name. Defaults to the directory name if omitted. |
| `description` | Used for matching and documentation. |
| `trigger` | Optional phrase used by auto-matching. |
| `disable-model-invocation` | Prevents automatic loading from normal prompt text. Direct backend directives can still load it. |
| `user-invocable` | Allows a user/manual prompt to request the skill when the agent also allowlists it. |
| `priority` | Tie-breaker when several auto-matched skills score similarly. |

Skill names must use lowercase ASCII letters, digits, and hyphens only. Path separators and parent-directory segments are rejected.

---

## Agent Allowlists

An internal skill can load only when the target agent explicitly allowlists it.

The allowlist lives in canonical agent config:

```yaml
capabilities:
  internal_skills:
    allowed:
      - ralphx-agent-workspace-swe
    auto_match: true
    max_auto_loaded: 2
```

| Setting | Default | Meaning |
|---|---|---|
| `allowed` | `[]` | Skills this agent may load. Empty means no internal skills. |
| `auto_match` | `false` | Whether RalphX may match skills from prompt text without an explicit directive. |
| `max_auto_loaded` | `2` | Maximum automatically matched skills for one prompt. Explicit directives are not auto-match slots. |

The allowlist is a hard boundary. User text cannot load a skill that the agent does not allowlist.

---

## How Matching Works

RalphX selects skills before spawning or composing the provider prompt.

Selection order:

1. Backend-authored directives.
2. Manual invocation for skills marked `user-invocable: true`.
3. Auto-match when the agent has `auto_match: true`.

Backend directives are exact:

```text
<!-- ralphx_internal_skill=ralphx-agent-workspace-swe -->
```

or:

```text
Use /ralphx-agent-workspace-swe skill
```

If an exact directive names a skill outside the agent's allowlist, RalphX fails closed instead of silently injecting it. Non-directive mentions of non-allowlisted skills are ignored.

Auto-match scores against the skill name, description, trigger text, and selected metadata. Skills with `disable-model-invocation: true` do not auto-match from prompt text.

---

## What Gets Injected

When a skill is selected, RalphX injects an internal-skill block into the agent system/developer prompt:

```xml
<ralphx_internal_skills>
RalphX selected the following internal skills for this turn...
<internal_skill name="ralphx-agent-workspace-swe">
...
</internal_skill>
</ralphx_internal_skills>
```

The injected text is runtime guidance. It is not a user-authored chat message and should not be treated as conversation content.

Current prompt paths:

| Harness | Behavior |
|---|---|
| Claude | Uses enriched system prompt text when a skill is selected. |
| Codex | Enriches the composed RalphX agent instruction block. |

---

## Current Built-In Skill

### `ralphx-agent-workspace-swe`

Purpose: guide an Agent Workspace agent when RalphX sends workflow bridge events from an attached Ideation run.

Current allowlist:

| Agent | Allowed? | Reason |
|---|---:|---|
| `ralphx-chat-project` | Yes | Ideation-mode Agent Workspace conversations use this agent. |
| `ralphx-general-worker` | No | General Edit workspace agent; not an Ideation bridge recipient. |
| `ralphx-general-explorer` | No | General Chat workspace agent; not an Ideation bridge recipient. |

The bridge only wakes active Ideation workspaces with a linked ideation session.

The skill's default stance is report-only:

| Event style | Expected agent behavior |
|---|---|
| Normal pipeline progress | Summarize briefly; no tools. |
| Blocked, failed, merge incomplete, cancelled | Explain what happened; do not retry unless explicitly instructed. |
| Explicit workspace-actionable issue | Use available tools only when the payload or user instruction clearly requires intervention. |

This keeps the Agent Workspace agent from competing with the task pipeline scheduler, verifier, reviewer, or merger.

---

## Security And Safety Rules

| Rule | Why it matters |
|---|---|
| Skills load only from RalphX-owned roots | Prevents arbitrary project/user files from becoming system instructions. |
| Agents must allowlist every skill | Prevents broad guidance from leaking into unrelated agents. |
| Backend directives fail closed when disallowed | Prevents hidden prompt text from escalating an agent's role. |
| Provider-native skill loading is optional | Keeps behavior stable across Claude, Codex, and future harnesses. |
| Skills must describe only live tools/surfaces | Prevents agents from trying tools they do not have. |

---

## Troubleshooting

| Symptom | Likely cause | Fix |
|---|---|---|
| Skill does not load | Agent does not allowlist it | Add the skill under `capabilities.internal_skills.allowed` for that agent. |
| Skill directive errors | Backend requested a non-allowlisted skill | Fix the target agent or route the event to the correct agent. |
| Auto-match does not load a skill | `auto_match` is off or the skill has `disable-model-invocation: true` | Use an explicit backend directive or enable auto-match intentionally. |
| Skill loads for the wrong agent | Allowlist is too broad | Narrow `capabilities.internal_skills.allowed` and add a regression test. |
| Agent mentions unavailable tools | Skill content does not match that agent's live surface | Update the skill or split it into a narrower skill. |

---

## When To Add A New Internal Skill

Add an internal skill when:

- several agents need the same specialized RalphX workflow guidance
- the guidance should be versioned with the app
- the behavior must work across harnesses
- the instructions are too large or too conditional for the base agent prompt

Do not add an internal skill when a normal agent prompt edit is enough, or when the guidance only belongs to one short-lived implementation detail.
