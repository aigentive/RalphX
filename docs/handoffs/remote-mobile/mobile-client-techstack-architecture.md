# RalphX Mobile — React Native Client: Tech Stack, Architecture & Full UX/UI Spec (2026-08-01, rev 2)

The mobile app is a **remote control for RalphX hosts**: it executes nothing locally, syncs the way the desktop client syncs (snapshot-hydrate + live WS), and inherits every capability the remote facade registers. Rev 2 commits to **React Native** and specs the complete screen set. Protocol ground truth: `.artifacts/specs/remote-multi-env/source-spec.md`; capability roadmap: `full-remote-management-implementation-spec.md`.

---

## 1. Tech stack (React Native, committed)

| Layer | Choice | Notes |
|---|---|---|
| Framework | React Native + Expo (dev client, EAS builds) | OTA updates for UI iterations; native modules allowed |
| Navigation | Expo Router (stack per tab, native gestures) | Deep links: `ralphx://pair?...#code=...`, `ralphx://env/<id>/...` |
| Styling | NativeWind (Tailwind syntax) | Tokens mirrored from the desktop design system; accent `#ff6b35`; system font (SF Pro / Roboto); dark-first |
| Data | TanStack Query, keys `(envId, cmd, argsHash)` | Event-driven invalidation identical to the desktop remote environment |
| State | Zustand stores: environments, supervisors, gates, composer drafts | Ports of the desktop TS stores |
| Transport | `fetch` + `WebSocket` implementing `NetworkInvoke` / `NetworkEventBus` | The only platform-specific layer; everything above rides shared packages |
| Shared packages | `@ralphx/remote-protocol` (zod schemas, 10-code error taxonomy, generated capability manifest), `@ralphx/remote-client` (supervisor, epoch/cursor, intent-poll helpers), `@ralphx/api` (typed command wrappers) | Extracted from `frontend/src/lib/remote` + `frontend/src/api`; consumed by desktop and mobile |
| Secure storage | `expo-secure-store` (iOS Keychain / Android Keystore) | Device token never in JS-accessible plain storage; supervisor requests it per connect |
| QR | `expo-camera` scanner | Pairing payload: one-time code in URL hash fragment |
| Lists | FlashList | Transcript + kanban virtualization |
| Media | Streamed download of `/remote/v1/attachments/:id` to file URI | Blob-in-memory only under ~2 MB |

**Sync model (unchanged from desktop):** invoke plane (`POST /remote/v1/invoke`), event plane (`GET /remote/v1/events?ticket=…` with `(streamEpoch, cursor)` resume; new epoch ⇒ cold-hydrate), allowlisted fetch remounts, intent rows + poll for spawn-adjacent mutations (start / continue / stop). Offline = read-only cache + banner; no mutation outbox. Background suspension is modeled as an ordinary disconnect; foreground triggers the reconnect path.

**Authority rules (non-negotiable, enforced in the shared client layer):**
- Writable affordances require confirmed scopes AND `presentation === connected`.
- The scope set gates rendering: `ui:read` viewer, `+ui:operate` brakes/inert edits, `+ui:agent` full control. Never render an enabled control the token can't use — render the locked variant with the reason.
- Every mutation carries `requestId` idempotency. All errors render from the 10-code `REMOTE_*` taxonomy — never a generic toast for a typed refusal.
- `emit()` on the event bus is local-only. The client never writes to the WS.

---

## 2. Navigation architecture

```
Root
├─ (no environments) → S1 Welcome → S2 Pairing
└─ Tab bar (per active environment, switcher in header)
   ├─ ⌂ Projects   S3 → S4 Kanban → S5 Task
   ├─ ✦ Agents     S6 → S7 Conversation (→ S8 Tool detail, S9 Gates,
   │                                      S10 Workspace → S11 Review)
   ├─ ◇ Plans      S12 Ideation sessions → session detail
   ├─ ▤ Activity   S13 Inbox/Notifications (+ Automations)
   └─ ⚙ Settings   S14 Environments & devices → S15 Env settings, S16 App
```

