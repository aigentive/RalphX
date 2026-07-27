# RalphX Notification System — Phased Implementation Plan

> Status: PLANNED (not implemented). Drafted 2026-07-09 from a full-codebase HITL audit on `feat/automation-v2`.
> Adversarial critique round 1 completed 2026-07-09 (code-verified): architecture confirmed sound; 9 gaps found and folded in below. Per CLAUDE.md, run full Adversarial Plan Convergence again at implementation time.

## 1. Goals

1. **In-app notifications**: generalize the existing navbar reviews icon + badge + drawer (`AppTopBar.tsx:580-613` → `ReviewsPanel.tsx`) into a notification center covering every human-in-the-loop (HITL) and alert scenario, plus actionable toasts while the app is focused.
2. **Desktop notifications**: native macOS notifications (via `tauri-plugin-notification`, currently NOT installed) for users not actively viewing the app, with settings, per-category control, and focus-aware suppression.

Non-goals (v1): deep-linking from the OS notification into a specific view (macOS/tauri-plugin click payloads are unreliable — click focuses the app, the in-app center takes over), notification sounds beyond the OS default, cross-device push.

## 2. Complete trigger inventory (audited, file:line verified)

### A. Task pipeline — states waiting on a human (from the 25-state machine)

| # | Trigger | Signal | Set at | Event today | UI today |
|---|---|---|---|---|---|
| A1 | AI review approved, human confirm needed | `review_passed` | `on_enter_states/review.rs:343-412` | `review:ai_approved` | ReviewsPanel + HumanReviewTaskDetail |
| A2 | AI review escalated | `escalated` | `review.rs:414-431` | `review:escalated` | ReviewsPanel + EscalatedTaskDetail |
| A3 | QA failed | `qa_failed` | `on_enter_states/qa.rs:73-92` | `qa_failed` | BasicTaskDetail |
| A4 | Merge conflict (agent-reported or auto-detected) | `merge_conflict` | `handlers/git.rs:654-672`, `merge_outcome_handler.rs:691` | `merge:conflict` | MergeConflictTaskDetail |
| A5 | Merge incomplete | `merge_incomplete` | `git.rs:804-822` | `merge:incomplete` | MergeIncompleteTaskDetail |
| A6 | Task blocked on human input | `blocked` + `Blocker::human_input` (`state_machine/types.rs:29-40`) | transition table | `task:status_changed` | BasicTaskDetail |
| A7 | Task failed (terminal, needs Retry decision) | `failed` | `on_enter_states/outcomes.rs:165-175` | `task_failed` (no `Notifier` call — gap) | BasicTaskDetail |
| A8 | Waiting on external PR | `waiting_on_pr` | `entities/status.rs:50-51` | poller-driven | — |
| A9 | Provider-wide pause (usage limit) | `task:provider_error_paused` | `chat_service_handlers.rs:2826-2870` | emitted | weak |
| A10 | Recovery prompt (restart/cancel after app restart) | `recovery:prompt` | `reconciliation/events.rs:343-389` | emitted | modal via `useEvents.recovery.ts` |
| A11 | **Task silently stuck** | `task:recovery_failed` (`chat_service_handlers.rs:2983-2994`), `task:on_enter_error` (`transition_handler/mod.rs:291-303`) | emitted | **ZERO frontend listeners — silent today** |

### B. Interactive agent HITL (agent process is blocked, time-sensitive)

| # | Trigger | Signal | Notes |
|---|---|---|---|
| B1 | Permission request | `permission:request` / `permission:expired` (`handlers/permissions.rs:12-75`) | Agent blocked ≤5 min; global modal `PermissionDialog.tsx`; pending set hydratable via `getPendingPermissions` |
| B2 | Agent question | `agent:ask_user_question` / `agent:question_resolved` (`handlers/questions.rs:12-205`) | Agent blocked; inline `QuestionInputBanner`; only visible if that conversation is open |
| B3 | Plan-mode proposal | same transport, `metadata.kind = "plan_mode_proposal"` (`question-handler.ts:410`) | rides B2 |
| B4 | Workspace/ideation plan needs approval | client-derived `needs_approval` (`AgentsArtifactPane.tsx:2260`); backend signals: `plan_artifact:created` … `plan_artifact:approved` | No dedicated backend "awaiting approval" event today |
| B5 | Agent finished turn, waiting for user | `agent:turn_completed` (`chat_service_streaming.rs:2088-2107`) → `waiting_for_input` | The generic "your turn" signal; high volume — desktop-only category |

