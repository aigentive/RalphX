> **Maintainer note:** Keep this file compact. Prefer one-line rules, tables, and explicit non-negotiables over prose.

# Agent Workspace Review Modes

RalphX has two distinct review workflows. A local checkout exists in both, but it does not make their authority, artifacts, or side effects interchangeable.

## Contract

| | Workspace Review | Review PR |
|---|---|---|
| User surface | Review action/tab on an Agent Workspace | Start mode `Review PR` |
| Target | Local workspace delta or selected local source against its base | One linked remote GitHub pull request at its current head, inspected through the agent-workspace checkout |
| Source of truth | Backend-injected review target, diff fingerprint, applicable head, and review-run authority | Linked PR identity plus live GitHub head/lifecycle state; the checkout is the inspection substrate |
| Agent | `ralphx-workspace-reviewer` | `ralphx-pr-reviewer` |
| Durable artifact | Versioned local Overview + Requested Changes artifact pair and Overview hunk annotations | Versioned PR Review artifact for the reviewed GitHub head |
| Side effects | Completes the local publish/review gate; may route a local fixer | May propose Request Changes, Approve, or Comment; GitHub submission requires explicit user approval |
| Freshness | Scope + diff fingerprint + applicable head | Exact PR number + current remote head SHA |
| Pause | Not applicable | Pauses new-head re-review dispatch only; remote PR lifecycle monitoring continues |
| Terminal | Passed/blocking/failed are gate outcomes, not GitHub PR lifecycle | Remote `merged` or `closed` atomically terminalizes workspace publication, monitor, pending/submitting actions, and the deduped lifecycle event |

## Non-Conflation Rules

- Local Workspace Review eligibility is `Edit | Ideation` only; PLAN and Review PR suppress all local review reads/actions, while PLAN may perform idempotent cleanup of authority that was already live before the mode transition.
- Entering PLAN must quiesce reviewer/fixer runtime state and cancel any review-owned auto-merge guard before persisting the mode; cleanup preserves history, leaves auto-merge disabled, and cannot consume reviewer output or authorize publication.
- Never describe Workspace Review as reviewing or approving a GitHub PR; it is a local quality/publish gate and has no GitHub review-action tool.
- Workspace-delta Review authority requires a settled index with no unfinished merge/rebase; block with completion-or-abort guidance and recompute target/fingerprint/receipt after settlement.
- Never describe Review PR as merely reviewing local branch changes; its authority is the linked remote PR identity, head, and lifecycle.
- Shared concepts are limited to read-only inspection, local checkout access, versioned review artifacts, and actionable findings; do not share state machines, action tools, freshness rules, or fixer behavior by analogy.
- Review PR mutations fail closed when live PR health cannot be confirmed. Late proposals/submissions use repository guards/CAS and cannot resurrect actions after terminal settlement.
- Durable state is authoritative. UI projections suppress stale action controls when either workspace publication or monitor state is terminal and keep polling every nonterminal Review PR context.
- Workspace Review reviewer/repair confirmations use the exact per-conversation role runtime override for provider, model, effort, and speed; approval and sandbox remain backend-resolved role defaults, while the backend alone owns target receipts and repair attempt identity.
- Review findings carry a disposition (Blocking | Fold In | Backlog | Informational) and a stated cost of doing nothing; Blocking and Fold In are both requested work and both drive outcome `blocking`, while Backlog and Informational never affect the gate. Fold In demotes to Backlog once an automated fixer has already run against the delta, so the review → fix → review loop terminates.

## Ownership And Debugging

| Concern | Workspace Review owner | Review PR owner |
|---|---|---|
| Agent contract | `agents/ralphx-workspace-reviewer/` | `agents/ralphx-pr-reviewer/` |
| Application/runtime | `src-tauri/src/application/agent_workspace_review*.rs` | `src-tauri/src/application/services/pr_merge_poller.rs`, `pr_startup_recovery.rs`, `agent_workspace_terminal_cleanup.rs` |
| HTTP/tool transitions | `src-tauri/src/http_server/handlers/agent_workspaces/workspace_review_context.rs` + workspace-review handlers in `mod.rs` | `src-tauri/src/http_server/handlers/agent_workspaces/pr_review/` |
| Persistence | Workspace-review monitor/artifact repositories | `AgentConversationWorkspaceRepository` PR monitor/action/terminal-settlement methods; SQLite and memory implementations |
| Frontend | Workspace Review artifact/gate surfaces | `AgentWorkspacePrReviewCard.tsx`, presentation helper, sidebar publication polling |
| Focused tests | `agent_workspace_review*_tests.rs`, handler review suites, agent catalog tests | `pr_merge_poller_tests.rs`, startup/repository PR-review tests, PR card/presentation/sidebar polling tests, agent catalog tests |

## Change Checklist

1. Name the workflow exactly (`Workspace Review` or `Review PR`) in code, tests, prompts, and UI copy.
2. Extend only the owning row above; changing a model-facing tool also requires canonical metadata, runtime authorization/registration, prompt, and tests per `agent-mcp-tools.md`.
3. For Review PR lifecycle changes, prove exact conversation+PR scoping, idempotent terminal settlement, stale-attempt rejection, live/startup recovery parity, and absence of post-terminal controls or notifications.
4. For Workspace Review gate changes, prove exact Overview + Requested Changes pair authority, scope/fingerprint/head freshness, legacy overview-only fail-closed behavior, and publish/fixer handoff; do not reuse Review PR lifecycle state.