Global chrome on every screen:
- **Header**: environment pill (name + status dot: ● green connected / ◐ amber reconnecting / ○ gray offline) — tap opens the environment switcher sheet. Scope chip when not full control: `[viewer]` or `[operate]`.
- **Offline banner** (persistent, under header, when disconnected): `○ Offline — showing cached data · Retry`. All mutating controls become locked variants.
- **Intent-pending affordance**: any intent-row action (start/continue/stop) shows an inline spinner on the control plus a status line fed by the poll; terminal failure surfaces as an inline error card with the `REMOTE_*` code's human string and a Retry.

Design language: dark-first (`#111114` canvas, `#1b1b20` surfaces, 1px `#2a2a30` borders), accent `#ff6b35` reserved for primary actions + live-run indicators, status colors: running amber pulse, done green, failed red, paused gray. Text hierarchy via weight not color. Touch targets ≥ 44pt. Every icon-only button has an accessible label.

---

## 3. Screens, one by one

Legend for the specs: **Manages** = what the screen owns; **Data** = commands/events feeding it; **Scopes** = what each tier can do here. Mockups are ~phone width; `▸` = navigates, `⋯` = overflow menu.

### S1 — Welcome (first run / zero environments)

```
┌──────────────────────────────────┐
│                                  │
│            ◆ RalphX              │
│                                  │
│   Control your agents from       │
│   anywhere on your tailnet.      │
│                                  │
│   1. Open RalphX on your Mac     │
│   2. Settings → Remote Access    │
│   3. Show pairing code           │
│                                  │
│  ┌────────────────────────────┐  │
│  │      ▣  Scan QR code       │  │  ← primary, #ff6b35
│  └────────────────────────────┘  │
│      Enter code manually         │  ← text link
│                                  │
│  Requires Tailscale on this      │
│  phone. Open Tailscale ↗         │
└──────────────────────────────────┘
```

**Manages**: entry into pairing; Tailscale prerequisite check (best-effort probe of the host URL scheme — if unreachable, inline hint "Can't reach the tailnet — is Tailscale connected?").
**Data**: none (pre-auth).
**States**: default; camera-permission-denied (fall back to manual entry with explainer).

### S2 — Pairing

```
 Scan                     Manual
┌──────────────────────────────────┐
│  ‹ Back                          │
│  ┌────────────────────────────┐  │
│  │                            │  │
│  │      [ camera view ]       │  │
│  │   ┌ ─ ─ ─ ─ ─ ─ ─ ─ ┐      │  │
│  │        QR reticle           │  │
│  │   └ ─ ─ ─ ─ ─ ─ ─ ─ ┘      │  │
│  └────────────────────────────┘  │
│  Point at the pairing code on    │
│  your Mac.                       │
│         Enter code instead       │
└──────────────────────────────────┘
        ↓ on scan / submit
┌──────────────────────────────────┐
│  Pairing with                    │
│  ● studio.tailnet.ts.net         │
│                                  │
│  This device will be able to:    │
│  ✓ View projects, tasks, chats   │
│  ✓ Stop, pause & deny (brakes)   │
│  ✗ Start or steer agents         │
│    (enable later on the host)    │
│                                  │
│  Device name  [ Adrian's iPhone ]│
│  ┌────────────────────────────┐  │
│  │        Pair device         │  │
│  └────────────────────────────┘  │
└──────────────────────────────────┘
```

**Manages**: one-time code → device-token exchange; device naming; scope preview (rendered from the pairing payload — clearly separates default `read+operate` from host-granted `ui:agent`).
**Data**: `POST /remote/v1/auth/pair`; token → SecureStore; environment row created; supervisor starts.
**States**: scanning; exchanging (spinner on button); success (auto-navigate to S3); failures — invalid/expired code, version mismatch, unreachable host — each a distinct full-width error card with the taxonomy string and "Try again".
**Scopes**: n/a (creates them).

### S3 — Projects (environment home)

