# Agent Harnesses

RalphX can route different parts of the product through different agent harnesses. A harness is the external AI runtime RalphX launches, supervises, resumes, and parses events from.

Today RalphX supports two harnesses:

| Harness | Best fit today | Notes |
|---|---|---|
| `claude` | Full execution pipeline, team mode, mature plugin/MCP flows | Still the default harness |
| `codex` | Ideation plus execution, review, and merge lanes when explicitly configured | Uses Codex CLI semantics, not Claude plugin semantics |

---

## Key idea

Harness choice is now **lane-based**, not app-wide.

That means you can configure different harnesses for different workflow lanes, for example:

| Lane | Example choice |
|---|---|
| Ideation primary | Codex |
| Ideation verifier | Codex |
| Execution worker | Codex |
| Execution reviewer | Codex |
| Execution merger | Codex |

This lets you adopt Codex incrementally without forcing the whole product onto a single runtime.

---

## Architecture direction

RalphX is treating Claude and Codex as the first two entries in a longer-lived multi-harness surface.

That means new harness work is expected to flow through shared:

- harness registries keyed by `AgentHarnessKind`
- runtime adapters for probing, CLI/bootstrap resolution, and startup integration
- client/factory bundles instead of one-off provider fields
- provider-neutral session/run metadata such as `provider_harness` and `provider_session_id`

The goal is to make adding a future harness a targeted extension of that shared surface, not another repo-wide `claude + X` refactor.

RalphX also has provider-neutral [internal skills](internal-skills.md). These are RalphX-owned instruction packs that can be injected into Claude, Codex, or future harness prompts through canonical per-agent allowlists.

---

## Where you configure it

Use the dedicated harness screens in the desktop app:

- **Settings → General → Execution Agents** for worker, reviewer, re-executor, and merger lanes
- **Settings → Ideation → Ideation Agents** for ideation, verifier, and specialist lanes

RalphX stores harness settings with the same layered precedence used elsewhere:

1. project-specific settings
2. global settings
3. YAML defaults
4. built-in defaults

The backend also supports per-lane model, effort, approval-policy, sandbox, and fallback-harness settings.

---

## Current Codex limitations

Codex support is intentionally incremental. The current product contract is:

| Area | Current behavior |
|---|---|
| Team mode | Claude-only |
| Codex team sessions | Not supported; Codex runs are normalized to solo mode |
| Codex execution/review/merge | Supported when those lanes are configured to Codex |
| Legacy Claude sessions/data | Still supported; provider-neutral fields are additive |
| Harness fallback | A lane may fall back to another harness if configured to do so |

If a lane resolves to Codex but Codex is unavailable, RalphX can fall back to Claude when that lane is configured with a Claude fallback.

---

## How session data works now

Older RalphX data used Claude-specific fields such as `claude_session_id`.

RalphX now stores provider-neutral metadata:

| Field | Meaning |
|---|---|
| `provider_harness` | Which harness produced the run/session |
| `provider_session_id` | Harness-native session or thread id |

Legacy Claude fields still work for older data. Newer code treats the provider-neutral fields as canonical.

---

## What to expect in the UI

You may see harness-related behavior in several places:

| Surface | What you will see |
|---|---|
| Conversation history | Harness badges plus stored-session vs new-attempt routing hints |
| Active chat header | Current harness plus provider-session lineage for the selected run |
| Assistant messages | Provider metadata badges for the active conversation when a stored harness/session exists |
| Execution settings | First-class execution lane harness selection and related options |
| Ideation settings | First-class ideation lane harness selection and related options |
| Runtime availability checks | Errors that refer to the selected harness, not only Claude |
| Recovery/resume flows | Provider-aware session recovery instead of Claude-only assumptions |

---

## Choosing a harness

Use Claude when you need:

- the broadest current feature coverage
- team mode
- established plugin-driven workflows

Use Codex when you want:

- Codex-native ideation, execution, review, or merge on a specific lane
- Codex sandbox/approval semantics for that lane
- incremental adoption without moving the whole product to one runtime

---

## Recommended rollout

If you are enabling Codex for the first time, start with:

1. ideation primary
2. ideation verifier
3. execution worker
4. execution reviewer / merger once you are comfortable with that project’s workflow

That gives you the lowest-risk adoption path while still letting you graduate into a full Codex-backed execution pipeline per project.
