# Notification System — UI/UX Design Spec

> Companion to `docs/handoffs/notification-system.md` (phases/PRs referenced below).
> Visual language: `specs/design/styleguide.md` (v27 productivity chrome) — flat surfaces, `--bg-surface` panels, `--border-subtle` dividers, **orange is the only accent** (`--accent-primary #FF6A35`), status colors for badges/alerts only. All mockups are dark theme; tokens carry both themes.

---

## 1. Navbar trigger + badge (PR 2)

Lives in the existing reviews-button slot in `AppTopBar` (48px topbar, right cluster next to command search). Icon: `Inbox` (lucide). Keyboard: **⌘⇧R** (unchanged). Icon-only button rule applies: `aria-label` + app `Tooltip` ("Notifications ⌘⇧R").

### Badge states

```
 state A: nothing pending, no unread          state B: 3 attention items
 ┌──────┐                                     ┌─────────┐
 │  ⊡   │                                     │  ⊡  (3) │   pill: bg --accent-primary,
 └──────┘                                     └─────────┘   text #fff, 10px, "9+" cap
                                                            (reuses reviews-badge spec)

 state C: no attention items, unread history exists
 ┌──────┐
 │  ⊡ • │    4px dot, --accent-primary — secondary cue only, never a number
 └──────┘
```

**Badge number = Tier 1 attention count only** (human-actionable, live-derived). History unread never inflates the number — it only earns the dot. Severity does not change badge color (orange always); red is reserved for row-level status icons.

### Topbar in context

```
┌────────────────────────────────────────────────────────────────────────────┐
│ ●●●  acme-app ▾            ┌ ⌘K Search… (380px) ┐   ⊡ (3)   Aa ▾   ◐ ▾    │  48px
└────────────────────────────────────────────────────────────────┬───────────┘
                                                                 └ notification
                                                                   center trigger
```

---

## 2. Notification center drawer (PR 2 shell + needs-action; PR 7 history)

Same chrome as today's reviews sidebar (styleguide §"Right reviews sidebar"): fixed right panel, **400px**, `--bg-surface`, `border-left 1px --border-subtle`, no radius/shadow/floating margin. Opens instantly (shell paints synchronously per `frontend-interaction-performance.md`; rows hydrate after a paint boundary — skeleton rows in the interim).

```
◄──────────────── app content ────────────────►│◄─────────── 400px ───────────►
                                                ┌───────────────────────────────┐
                                                │ Notifications        ⋯    ✕   │ ← px-4 py-3,
                                                │┌─────────────────┬───────────┐│   border-b subtle
                                                ││ Needs action (5)│ History • ││ ← Tabs (h-9, --bg-
                                                │└─────────────────┴───────────┘│   surface, active
                                                │                               │   --bg-elevated)
                                                │ REVIEWS · 2                   │ ← group eyebrow:
                                                │┌─────────────────────────────┐│   11px/600/upper/
                                                ││ ◆ Add rate limiting to API  ││   tracking .08em,
                                                ││   AI approved — confirm to  ││   secondary/60
                                                ││   merge                     ││
                                                ││   12m · acme-app   [Review] ││ ← TaskReviewCard,
                                                │└─────────────────────────────┘│   unchanged flow →
                                                │┌─────────────────────────────┐│   ReviewDetailModal
                                                ││ ▲ Migrate auth middleware   ││   opens in place
                                                ││   Escalated — AI could not  ││
                                                ││   decide                    ││
                                                ││   1h · acme-app    [Decide] ││
                                                │└─────────────────────────────┘│
                                                │                               │
                                                │ AGENT REQUESTS · 2            │
                                                │┌─────────────────────────────┐│
                                                ││ ⛨ Permission: Bash          ││ ← icon --status-
                                                ││   worker on “OAuth flow”    ││   warning; agent is
                                                ││   wants to run `git push`   ││   BLOCKED — expires
                                                ││   ⏳ expires in 3m [Respond] ││   countdown shown
                                                │└─────────────────────────────┘│
                                                │┌─────────────────────────────┐│
                                                ││ ? Question from ideation    ││
                                                ││   “Should retries use expo- ││
                                                ││   nential backoff?”         ││
                                                ││   4m · acme-app    [Answer] ││
                                                │└─────────────────────────────┘│
                                                │                               │
                                                │ AUTOMATIONS · 1               │
                                                │┌─────────────────────────────┐│
                                                ││ ▣ Nightly refactor — run #14││
                                                ││   Plan awaiting approval    ││
                                                ││   22m · acme-app   [Review  ││
                                                ││                      plan]  ││
                                                │└─────────────────────────────┘│
                                                │                               │
                                                │            · · ·              │
                                                └───────────────────────────────┘
```