```
┌──────────────────────────────────┐
│ ● Studio ▾              [operate]│
│ Projects                         │
│ ┌──────────────────────────────┐ │
│ │ ralphx.app                 ▸ │ │
│ │ 4 running · 2 awaiting review│ │
│ ├──────────────────────────────┤ │
│ │ themefy-web                ▸ │ │
│ │ idle · 12 open tasks         │ │
│ ├──────────────────────────────┤ │
│ │ internal-tools             ▸ │ │
│ │ idle                         │ │
│ └──────────────────────────────┘ │
│                                  │
│  Projects are managed on the     │
│  host. This list is read-only.   │
└──────────────────────────────────┘
```

**Manages**: project selection; per-project live summary (running agents, review-waiting counts). Explicitly does NOT create projects (spec non-goal — footer states it).
**Data**: spawn-free project reads (`list_remote_projects` twin), task/agent count reads; invalidated by `task:*` / `agent:*` events.
**States**: loading skeleton rows; empty ("No projects on this host yet — add them on the Mac"); offline (cached list, dimmed counts + "as of 12:41").
**Scopes**: identical at all tiers (pure read).

### S4 — Kanban (project board)

```
┌──────────────────────────────────┐
│ ‹ ralphx.app          ⌕  ⋯       │
│ Plan: v0.90 remote ▾             │  ← active-plan filter
│ ◄ Backlog │ Ready │ Running ► ●  │  ← swipeable columns
│ ┌──────────────────────────────┐ │
│ │ Fix FK baseline diff       ▸ │ │
│ │ #482 · fix · ▲ high          │ │
│ ├──────────────────────────────┤ │
│ │ Remote stop lane           ▸ │ │
│ │ #495 · feat · ⟳ worker 12m   │ │  ← amber pulse when running
│ ├──────────────────────────────┤ │
│ │ + Add task (Backlog)         │ │  ← operate: backlog-only create
│ └──────────────────────────────┘ │
│        long-press card:          │
│   ┌ Pause ─ Block ─ Move ▸ ┐     │  ← Move/resume locked at operate
└──────────────────────────────────┘
```

**Manages**: column browsing (swipe between state-machine columns, running counts per column); task cards (title, id, type, priority, live run indicator); backlog task creation; brakes via long-press.
**Data**: task list reads + `task:*` events; `create_task` (Backlog pinned at operate), `pause_task`, `block_task`, `move_task`/`resume_task` (`ui:agent`).
**States**: per-column skeletons; empty column illustrations; offline = read-only cached board.
**Scopes**: read = browse; operate = + create-backlog, pause, block; agent = + move, resume, restart, approve (menu entries appear; locked variants show `⌂ agent control required` hint otherwise).

### S5 — Task detail

```
┌──────────────────────────────────┐
│ ‹ Board            #495       ⋯  │
│ Remote stop lane                 │
│ ⟳ Running · worker · 12m         │
│ [ Overview | Steps | Activity |  │
│   Review ]                       │
│ ────────────────────────────────│
│ Overview                         │
│  Priority  ▲ High   (editable)   │
│  Category  feat     (editable)   │
│  Branch    feat/rme-wp2-stop     │
│  Descr.    Implement stop via …  │  ← read-only at operate
│ ────────────────────────────────│
│ ┌ ■ Stop run ┐ ┌ ⏸ Pause ┐       │  ← brakes row, always visible
│ └────────────┘ └─────────┘       │
│ ┌────────── Resume ───────────┐  │  ← agent-gated primary
└──────────────────────────────────┘
```