### C. Automation v2

| # | Trigger | Signal | Notes |
|---|---|---|---|
| C1 | Run parked awaiting plan approval | `AutomationRunStatus::AwaitingPlanApproval` (`entities/automation.rs:99-110`); every transition emits `automation:run:updated` via `AutomationTransitionService` (`transition.rs:166-174`) | Deep link exists: `requestAutomationRunOpen` → Plan tab |
| C2 | Automation paused, actionable reasons | `AutomationStatus::Paused` + `paused_reason_code` ∈ `judge_failed`, `plan_judge_failed`, `plan_revision_exhausted`, `workspace_review_blocked`, `max_runs_exhausted`, `max_consecutive_failures` | `paused_reason_code = "user"` excluded (self-inflicted) |
| C3 | Run failed | `AgentFailed` + `error_code` (`timeout`, `no_changes`, `publish_failed`, `agent_failed`, `plan_not_submitted`, `plan_reminder_failed`, `plan_resume_failed`) | labels exist in `automationRunView.ts:9-14` |
| C4 | Run reached terminal success | `Completed` / `Merged` (`PrClosed` = warning) | informational |

### D. Git / GitHub / publish

| # | Trigger | Signal | Notes |
|---|---|---|---|
| D1 | gh interactive login needed (device code) | `gh-auth:login_prompt` (`project_commands.rs:28,1009-1027`) | urgent + short-lived code |
| D2 | Startup git-auth preflight found blocked projects | `git-auth:startup_preflight` (`startup_git_auth_preflight.rs:24,237`) | **ZERO frontend listeners** — frontend re-derives via polling in `useGitAuthStartupNotification.ts` |
| D3 | PR-reviewer proposes an action, awaiting user | `AgentWorkspacePrReviewMonitorStatus::AwaitingUser` (`agent_workspaces.rs:2028` etc.) + `pr_review_artifact:*` events | |

### E. Informational (nice-to-notify, never badge-actionable)

Task merged (`task:merged`), execution completed (`execution:completed`), session auto-recovered (`agent:session_recovered`), app update available (existing `UpdateChecker`).

### Existing scaffolding worth knowing

- `Notifier` trait (`domain/state_machine/services.rs:90-105`) — only impl is `LoggingNotifier` (`task_transition_service.rs:447-468`) which just `tracing::info!`s. Exactly 4 call sites (`qa_failed`, `review_error`, `review:ai_approved`, `review:escalated`). `QaFailedData.notified`/`FailedData.notified` + `mark_notified()` are dead scaffolding (never called).
- No notifications table, no `tauri-plugin-notification`, no dock-badge code.
- Reviews badge today: `useTasksAwaitingReview` (TanStack, 30s staleTime) invalidated by `review:update` via `useReviewEvents` in `EventProvider.tsx`. Badge count = tasks in `pending_review|reviewing|review_passed|escalated`.

## 3. Architecture

**Two-tier model** — this is the load-bearing design decision:

### Tier 1 — Live "Needs your action" list (derived, self-healing, drives the badge)

A backend aggregation command `list_attention_items` composes the authoritative pending states into one typed list. Because items are *derived from state*, they can never go stale: when a task leaves `review_passed`, the item disappears on the next invalidation. No resolution bookkeeping, no fail-open resolution bugs.

