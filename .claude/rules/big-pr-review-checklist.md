---
paths:
  - ".github/PULL_REQUEST_TEMPLATE.md"
  - ".github/workflows/**"
---

> **Maintainer note:** This file optimizes for LLM context efficiency. Rules: (1) Tables > prose (2) One example max per concept (3) No redundant explanations (4) Use symbols: → = leads to, | = or, ❌/✅ = wrong/right (5) Before adding content, ask: "Can this be a single line?" If yes, make it one line.

# Big-PR Review Checklist

Source: fix-chain analysis of one month of merged PRs (12 chains, 90 regression-fix PRs classified). Large feature/refactor PRs in this codebase repeatedly ship the SAME failure classes; run this checklist on any big PR before merge. Complements `stateful-workflow-review.md` (deep lens for completion/cache/retry/recovery changes).

## Recurring Failure Classes (by observed frequency)

| Class | Count | Exemplars | One-line description |
|---|---:|---|---|
| SCOPE | 16 | #540, #557, #699 | Child/parent, old/new conversation, mode, or provider-session state affected another owner |
| STALE | 15 | #651, #684, #750 | Historical run IDs, fingerprints, bases, sessions, or snapshots trusted without re-proving current ownership |
| WRITER | 15 | #417, #588, #688 | Multiple observers/timers/components independently wrote the same visual state (13 scroll fixes → one FSM controller) |
| UI | 12 | #532, #647, #752 | Lifecycle/approval/completion inferred from local presentation state instead of durable backend authority |
| REC | 11 | #569, #682, #739 | Live completion worked but startup/stop/reconciliation/queued-continuation followed different rules |
| DRIFT | 7 | #573, #657, #763 | Memory vs SQLite, frontend vs backend, optimistic vs live copies diverged |
| EVENT | 3 | #568, #603, #715 | Event handler or background scan admitted unrelated/inactive/archived owners |
| EFFECT | 3 | #685, #701, #771 | Auto-merge/reset/dismissal/acknowledgement fired before the governing operation settled |
| ERROR | 3 | #672, #745, #766 | Missing settings or failed resolution looked usable instead of blocking explicitly |
| GENERAL | 3 | #550, #683, #737 | Capability wired through the main path but omitted sidecars/startup/recovery/MCP/another provider |
| FAIL | 2 | #531, #554 | Optional/missing state bypassed a safety gate |

## Pre-Merge Checks (each falsifiable)

| # | Class | Check |
|---|---|---|
| 1 | SCOPE | Switch among parent, child, fork, archived, and unrelated conversations: every read, event, action, and provider session still targets its explicit owner |
| 2 | FAIL | Inject missing rows and repo/query errors at every permissive gate: neither authorizes progress, publish, approval, or completion |
| 3 | STALE | Immediately before each transition/effect, re-resolve current run, attempt, artifact, fingerprint, branch/head, and enabled provider; reject every stale variant |
| 4 | UI | Reload/discard all frontend stores: lifecycle, approval, review, notification, and automation truth rehydrates from persisted backend state |
| 5 | REC | Run the same scenario through live completion, stop, retry, startup, reconciliation, and queued continuation: identical terminal classification and effects |
| 6 | DRIFT | For every duplicated representation, force one write/refresh to fail: copies converge or surface an error, never report success |
| 7 | EVENT | Deliver valid events for a different conversation/run/project and scan archived records: no visible or durable state changes |
| 8 | EFFECT | Fail the final CAS/transition/navigation step: assert absence of notifications, acknowledgement, auto-merge, publication, cleanup, terminal metadata |
| 9 | ERROR | Trace backend → IPC/HTTP/MCP → UI errors end-to-end: no layer converts failure into empty data, defaults, or success |
| 10 | GENERAL | Enumerate every spawn/send path and enabled harness: settings, model, effort, env, sandbox, service tier, and provider-disable gates preserved |
| 11 | WRITER | Identify each mutable UI concern (scroll, tab, drawer, badge, focus): exactly one component/controller may write it |
| 12 | ALL | Add behavioral regressions for stale, duplicate, re-entry, restart, and failure cases through production entry paths, asserting forbidden effects do NOT occur |