### Row anatomy (generic `AttentionItemRow`)

```
┌────────────────────────────────────────────────┐
│ <icon>  <title — 1 line, truncate>             │  icon: 16px, status color
│         <detail — max 2 lines, --text-muted>   │  by severity (see §6)
│         <time> · <project>        [<action>]   │  meta: 12px --text-muted;
└────────────────────────────────────────────────┘  action: sm ghost button
  card: --bg-elevated, 1px --border-subtle, rounded-md (8px), p-3, gap-2
  hover: border --border-strong + bg-hover/30 · whole row clickable = action
  focus-visible: ring-1 --accent-primary
```

- Review-category rows keep the existing `TaskReviewCard` body and open `ReviewDetailModal` **in place** (no view hop) — behavior parity with today's ReviewsPanel.
- Permission rows show a live countdown (requests expire at 5 min). `[Respond]` re-raises the global `PermissionDialog` (requests are queued globally; the row is a second door to the same dialog, not a new surface).
- All other rows navigate on click (see mapping §6) and close the drawer.
- Group eyebrows render only for non-empty groups; groups ordered by urgency: **Agent requests → Reviews → Tasks → Automations → Git**. (Agent requests first: a blocked agent burns wall-clock.)

### History tab (PR 7)

Chronological Tier 2 log. Read/unread only — rows never claim liveness ("Permission requested" stays after it was denied; that's history, not a to-do).

```
                                                ┌───────────────────────────────┐
                                                │ Notifications        ⋯    ✕   │
                                                │┌─────────────────┬───────────┐│
                                                ││ Needs action (5)│ History • ││
                                                │└─────────────────┴───────────┘│
                                                │            Mark all read  ⟲  │
                                                │ TODAY                         │
                                                │ • ▣ Run #14 awaiting plan     │ ← • = unread dot
                                                │     approval           22m    │   (accent), row
                                                │ • ✕ Task failed: “Add OAuth   │   bg --accent-muted
                                                │     flow” — agent error 1h    │
                                                │   ✓ Run #13 merged (PR #612)  │ ← read: no dot,
                                                │                        3h     │   plain row
                                                │ YESTERDAY                     │
                                                │   ⛨ Permission requested:     │
                                                │     Bash (resolved)    1d     │
                                                │   ⚠ Automation paused —       │
                                                │     judge failed       1d     │
                                                │            · · ·              │
                                                │        Load older ▾           │
                                                └───────────────────────────────┘
```

- Rows are compact list items (no card), 1-line title + relative time; click navigates via `target_json` and marks read.
- **Mark-read semantics:** rows visible in the viewport for >1s are marked read (batched `mark_notification_read`); "Mark all read" clears the dot. Opening the drawer alone does NOT mark all read.

### Empty states

```
 Needs action, empty:                     History, empty:
 ┌───────────────────────────────┐        ┌───────────────────────────────┐
 │                               │        │                               │
 │        ✓ (status-success)     │        │        ⊡ (--text-muted)       │
 │      All clear                │        │     No notifications yet      │
 │  Nothing needs your attention.│        │  Alerts and completions will  │
 │                               │        │  show up here.                │
 └───────────────────────────────┘        └───────────────────────────────┘
```

---

## 3. In-app toasts (PR 7)

Existing sonner instance, bottom-left, themed wrapper (`components/ui/sonner.tsx`). Fired only when: app **focused** + severity `action_required` + drawer **closed** + `focused_toasts_enabled`. Info severity never toasts. Action button runs the same navigation as the drawer row.

```
                     (bottom-left, above status bar)
 ┌──────────────────────────────────────────────────┐
 │ ⚠  Merge conflict on “Add OAuth flow”            │   toast.warning(title, {
 │    Programmatic and agent merge both failed —    │     description,
 │    manual resolution required.        [Open]     │     action: { label:"Open",
 └──────────────────────────────────────────────────┘       onClick: navigate }})

 ┌──────────────────────────────────────────────────┐
 │ ⛨  worker requests permission: Bash              │   duration: until expiry
 │    `git push origin feat/oauth`      [Respond]   │   for permissions (≤5m),
 └──────────────────────────────────────────────────┘   default 8s otherwise
```

---

## 4. macOS desktop notifications (PR 10)

Fired backend-side, only when the app window is **unfocused** (unless category marked "always"). Title = category headline; body = specific item; OS default sound; click focuses the app (no deep link v1 — the drawer is the landing surface, so the badge/drawer must already reflect the item when the app comes forward).

```
 single:                                          coalesced (≥3 within 5s):
 ┌───────────────────────────────────────┐        ┌───────────────────────────────────────┐
 │ ◆ RalphX                       now    │        │ ◆ RalphX                       now    │
 │ Plan approval needed                  │        │ 4 items need your attention           │
 │ Run #14 of “Nightly refactor” is      │        │ 2 reviews, 1 permission request,      │
 │ waiting on plan approval.             │        │ 1 merge conflict — acme-app           │
 └───────────────────────────────────────┘        └───────────────────────────────────────┘
```

Copy per category (title / body pattern):

| Category | Title | Body example |
|---|---|---|
| permission_request | `Permission needed` | `worker wants to run Bash on “Add OAuth flow” — expires in 5m` |
| agent_question | `Agent has a question` | `ideation on acme-app: “Should retries use exponential backoff?”` |
| agent_waiting | `Your turn` | `Agent finished on “Add OAuth flow” and is waiting for you` |
| review_needed | `Review ready` | `AI approved “Add rate limiting to API” — confirm to merge` |
| review_escalated | `Review escalated` | `AI couldn’t decide on “Migrate auth middleware”` |
| merge_conflict | `Merge conflict` | `“Add OAuth flow” needs manual conflict resolution` |
| task_failed | `Task failed` | `“Add OAuth flow” failed: agent error — retry from the app` |
| task_stuck | `Task needs attention` | `Recovery failed on “Add OAuth flow” — task may be stuck` |
| automation_plan_approval | `Plan approval needed` | `Run #14 of “Nightly refactor” is waiting on plan approval` |
| automation_paused | `Automation paused` | `“Nightly refactor” paused: plan judge failed` |
| automation_run_failed | `Automation run failed` | `Run #14 of “Nightly refactor”: run timed out` |
| gh_auth | `GitHub login needed` | `Enter code ABCD-1234 to finish gh login` |
| provider_paused | `Agents paused` | `Claude usage limit reached — queue paused, auto-resumes` |

---

## 5. Notification settings panel (PR 9)

New section in `SettingsView` (registered via `settings-registry.ts`), using `SectionCard` + `SettingRow` + `Switch` per styleguide §7/§8.

```
┌─ Notifications ────────────────────────────────────────────────────────┐
│                                                                        │
│  ┌ ⊡ ┐  Desktop notifications                                          │
│  └───┘  Native macOS alerts when RalphX needs you                      │
│  ──────────────────────────────────────────────────────────────────    │
│  Enable desktop notifications                                  [ ●  ]  │
│  Only when RalphX is in the background                         [ ●  ]  │
│     Alerts are suppressed while the app window is focused              │
│  In-app toasts for actionable items                            [ ●  ]  │
│                                                                        │
│  NOTIFY ME ABOUT                                    (eyebrow, upper)   │
│  ──────────────────────────────────────────────────────────────────    │
│  Agent requests (permissions & questions)                      [ ●  ]  │
│  Agent waiting for your reply                                  [ ●  ]  │
│  Reviews & escalations                                         [ ●  ]  │
│  Task failures & merge conflicts                               [ ●  ]  │
│  Automation approvals & pauses                                 [ ●  ]  │
│  Automation run completions                                    [ ○  ]  │
│  Git & GitHub authentication                                   [ ●  ]  │
│                                                                        │
│  ┌ ⓘ InlineNotice (info tone) ─────────────────────────────────────┐   │
│  │ The in-app badge and Needs-action list always stay on — these   │   │
│  │ toggles only control desktop alerts and toasts.                 │   │
│  └──────────────────────────────────────────────────────────────────┘  │
└────────────────────────────────────────────────────────────────────────┘
   Switch[checked]: bg --accent-primary · toggles disabled+opacity-50 when
   master toggle is off · groups map to categories server-side (one toggle
   may govern several NotificationCategory values)
```

Defaults match the plan: everything ON except run completions (info tier). First enablement triggers the OS permission prompt (`isPermissionGranted`/`requestPermission`); a `warn` InlineNotice appears if macOS permission was denied, with a "System Settings…" opener.

---

## 6. Category mapping (single source of truth for icon / severity / action / target)

| Category | Icon (lucide) | Icon color | Row action | Click target |
|---|---|---|---|---|
| permission_request | ShieldQuestion | --status-warning | Respond | re-raise `PermissionDialog` |
| agent_question | MessageCircleQuestion | --accent-primary | Answer | linked setup conversation (`openIdeationInAgents`) or current Agents conversation navigation |
| plan_approval | FileCheck | --accent-primary | Review plan | session artifact pane, plan tab |
| review_needed | GitPullRequest | --accent-primary | Review | in-place `ReviewDetailModal` |
| review_escalated | TriangleAlert | --status-warning | Decide | in-place `ReviewDetailModal` |
| qa_failed | FlaskConical | --status-error | Open task | `openTaskInAgents(taskId, "graph")` |
| merge_conflict | GitMerge | --status-error | Resolve | `openTaskInAgents(taskId, "graph")` |
| merge_incomplete | GitMerge | --status-warning | Open task | `openTaskInAgents(taskId, "graph")` |
| task_failed | XCircle | --status-error | Open task | `openTaskInAgents(taskId, "graph")` |
| task_blocked | Hand | --status-warning | Open task | `openTaskInAgents(taskId, "graph")` |
| task_stuck | LifeBuoy | --status-error | Open task | `openTaskInAgents(taskId, "graph")` |
| provider_paused | PauseCircle | --status-warning | — (info row) | activity view |
| automation_plan_approval | Bot | --accent-primary | Review plan | `requestAutomationRunOpen` → plan tab |
| automation_paused | Bot | --status-warning | Open automation | automation detail |
| automation_run_failed | Bot | --status-error | Open run | `requestAutomationRunOpen` |
| automation_run_completed | Bot | --status-success | Open run | `requestAutomationRunOpen` → pr tab |
| gh_auth | KeyRound | --status-warning | Enter code | gh login flow |
| git_auth_preflight | KeyRound | --status-warning | Open settings | project git settings |
| pr_review_action | GitPullRequestArrow | --accent-primary | Review action | workspace PR review pane |

Severity → icon color is fixed (`action_required` never recolors the card; only the icon carries status color). Orange remains the accent for selection/focus/badge; red/amber/green appear only inside icons per the styleguide "status colors for badges/alerts only" rule.

---

## 7. Behavior rules (bind the mockups to the perf/a11y non-negotiables)

1. **First paint wins**: clicking ⊡ flips `notificationsPanelOpen` synchronously; the 400px shell + header + tab chrome render before any query resolves. Skeleton rows (3 shimmering cards) until Tier 1 hydrates. Closing visually closes first; heavy children unmount after paint.
2. **Stable frame**: the drawer keeps a lightweight hidden frame mounted after first open (frequent-toggle rule) — subsequent toggles are visibility-only.
3. **No open/close animation** unless proven smooth in the real split layout (instant beats janky).
4. **Badge never lies**: count comes only from the live Tier 1 query; the dot only from unread history. If the attention query errors, show the last-known count with a subtle `⚠` tooltip — never zero-out on a failed read (fail-closed display).
5. **One item, one ping**: a state instance produces at most one desktop notification (dedupe key), regardless of how many times the drawer/toast re-renders it.
6. **Icon-only buttons** (⊡ trigger, ✕ close, ⋯ overflow, ⟲ refresh) each get `aria-label` + app Tooltip.
7. WKWebView: the drawer and cards set explicit `background-color`/`border-*` longhands; no chained `var()`; verify in `npm run tauri dev`.