**Manages**: full task lifecycle view. Tabs: **Overview** (metadata; `category`/`priority` editable at operate — the only inert edits; title/description editable only at agent tier because they're agent-consumed), **Steps** (step list + status; start/complete/skip at agent), **Activity** (full-timestamp state history), **Review** (links into S11 when review artifacts exist).
**Data**: task read + state history + steps + `task:*`/`step:*` events; brakes `stop_task`/`pause_task` (operate); `update_task`, step ops, `resume_task` intent (agent).
**States**: running (live header pulse), paused, blocked (banner with unblock — agent-gated), failed (terminal banner: "Failed steps are terminal on the host"), offline.
**Scopes**: as annotated; every locked control renders with the lock reason, never hidden — the user should learn what flipping `ui:agent` on the host unlocks.

### S6 — Agents (conversations + inbox)

```
┌──────────────────────────────────┐
│ ● Studio ▾            [agent] ⌕  │
│ Agents          ◉ 2 need you     │  ← inbox strip, accent
│ ┌──────────────────────────────┐ │
│ │ ◉ Permission: run `cargo …`  │ │
│ │   ralphx.app · builder     ▸ │ │
│ │ ◉ Question: pick base branch │ │
│ │   themefy-web · planner    ▸ │ │
│ └──────────────────────────────┘ │
│ Conversations                    │
│ ┌──────────────────────────────┐ │
│ │ ⟳ WP2 stop lane            ▸ │ │
│ │   opus · running · 2m ago    │ │
│ ├──────────────────────────────┤ │
│ │ ○ Release notes draft      ▸ │ │
│ │   idle · yesterday           │ │
│ ├──────────────────────────────┤ │
│ │ ✓ FK migration fix         ▸ │ │
│ │   done · Tue                 │ │
│ └──────────────────────────────┘ │
│                        ┌──────┐  │
│                        │ ✚ New│  │  ← agent-gated FAB
└──────────────────────────────────┘
```

**Manages**: the Agents inbox (pending permission requests + questions, surfaced above everything — these are the highest-urgency mobile moments) and the conversation list (host sidebar order preserved; status glyph, provider, recency).
**Data**: sidebar/list remote twins (paginated), pending-gate reads (fail-closed), `agent:*` + gate events; `✚ New` → S7 composer in start mode (start intent).
**States**: skeletons; empty ("No conversations yet"); inbox empty state collapses the strip; offline.
**Scopes**: read = list + transcripts; operate = + deny permission, answer question **No** (deny-shaped answers only if backend classifies so — otherwise question answering is agent); agent = + new conversation, approvals.

### S7 — Conversation

```
┌──────────────────────────────────┐
│ ‹ Agents   WP2 stop lane    ⋯    │
│ ⟳ running · opus · turn 6        │  ← run status bar (live)
│ ┌──────────────────────────────┐ │
│ │ You  12:03                   │ │
│ │ Implement the stop intent…   │ │
│ │──────────────────────────────│ │
│ │ ✦ Agent  12:04               │ │
│ │ I'll start with the entity…  │ │
│ │ ▸ 🔧 Edit remote_stop.rs     │ │  ← tool chip, tap → S8
│ │ ▸ 🔧 cargo test (4 files) ✓  │ │
│ │ ▸ 🖼 screenshot.png          │ │  ← attachment, tap → viewer
│ │──────────────────────────────│ │
│ │ ◉ Permission needed          │ │  ← inline gate card → S9
│ │ Run `git push origin…`       │ │
│ │ [ Deny ]        [ Approve ]  │ │
│ └──────────────────────────────┘ │
│ ┌──────────────────────────────┐ │
│ │ Message…            ⏎        │ │
│ └──────────────────────────────┘ │
│  opus ▾ · high ▾        ■ Stop   │  ← composer options + stop
└──────────────────────────────────┘
```

**Manages**: the core loop. Virtualized transcript (chrome + placeholders paint first, hydration after — same first-paint rule as desktop); live streaming blocks; tool chips (truncated preview, tap expands); attachments; inline gate cards; composer with model/effort options (these travel with the send — UX-5); **Stop** always visible while running.
**Data**: transcript twins (paged) + `agent:*` stream events; send: live run ⇒ `send_remote_chat_message`, idle ⇒ continue-intent + poll (seamless to the user — one send affordance, the client picks the path); stop ⇒ stop-intent + poll; gates via registered approve/deny + answer commands.
**States**: streaming (token-level append); idle (composer hint "Sending will wake this agent"); **send-pending** (message renders with ◐ "delivering…" until the intent terminalizes — a persisted-never-dispatched intent MUST surface as a red failure state on the bubble with Retry, never a ghost sent message); stop-pending (Stop → spinner → run status flips or inline failure); attachment placeholders fill from the binary route (until host-produced ingress ships: "Stored on the host" chip + copy-path); offline (composer disabled with reason).
**Scopes**: read = watch live; operate = Stop + Deny; agent = send/steer/approve/answer/new.

### S8 — Tool-call detail (sheet)

```
┌──────────────────────────────────┐
│ ── drag handle ──                │
│ 🔧 Edit remote_stop.rs        ✕  │
│ status ✓ completed · 1.2s        │
│ Arguments                        │
│ ┌──────────────────────────────┐ │
│ │ { "path": "src-tauri/…",     │ │
│ │   "old_string": "…" }        │ │  ← mono, scrollable
│ └──────────────────────────────┘ │
│ Result                           │
│ ┌──────────────────────────────┐ │
│ │ Applied 1 edit. 34 lines…    │ │
│ └──────────────────────────────┘ │
│            Copy result           │
└──────────────────────────────────┘
```

**Manages**: full untruncated arguments/result for one tool call.
**Data**: `get_agent_message_tool_call_detail` / `get_agent_timeline_item_tool_call_detail` (registered `ui:read` after WP3). Fetch error ⇒ error card in the sheet (fail-closed — never silently show the truncated preview as if complete).
**States**: loading, loaded, error+retry.

### S9 — Gates (approval / question sheets)

```
┌──────────────────────────────────┐
│ ◉ Permission request             │
│ WP2 stop lane · builder          │
│                                  │
│ The agent wants to run:          │
│ ┌──────────────────────────────┐ │
│ │ git push origin feat/rme-…   │ │
│ └──────────────────────────────┘ │
│ ┌──── Deny ────┐ ┌─ Approve ──┐  │
│ └──────────────┘ └────────────┘  │
│    Deny is available to every    │
│    paired device.                │
├──────────────────────────────────┤
│ ？ Question                      │
│ Which base branch?               │
│ ○ main                           │
│ ● feat/remote-multi-env          │
│ [ or type an answer…          ]  │
│ ┌──────────── Send ───────────┐  │
└──────────────────────────────────┘
```

**Manages**: the two mid-run gates. Approve/deny permission (deny at operate, approve at agent); structured or free-text question answers (agent — free-text steers a live run).
**Data**: pending-gate reads (fail-closed — a read error renders "Couldn't verify pending gates", never an empty happy state); registered `approve_`/`deny_permission_request` (pinned decisions), `answer_user_question`/`resolve_user_question` seam. CAS/stale handling: acting on an already-resolved gate renders "Already handled on the host" (idempotent, calm).
**Entry points**: inbox strip (S6), inline cards (S7), push notification (P3).

### S10 — Workspace (per-conversation publish/PR surface)

```
┌──────────────────────────────────┐
│ ‹ Conversation   Workspace       │
│ branch feat/rme-wp2-stop         │
│ base   feat/remote-multi-env ✓   │
│ ┌──────────────────────────────┐ │
│ │ Changes  14 files  +812 −96 ▸│ │  ← diff summary (Wave-2 lane)
│ ├──────────────────────────────┤ │
│ │ Review   ● blocking: 2     ▸ │ │  → S11
│ ├──────────────────────────────┤ │
│ │ PR #961  open · checks ⟳   ▸ │ │  ← PR card (read + close)
│ └──────────────────────────────┘ │
│ Automation                       │
│  Auto-publish        [on ▣]      │  ← agent-gated toggles
│  PR supervision      [on ▣]      │
│  Auto-merge          [off □]     │
│ ┌────────── Publish ──────────┐  │  ← agent-gated primary
└──────────────────────────────────┘
```

**Manages**: workspace state for one conversation: branch/base freshness, change summary, review status, linked PR lifecycle, automation toggles, publish.
**Data**: workspace reads (projected twins per Wave-2), publication events, PR monitor state; toggles = seam-split flag writes (`SeedsSpawnTriggeringState` @ agent); `publish_agent_conversation_workspace`, `close_agent_workspace_pr` (agent). Diff drill-down ships with the Wave-2 diff lane; until then the row shows counts only with "Full diff on the host".
**States**: clean/dirty/stale-base (banner: "Base moved — update from base runs on the host"); PR terminal (merged/closed collapses actions — durable-state-authoritative, no stale controls); offline.
**Scopes**: read = all state; operate = nothing extra; agent = toggles, publish, close PR.

### S11 — Review artifact viewer

```
┌──────────────────────────────────┐
│ ‹ Workspace   Review v3          │
│ ● Blocking · 2 requested changes │
│ [ Overview | Requested changes ] │
│ ┌──────────────────────────────┐ │
│ │ ## Summary                   │ │
│ │ The stop-intent dispatcher   │ │
│ │ correctly claims…            │ │
│ │                              │ │
│ │ ▸ src-tauri/…/stop.rs:141    │ │  ← hunk annotation link
│ │   claim/CAS race note…       │ │
│ └──────────────────────────────┘ │
│ ┌ Request fixes ┐ ┌ Approve ─┐   │  ← agent-gated actions
└──────────────────────────────────┘
```

**Manages**: the versioned Overview + Requested Changes artifact pair (markdown render, hunk annotations as expandable cards; file:line links copy the ref — no local file to open). Version picker in title. For **Review PR** conversations the same screen renders the PR-review artifact for the reviewed head, with propose-Approve/Request-Changes/Comment actions that queue for explicit user submission semantics identical to desktop.
**Data**: review-context remount (WP3) → artifact ids → `get_artifact`/`get_artifact_at_version`; review events invalidate. Actions: fixer routing / gate completion commands (agent).
**States**: no-review-yet empty state; context-fetch error (typed, retry); artifact version superseded banner ("A newer review exists — v4").

### S12 — Plans (ideation)

```
┌──────────────────────────────────┐
│ ● Studio ▾  Plans                │
│ ┌──────────────────────────────┐ │
│ │ ◇ Remote full management   ▸ │ │
│ │   verifying · round 2        │ │
│ ├──────────────────────────────┤ │
│ │ ◇ Provider connections     ▸ │ │
│ │   converged · 6 proposals    │ │
│ └──────────────────────────────┘ │
│        session detail ▾          │
│ [ Chat | Plan | Proposals ]      │
│  Proposals                       │
│  ▣ P1 Continue lane              │  ← selection toggles (agent)
│  ▣ P2 Stop lane                  │
│  □ P3 Attachments                │
│ ┌── Apply to kanban (host) ───┐  │  ← locked: runs on host, v1
└──────────────────────────────────┘
```

**Manages**: ideation sessions list; per-session tabs: **Chat** (same composer pattern as S7 via `send_chat_message`/`send_orchestrator_message`), **Plan** (rendered plan doc + verification status read), **Proposals** (selection toggles). **Apply to kanban stays host-locked in v1** (spawn-heavy) — the button renders locked with "Runs on the host" until its Wave-2+ seam exists.
**Data**: ideation session/message/proposal reads (paged), `set_proposal_selection`/`toggle_proposal_selection`, session chat sends (agent); verification status reads.
**Scopes**: read = browse everything; agent = chat + proposal selection.

### S13 — Activity (inbox, notifications, automations)

```
┌──────────────────────────────────┐
│ ● Studio ▾  Activity             │
│ [ Needs you | All | Automations ]│
│ Needs you (2)                    │
│ ┌──────────────────────────────┐ │
│ │ ◉ Permission · cargo publish │ │
│ │ ◉ Question · base branch     │ │
│ └──────────────────────────────┘ │
│ All                              │
│ │ ✓ Review passed · WP1 · 12:22│ │
│ │ ⇧ PR #961 opened · 12:10     │ │
│ │ ■ Run stopped · WP3 · 11:58  │ │
│ Automations                      │
│ │ ⟳ nightly-triage · running   │ │
│ │   [ ⏸ Pause ] [ ■ Stop ]     │ │  ← brakes at operate
└──────────────────────────────────┘
```

**Manages**: unified notification feed (badge source for the tab), gate fast-path ("Needs you"), automation runs with brakes. Full timestamps on every row.
**Data**: notification reads (badge fail-closed: unread-count read error shows `!`, not 0), notification events; automation list/run reads, `stop_automation`/`pause_automation` (operate, authority-reducing).
**States**: empty ("All caught up"); offline (cached feed).

### S14 — Environments & devices

```
┌──────────────────────────────────┐
│ Settings                         │
│ Environments                     │
│ ┌──────────────────────────────┐ │
│ │ ● Studio                   ▸ │ │
│ │   agent control · connected  │ │
│ ├──────────────────────────────┤ │
│ │ ○ MacBook                  ▸ │ │
│ │   viewer · unreachable       │ │
│ └──────────────────────────────┘ │
│ ┌──── ✚ Pair new environment ──┐ │
│ App                              │
│  Appearance          Dark ▾      │
│  Notifications       On ▾        │
│  About · protocol v1             │
└──────────────────────────────────┘
```

**Manages**: environment roster (connection state, granted tier), entry to pairing (S2) and per-environment settings (S15); app-local prefs.
**Data**: local environment store; descriptor probe on pull-to-refresh.

### S15 — Environment settings

```
┌──────────────────────────────────┐
│ ‹ Settings   Studio              │
│ Connection                       │
│  Host  studio.tailnet.ts.net     │
│  Status ● connected · 34ms       │
│  Protocol v1 · epoch 8f2c        │
│ This device                      │
│  Name   Adrian's iPhone          │
│  Scopes read · operate · agent   │
│  ┌──── Remove this device ────┐  │  ← destructive, confirm sheet:
│  └────────────────────────────┘  │    revoke → keychain → row
│ Host options (ui:agent)          │
│  Default provider   claude ▾     │
│  Role defaults              ▸    │  ← narrowed editor: model/effort
│  Review runtime settings    ▸    │    only; approval/sandbox rows
│  Execution settings         ▸    │    shown read-only "host-managed"
└──────────────────────────────────┘
```

**Manages**: per-environment connection diagnostics; device self-management (rename, remove = revoke-first flow); host option editors backed by the registered settings commands. The narrowed role-default editor **shows** `approval_policy`/`sandbox_mode` as read-only "host-managed" rows — visible honesty about the security envelope staying host-local.
**Data**: session/descriptor reads, `POST /remote/v1/auth/revoke`, registered `update_*_settings` commands (agent).
**States**: connected/degraded (latency + last-event age); revoke failure (host unreachable) still deletes locally with "host copy revokes on next contact" note.

---

## 4. Cross-screen patterns (the contract every screen obeys)

1. **Locked ≠ hidden.** Scope-gated controls render disabled with the reason ("Enable agent control for this device on the host"). Discovery of the grant path is a product feature.
2. **Brakes are never gated behind agent.** Stop/pause/block/deny appear at `ui:operate` on every surface that shows a live run.
3. **Intent lifecycle is visible.** Pending → spinner on the initiating control + status line; terminal failure → inline error card with taxonomy string + Retry; never a silent revert.
4. **Fail-closed rendering.** A read error is an error state; it never renders as "zero items" when zero would calm the user (gates, badges, review status).
5. **Stale-action calm.** CAS/already-resolved refusals render "Already handled on the host" — expected in a two-writer world (desktop + phone), not an error tone.
6. **First paint wins.** Every heavy surface (transcript, board, diff) paints chrome + placeholders synchronously; hydration follows a paint boundary.
7. **Cached truth is labeled.** Offline views show "as of <time>" and disable mutations with the offline reason.

## 5. Phasing (unchanged)

P0 pairing + read plane (S1–S8 read paths) → P1 brakes + gates (S9, brake rows) → P2 full control at `ui:agent` (composer send/continue, start, toggles, publish, review actions — lands as the desktop facade waves merge) → P3 push bridge, background polish, persisted offline cache.