Sources (each already queryable):
- Tasks in `review_passed | escalated | qa_failed | merge_conflict | merge_incomplete | failed | blocked(human_input)` — via `task_repo.list_paginated` with status filter (same pattern as `get_tasks_awaiting_review`, `task_commands/query.rs:322-358`). **Explicit semantic change vs the old badge**: `pending_review | reviewing` (in-progress AI reviews, not human-actionable) leave the badge count; failure states join it. The panel's Reviews tab still lists in-progress reviews for visibility, excluded from the badge count. This is a deliberate product decision — update badge tests accordingly, don't let it happen as a side effect.
- Pending permission requests — `permission_state` (verified: single Arc shared into HTTP AppState in `runtime_wiring.rs:182-200`; Tauri commands already read it, `permission_commands.rs:70-75`). `PendingPermissionInfo` has no `project_id`/`created_at` — the aggregator resolves `context_id`/`task_id` → conversation/task → `project_id` server-side; unresolvable items get `project_id: None` = global, always shown under any project filter. Add `created_at` capture to `PendingPermissionInfo`/`PendingQuestionInfo` in PR 1.
- Pending agent questions — `question_state` (same sharing; `session_id` → conversation → project resolution as above)
- Automation runs in `AwaitingPlanApproval`; automations `Paused` with actionable `paused_reason_code`
- Workspace plan artifacts awaiting approval — active planning sessions with a `plan_artifact_id` set and no matching `plan_artifact_approvals` row, **excluding** conversations with `automation_run_id` set (automation-owned plan drafting is covered by C1 and must not double-count or alert during Automatic-mode judge approval) and **excluding** sessions with linked implementation tasks (mirrors the client's `!hasImplementationAttempt` condition, `AgentsArtifactPane.tsx:781,2260-2270`). Note: there is no per-workspace "manual approval mode" datum (`plan_approval_mode` exists only on automations) — do not filter on it for workspaces.
- PR-review monitors in `AwaitingUser`

```rust
// DTO (serde camelCase; Zod mirror in frontend/src/types/notifications.ts)
pub struct AttentionItem {
    pub id: String,              // stable synthetic key, e.g. "task:{id}:review"
    pub category: NotificationCategory,
    pub title: String,
    pub detail: Option<String>,
    pub project_id: Option<String>,
    pub created_at: Option<String>, // when the state was entered, if known
    pub target: NotificationTarget, // typed navigation payload, see below
}
```

**Badge count = attention items count.** The reviews badge is subsumed: review items are a category inside the same list, so there is no double counting and no cutover risk.

Refresh: TanStack Query keyed `attentionKeys.list(projectId?)`, invalidated by the events that already fire for each source (`review:update`, `task:status_changed`, `permission:request`/`permission:expired`, `agent:ask_user_question`/`agent:question_resolved`, `automation:updated`, `automation:run:updated`, `plan_artifact:created`/`approved`, `pr_review_artifact:*`). One new hook `useAttentionEvents` registered in `EventProvider.tsx` `GlobalEventListeners`.

### Tier 2 — Durable notification log (history + unread + desktop dispatch source)

A `notifications` table records point-in-time alerts. Log rows are history — they are never "resolved", only read/unread. Liveness questions belong to Tier 1.

```sql
CREATE TABLE notifications (
  id          TEXT PRIMARY KEY,
  created_at  TEXT NOT NULL,
  project_id  TEXT,
  category    TEXT NOT NULL,
  severity    TEXT NOT NULL,   -- 'action_required' | 'warning' | 'info'
  title       TEXT NOT NULL,
  body        TEXT,
  target_json TEXT,
  dedupe_key  TEXT UNIQUE,     -- ON CONFLICT DO NOTHING (never re-alert on re-entry)
  read_at     TEXT
);
CREATE INDEX idx_notifications_unread ON notifications(created_at) WHERE read_at IS NULL;
```

`NotificationCategory` (single Rust enum, serialized snake_case, mirrored in TS):
`review_needed, review_escalated, qa_failed, merge_conflict, merge_incomplete, task_failed, task_blocked, task_stuck, provider_paused, recovery_prompt, permission_request, agent_question, plan_approval, automation_plan_approval, automation_paused, automation_run_failed, automation_run_completed, agent_waiting, gh_auth, git_auth_preflight, pr_review_action, info`

`NotificationTarget` (typed, versioned JSON): `{ kind: "task" | "agent_conversation" | "automation_run" | "project" | "none", projectId?, taskId?, conversationId?, setupConversationId?, automationId?, runId? }`. Frontend maps task targets through `openTaskInAgents(taskId, mode, hints)`, linked ideation setup targets through `openIdeationInAgents(setupConversationId)`, and automation targets through `requestAutomationRunOpen` (`automationRunNavigation.ts:210`). Project-linked conversations use the current Agents conversation/plan navigation.

**`NotificationService`** (application layer):
- `record(NewNotification)` — insert with dedupe, emit `notification:created` Tauri event, then desktop-dispatch (Phase 3). **Fire-and-forget**: all errors logged, never propagated — recording must never block or fail a state transition.
- `record_ephemeral(...)` — desktop-dispatch only, no DB row (used for high-volume categories like `agent_waiting`).
- Called explicitly at producer sites AFTER the transition/authority commits — the same places that emit Tauri events today (state-machine on_enter handlers, `AutomationTransitionService`, HTTP handlers). Never before authority (stateful-workflow rule: authority before effects).
- Dedupe keys are attempt/instance-scoped so reconciler re-entry never re-alerts: `task:{id}:review:{status_entry_id}`, `perm:{request_id}`, `question:{request_id}`, `run:{run_id}:plan_approval`, `automation:{id}:paused:{reason}:{producer_timestamp}` (there is no `paused_at` column; producers run only on CAS `changed=true`, once per real transition, so a producer-generated timestamp or `updated_at` is safe).
- **Producer context rule (critique Gap 1):** the `Notifier` trait carries only `(type, task_id[, message])` — NOT enough to build category/severity/target/`project_id`/attempt-scoped dedupe key, and re-querying "latest `task_state_history` row" from inside a Notifier impl races the history insert for the current transition (revision loop `review_passed → revision_needed → review_passed` can compute the previous pass's entry id → silent dedupe swallow or re-alert). Therefore task-pipeline producers live at the **transition-handler level**, receiving a `NotificationContext { task, history_entry_id, project_id }` from the handler that just wrote the history row (either extend the `Notifier` signature or add a parallel seam). `task_state_history.id` exists (`migrations/v1_initial_schema.rs:185-194`). The 4 legacy `Notifier` call sites are migrated to this seam, not wrapped "for free"; `LoggingNotifier` is then deleted and the missing `enter_failed_state` producer added.

### Tier 3 — Desktop dispatch (Phase 3)

Dispatch lives **backend-side inside `NotificationService`** (single choke point; works even when the webview is busy; Rust API of `tauri-plugin-notification`).

- `WindowFocusState` (AtomicBool in managed state) updated from the existing `on_window_event` handler (`runtime_wiring.rs:68-91`, `WindowEvent::Focused(bool)`).
- Gate order: master setting on → category setting on → (app unfocused OR category marked always) → coalesce (≥3 dispatches within 5s collapse into one "N items need your attention" summary) → send.
- Behind a `DesktopNotifier` trait with `Noop` impl for tests and no-AppHandle contexts (mirror the `NoopAutomationEventEmitter` pattern; wire through the same seam that `automation_event_emitter_for_state` consolidated, `api.rs:125-128` — both HTTP-server and Tauri-command paths must reach the real impl).
- OS notification click focuses the app (plugin/macOS default). No deep link in v1.

### In-app surfaces

> Full UI/UX spec with ASCII mockups (badge states, drawer layout, row anatomy, toasts, macOS notification copy, settings panel, category→icon/action/target mapping): `docs/handoffs/notification-system-ui-spec.md`. PRs 2, 7, 9, 10 implement against it.

- **Navbar**: the reviews button slot becomes the notification center trigger (keep ⌘⇧R, keep the `reviews-toggle`/`reviews-badge` testid contract or migrate tests in the same PR). Icon: `Inbox` (lucide). Badge = attention count, unread-history dot as secondary accent.
- **Drawer**: `NotificationCenterPanel` evolves `ReviewsPanel.tsx` — section 1 "Needs action" (Tier 1 items, grouped by category; review items keep the existing `TaskReviewCard` + `ReviewDetailModal` in-place flow), section 2 "Recent" (Tier 2 history, mark-read on view, "Mark all read"). Per `frontend-interaction-performance.md`: the panel shell paints synchronously on click; data hydrates after a paint boundary; heavy children lazy.
- **Toasts**: while the app IS focused, a new `action_required` notification raises a sonner toast with an action button that runs the target navigation (pattern: `useFreshnessBlockedNotification.ts:65-84`). Setting-gated. Info-severity never toasts.

## 4. Phases and PRs

Branch note: automation producers (PR 6) depend on `feat/automation-v2`; everything else can land against `main`. Suggested base: land Phases 1-2 after automation-v2 merges, or base the series on it.

### Phase 1 — Attention aggregation + navbar generalization (in-app, no DB)

**PR 1 — `feat: attention items backend aggregation`**
- `NotificationCategory` + `NotificationTarget` + `AttentionItem` in `ralphx-domain` (serde camelCase on DTOs).
- `list_attention_items` Tauri command (`commands/notification_commands.rs`, registered in `registry.rs`) aggregating the Tier 1 sources above. Task-status source reuses the `list_paginated` status-filter pattern; permission/question sources read the shared in-memory states (verify Dual-AppState sharing covers the Tauri command path).
- Tests: per-source inclusion/exclusion (e.g., `paused_reason_code="user"` excluded; `Blocker::human_input` vs dependency blocker), empty states, project scoping. `scripts/test-rust-fast.sh pr` green.

**PR 2 — `feat: notification center panel + badge`**
- `frontend/src/types/notifications.ts` (Zod mirrors), `api/notifications.ts`, `hooks/useAttentionItems.ts`, `hooks/useNotificationEvents.ts` (event → invalidation, registered in `EventProvider.tsx`).
- `NotificationCenterPanel` from `ReviewsPanel` (reviews category keeps `TaskReviewCard` + `ReviewDetailModal` behavior; new generic `AttentionItemRow` for other categories with target navigation). `AppTopBar` badge switches from `pendingReviewCount` to attention count; ⌘⇧R preserved.
- Category → navigation wiring (`openTaskInAgents`, `openIdeationInAgents`, current Agents conversation/plan navigation, `requestAutomationRunOpen`).
- Tests (Vitest): badge count derivation (asserting the new semantics: in-progress AI reviews excluded, failure states included), panel renders per category, navigation dispatch per target kind, **first-paint synchronous shell test** (perf rule TDD), existing ReviewsPanel test migration.
- Test-surface migration is larger than Vitest: Playwright page objects/fixtures consume the reviews testids — `frontend/tests/pages/modals/reviews-panel.page.ts`, `tests/pages/kanban.page.ts`, `tests/fixtures/setup.fixtures.ts`, `tests/helpers/review-detail.helpers.ts` — plus `uiStore.test.ts` (`reviewsPanelOpen`), `App.test.tsx`, `App.navigation.test.tsx`. Migrate all in this PR.
- WKWebView rules apply to any new themed surface (explicit bg/border longhands, no chained `var()` on canvas paint).

### Phase 2 — Durable log + producers

**PR 3 — `feat: notifications persistence + service`**
- Migration (via `scripts/new_sqlite_migration.py`), `Notification` entity, repo trait + sqlite/memory impls (`DbConnection` `db.run(|conn| …)` rule), `NotificationService` (record/dedupe/ephemeral hook point), `notification:created` / `notification:updated` events, commands: `list_notifications` (paginated), `mark_notification_read`, `mark_all_notifications_read`, unread count. Retention: prune read rows > 30 days / cap 1000 in a startup job.
- Tests: dedupe on conflict, fire-and-forget error swallowing (repo error must NOT propagate to caller), read-state transitions, prune.

**PR 4 — `feat: task pipeline notification producers`**
- Transition-handler-level producer seam per the producer context rule (§3): handlers pass `NotificationContext` (task, freshly written `task_state_history.id`, project_id) — no latest-row re-query. Migrate the 4 legacy `Notifier` call sites (qa.rs:89, review.rs:47/379/425) onto it; delete `LoggingNotifier` (`task_transition_service.rs:1253`) → covers A1, A2, A3 (+ `review_error`).
- Add producer calls: A4/A5 (merge handlers in `git.rs` + `merge_outcome_handler.rs` — audit ALL paths into `MergeConflict`), A6, A7 (fix the missing-notify gap in `enter_failed_state`), A9, A10, A11 (**closes the silent-stuck-task gap**: `task:recovery_failed`, `task:on_enter_error` producers).
- Attempt-scoped dedupe keys throughout; producers run post-transition only.
- Tests: production-entry-path tests per producer incl. re-entry/duplicate scenarios asserting exactly-one notification (stateful-workflow test-falsification rule).

**PR 5 — `feat: interactive HITL producers`**
- B1 `permission_request` (record at `permissions.rs:12-48`; severity `action_required`), B2/B3 questions (`questions.rs:12-65`; plan-mode proposal keeps `metadata.kind`), B4 plan-awaiting-approval (record on `plan_artifact:created` for **non-automation** planning conversations only — same `automation_run_id` exclusion as the Tier 1 source; Tier 1 governs liveness, so no resolution needed when approved).
- Tests incl. hydration race (notification recorded even if no frontend listener mounted).
- B5 `agent_waiting` moves to PR 10 (it is ephemeral/desktop-only, and desktop dispatch + focus state don't exist until Phase 3 — landing it here would be an untestable no-op).

**PR 6 — `feat: automation + git notification producers`** *(depends on automation-v2)*
- C1/C2/C3/C4 recorded inside `AutomationTransitionService` methods (verified single choke point — no CAS callers outside `transition.rs`), post-CAS and only when `changed=true`: entering `AwaitingPlanApproval`, entering `Paused` with actionable reason (map reason → copy), entering `AgentFailed` (reuse `ERROR_CODE_LABELS` semantics server-side), entering `Merged`/`Completed` (info) / `PrClosed` (warning).
- Seam work this PR owns (not free): the shared `emit_run_updated` point (`transition.rs:166-174`) doesn't carry to-status/error_code, so producers are added per `transition_*` method (or the emit seam is widened to carry them); `NotificationService` is threaded into the `AutomationTransitionService` constructor (`transition.rs:146-156`) at every construction site (`api.rs`, scheduler wiring); `project_id` for the target needs an `automation_repo.get_by_id` lookup.
- D1 `gh-auth:login_prompt` (include device code in body), D2 startup preflight (record summary notification — **closes the zero-listener gap**), D3 `AwaitingUser` PR-review monitors.
- Tests: transition-service-level (every entry path to each status produces exactly one notification; approval-delivery re-entry via arm 0 CAS does not re-alert).

**PR 7 — `feat: notification history UI + toasts`**
- "Recent" section in `NotificationCenterPanel` (lazy-hydrated), unread accent on badge, mark-read on view + "Mark all read".
- Focused-state toast bridge: `useNotificationToasts` hook (subscribes `notification:created`, severity `action_required` only, action button navigates). Registered in `EventProvider`.
- Tests: unread lifecycle, toast fires only when focused-toasts setting on, dedupe with drawer-open state (no toast while panel open).

### Phase 3 — Desktop notifications

**PR 8 — `chore: tauri-plugin-notification install`**
- `src-tauri/Cargo.toml` + `.plugin(tauri_plugin_notification::init())` in `lib.rs:165-176` builder chain + `"notification:default"` in `capabilities/default.json` + `@tauri-apps/plugin-notification` in `frontend/package.json` (existing package manager/lockfile) + web-mode mock `frontend/src/mocks/tauri-plugin-notification.ts` + Vite alias (match existing mocks per `api-layer.md`).
- Smoke: permission prompt + test notification behind a dev-only command. Verify in `npm run tauri dev` (not web).

**PR 9 — `feat: notification settings`**
- Follow the Review Settings pattern end-to-end (`useReviewSettings.ts:8-75`, `review_commands.rs:988-1042`): migration → `NotificationSettings` domain + repo (sqlite/memory) → `get_/update_notification_settings` commands → `useNotificationSettings.ts` → `NotificationSettingsPanel.tsx` registered in `settings-registry.ts`.
- Fields: `desktop_enabled` (default true), `desktop_only_when_unfocused` (default true), `focused_toasts_enabled` (default true), per-category desktop toggles (`agent_waiting` default ON — it's the headline "your turn" ping; `automation_run_completed`/info default OFF).

**PR 10 — `feat: desktop dispatch + agent_waiting`**
- `WindowFocusState` managed state + update in `runtime_wiring.rs` `WindowEvent::Focused` arm; `DesktopNotifier` trait (real impl using plugin Rust API; `Noop` for tests/no-handle) wired into `NotificationService.record`/`record_ephemeral` after DB write + event emit. Verified: the HTTP AppState is built with the real `AppHandle` (`runtime_wiring.rs:177-197`), so Axum-side producers dispatch too.
- B5 `agent_waiting` producer lands here: `record_ephemeral` at the `turn_completed` emission site (`chat_service_streaming.rs:2094-2108`) with a **mandatory ownership filter** — suppress for conversations with `automation_run_id` set (automation runs complete turns continuously; `agent_workspace_auto_publish.rs:121` listens to this same event) and for child/background sessions; only user-attended interactive conversations ping. Plus focus suppression.
- Settings + focus gate + 5s/3-item coalescing summary.
- Tests: mock `DesktopNotifier` asserting gate matrix (enabled × focused × category × severity), coalescing, `agent_waiting` suppression for automation-run and background conversations, and that dispatch failure never fails `record`.

### Phase 4 — Polish (optional, cut-line)

**PR 11 — `feat: dock badge count`** — `NSDockTile.badgeLabel` via existing `objc2-app-kit` dep, synced to attention count (cleared on zero); macOS-gated.
**PR 12 — `chore: toast policy alignment + retention tuning`** — route the bespoke notification-ish hooks (`useGitAuthStartupNotification`, `useFreshnessBlockedNotification`, `ChildSessionNotification`, `ProactiveSyncNotification`) through the center where they represent durable facts; per-project mute; retention knobs.

## 5. Invariants & risk register

| Risk | Mitigation |
|---|---|
| Notification effects corrupting workflow authority | `record()` fire-and-forget, called only post-transition-commit, never awaited into transition results (stateful-workflow: authority before effects) |
| Re-entry / reconciler loops re-alerting | instance-scoped `dedupe_key` UNIQUE + ON CONFLICT DO NOTHING; adversarial tests per producer for stale/duplicate/re-entry (esp. automation plan-gate arm-0 CAS re-entry) |
| Badge lies (stale count) | badge = Tier 1 derived state, never log rows; invalidation driven by existing events + 30s staleTime safety net |
| Double counting reviews during cutover | review category lives inside Tier 1 list; old `pendingReviewCount` prop deleted in PR 2, not run in parallel |
| Desktop spam | ephemeral tier for high-volume `agent_waiting`; ownership filter (no pings for automation-run/background conversations); coalescing; per-category settings; unfocused-only default |
| `permission_state`/`question_state` visibility | VERIFIED: single Arcs shared into HTTP AppState (`runtime_wiring.rs:182-200`), Tauri commands already read them (`permission_commands.rs:70-75`); still test both server contexts |
| Notifier-impl history-row race | producers receive the freshly written `task_state_history.id` from the transition handler; never re-query "latest row" inside a notifier (revision-loop re-entry would collide dedupe keys) |
| B4 over-matching automation plan drafting | attention source excludes `automation_run_id` conversations and sessions with implementation tasks; C1 owns automation plan approvals |
| WKWebView rendering of new drawer | longhand bg/border, no chained `var()`, verify in `npm run tauri dev` |
| Panel jank | shell-first paint, lazy history hydration, TDD perf tests (perf rule is NON-NEGOTIABLE) |
| HTTP-server producers lacking AppHandle | `DesktopNotifier`/event emitter resolved through the shared seam (mirror `automation_event_emitter_for_state`); Noop fallback logs loudly |

## 6. Open product decisions (need owner input before Phase 3)

1. `agent_waiting` ("agent finished, your turn") — desktop default ON per plan; confirm.
2. Should automation run success (`Merged`) desktop-notify by default? Plan says OFF (info tier).
3. Icon swap `GitPullRequest` → `Inbox` on the navbar button — confirm.
4. Toast position/duration for action toasts (currently bottom-left global sonner).
