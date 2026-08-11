# RalphX — Agents Layout (ASCII Spec)

> The conversation-first layout is now the live shell. Agents is a root view;
> Plan, Ideation, Tasks, Graph, and Kanban are artifacts within it. References
> below to standalone task/planning roots describe the pre-migration proposal
> and must not be used to add new navigation or persisted route state.

---

## 1. Goals

| # | Goal |
|---|------|
| 1 | Keep the live root rail aligned with the eight-value `AppView` contract |
| 2 | Use **`Agents`** as the conversation-first workspace and host task/planning artifacts inside it |
| 3 | Inside the Agents view: left sidebar shows project-scoped conversations (multi-project tree) |
| 4 | Active session owns the middle pane; artifact views split it into 2 cols on demand |
| 5 | Keep Plan / Verify / Proposal / Tasks as per-session artifacts **inside the Agents view** |
| 6 | Reuse the existing chat component (messages, composer, model picker, send, streaming, attachments, etc.) verbatim inside the middle pane — no rework of chat capabilities in this spec |

### ⚠ Non-negotiable — reuse, don't rebuild

This spec is **additive** — a new Agents view that wraps existing screens, not a rewrite.

- The top navbar and every screen it reaches today (Ideation, Graph, Kanban, Insights, Settings, project select) stay untouched.
- Plan, Verification, Proposal, and Tasks (Graph + Kanban) screens — **reuse the existing components, hooks, stores, Tauri commands, and events as-is** inside the artifact pane wrapper.
- The artifact pane in §6 is **only a wrapper** that hosts these screens. No new screens, no new data layers, no new actions, no new copy, no new state.
- If an ASCII block in this spec shows controls or copy that differ from the shipping screen, the **shipping screen wins** — the ASCII is a layout reference only.

Full callout and scope table in §6.

---

## 2. Window — overall layout

**Agents view ON** (new button toggled in navbar):

```
┌───────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┐
│ ⬤⬤⬤  ◂ ▸   Ideation · Graph · Kanban · Insights · Settings · Project ▾ · [ ◧ Agents ●]            ⌕ Search          👤  │   ← existing navbar, unchanged — new Agents link appended (active)
├───────────────────────────────┬───────────────────────────────────────────────────────┬───────────────────────────────────────┤
│                               │                                                       │                                       │
│   LEFT  SIDEBAR               │          CHAT  PANE                                   │       ARTIFACT  PANEL                 │
│   (projects · sessions)       │          (active session)                             │       (toggleable · plan / verify /   │
│                               │                                                       │        proposal / tasks)              │
│                               │                                                       │                                       │
│                               │                                                       │                                       │
│                               │                                                       │                                       │
│                               │                                                       │                                       │
│                               │                                                       │                                       │
│                               │                                                       │                                       │
│                               │  (existing chat component)                            │                                       │
└───────────────────────────────┴───────────────────────────────────────────────────────┴───────────────────────────────────────┘
```

**Agents view OFF** (navbar default — existing app unchanged):

```
┌───────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┐
│ ⬤⬤⬤  ◂ ▸   Ideation · Graph · Kanban · Insights · Settings · Project ▾ · [ ◧ Agents  ]            ⌕ Search          👤  │   ← existing navbar, unchanged — new Agents link appended (inactive)
├───────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┤
│                                                                                                                               │
│                              [ whichever navbar tab is active — existing screen renders here ]                                │
│                                                                                                                               │
└───────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┘
```

**Widths (reference, 1440px window):**

| Pane | Collapsed | Expanded | Notes |
|------|-----------|----------|-------|
| Left sidebar | 280px | 280px | Fixed, resizable 240–360px |
| Chat pane | remaining | 50% of middle | Shrinks when artifact opens |
| Artifact pane | hidden | 50% of middle | Splits middle in half, resizable |

---

## 3. Left sidebar — detail

Projects are **first-class folders** in the sidebar. Each project owns its own session list; a chat always belongs to exactly one project.

```
┌─────────────────────────────────┐
│ Projects             ＋   ⌕     │   ← header · add project · search
├─────────────────────────────────┤
│                                 │
│  ＋  New agent                   │   ← opens modal (picks project)
│                                 │
├─────────────────────────────────┤
│                                 │
│  ▾ 📁 reefbot.ai         ＋ ⋯  │   ← expanded · hover-reveal add/menu
│     ● Fix ideation stall    ⋯  │
│     ○ QA kanban regression      │
│     ⚠ Refactor splash       1  │
│                                 │
│  ▾ 📁 ralphx-core        ＋ ⋯  │
│     ◐ Theme system  running·12m │
│     ○ Merger conflict audit     │
│     ○ Rust lint sweep           │
│                                 │
│  ▸ 📁 blog-gen              3  │   ← collapsed · unread / running badge
│                                 │
│  ▸ 📁 reports                   │
│                                 │
│  ▸ 📁 sandbox                   │
│                                 │
│  ⋮  (scrollable)                │
│                                 │
└─────────────────────────────────┘
```

> Insights and Settings stay in the top navbar — they are **not** duplicated at the sidebar bottom. Use the existing navbar entries to reach them.

### Sidebar header controls

| Control | Behavior |
|---------|----------|
| `＋` (header) | Opens **New project** dialog |
| `⌕` (header) | Expands an inline search across all projects' sessions |
| `＋ New agent` | Opens **New agent** dialog — user must pick a project and provider first |

### Project-row controls

| Control | Behavior |
|---------|----------|
| `▾ / ▸` | Collapse / expand project's session list |
| `📁 reefbot.ai` | Click name to focus project (scopes artifact-panel Tasks view) |
| `＋` (on row) | Create a chat **inside this project** — no picker needed |
| `⋯` (on row) | Rename · change repo path · archive · delete |
| `n` badge | Count of running or needs-approval sessions inside a collapsed project |

### Session-row states

| Glyph | State | Meaning |
|-------|-------|---------|
| `●` | Active | Selected in chat pane |
| `○` | Idle | Has history, no agent running |
| `◐` | Running | Agent currently executing |
| `⚠` | Needs approval | Plan / question blocked on user |
| `✓` | Completed | Agent finished its last run cleanly |
| `✕` | Error | Last run failed or session expired |

### 3.1 New agent dialog

Triggered by the top `＋ New agent` button. A chat session **is** an agent — every session is bound to a provider (Claude or Codex) and gets an External-MCP token on creation. See §11 for the full orchestration spec.

```
┌─ New agent ────────────────────────────────────────╳─┐
│                                                      │
│  Project                                             │
│  ┌────────────────────────────────────────────────┐  │
│  │ 📁 reefbot.ai                             ▾    │  │
│  └────────────────────────────────────────────────┘  │
│     · reefbot.ai                                     │
│     · ralphx-core                                    │
│     · blog-gen                                       │
│     ─────────────                                    │
│     ＋ New project…                                  │
│                                                      │
│  Provider                                            │
│  ┌────────────────────────────────────────────────┐  │
│  │ ( ● ) Claude       ( ○ ) Codex                 │  │
│  └────────────────────────────────────────────────┘  │
│                                                      │
│  Model                                               │
│  ┌────────────────────────────────────────────────┐  │
│  │ claude-opus-4-7                           ▾    │  │   ← list filtered by provider
│  └────────────────────────────────────────────────┘  │
│                                                      │
│  Title (optional)                                    │
│  ┌────────────────────────────────────────────────┐  │
│  │ e.g. "Fix ideation stall"                      │  │
│  └────────────────────────────────────────────────┘  │
│                                                      │
├──────────────────────────────────────────────────────┤
│                              [ Cancel ]   [ Create ] │
└──────────────────────────────────────────────────────┘
```

**Rules**

| Rule | Detail |
|------|--------|
| Project is required | Create is disabled until a project is picked |
| Provider is required | One of `Claude` or `Codex`; defaults to last-used |
| Model list | Filtered by provider — Claude models for Claude, GPT-5.4 etc. for Codex |
| Default project | Most recently used (stored per-user) |
| Inline create | Picking `＋ New project…` opens the New project dialog inline, returns to the picker populated |
| Title | Optional — first user message auto-titles the session if empty |
| Branch | **Not part of this spec.** Branch handling stays with whatever the existing chat / task flow does today; no branch picker in this dialog. |
| MCP token | Auto-generated on Create; scoped to this session, revoked on close (see §11.3) |

Fast path: clicking the `＋` on a project row creates a session using the **last-used provider + model** for that project and skips this dialog (inline title input appears at the top of the project's session list).

### 3.2 New project dialog

Triggered by the header `＋` or by `＋ New project…` inside the New agent picker.

```
┌─ New project ──────────────────────────────────────╳─┐
│                                                      │
│  Name                                                │
│  ┌────────────────────────────────────────────────┐  │
│  │ e.g. reefbot.ai                                │  │
│  └────────────────────────────────────────────────┘  │
│                                                      │
│  Repository path (optional)                          │
│  ┌────────────────────────────────────────────────┐  │
│  │ /Users/me/code/reefbot                ⌕ Browse │  │
│  └────────────────────────────────────────────────┘  │
│  Leave empty to run tasks in a sandbox workspace.    │
│                                                      │
│  Icon                                                │
│  ( 📁 )  ( 🧪 )  ( 🛠 )  ( 📦 )  ( 🧭 )  ( ⋯ )    │
│                                                      │
├──────────────────────────────────────────────────────┤
│                              [ Cancel ]   [ Create ] │
└──────────────────────────────────────────────────────┘
```

**Rules**

| Rule | Detail |
|------|--------|
| Name required | Must be unique per user |
| Repo path optional | Empty → sandbox mode; path → binds git working tree to this project |
| Icon | Defaults to `📁`; purely decorative |
| Branch | **Not part of this spec.** Use whatever default-branch resolution the existing project flow already does. |

### Project-scope & focus

- Every chat belongs to exactly one project — no "All projects" catch-all chats.
- Clicking a project **name** (not its chevron) focuses it: the artifact-panel Tasks view filters to that project's tasks. Focus is visual only; it does not move the active chat.
- The sidebar remembers each project's expanded/collapsed state per user.

---

## 4. Chat pane — collapsed (artifact panel closed)

> **Reuse-only.** The body of the chat pane (header title, message list, composer, model picker, send / stop, streaming, attachments, voice, tool-call rendering, question cards, etc.) is the **existing chat component** mounted inside this pane. This spec only adds the artifact-toggle icons in the header; everything below the header is whatever ships today.
>
> ❌ Do not add a branch picker, PR chip, working-tree badge, or any footer below the composer — branch/PR are **out of scope** for this spec.
> ✅ Do add the `⊙ ✓ ◈ ▦ ⋯ ⊞` artifact-toggle strip to the chat-pane header (the only new UI in the chat pane).

```
┌──────────────────────────────────────────────────────────────────────────────────────────────────┐
│  Fix ideation stall                                          ⊙  ✓  ◈  ▦       ⋯        ⊞        │   ← header · title · artifact toggles (NEW) · more · split
├──────────────────────────────────────────────────────────────────────────────────────────────────┤
│                                                                                                  │
│                                                                                                  │
│                   ( existing chat component — messages + composer )                              │
│                                                                                                  │
│                                                                                                  │
└──────────────────────────────────────────────────────────────────────────────────────────────────┘
```

### Top-bar icons (chat-pane header)

| Icon | Opens artifact tab | Notes |
|------|---------------------|-------|
| `⊙` | Plan | Constraints · avoid · proof obligations |
| `✓` | Verification | Adversarial debate rounds, gaps, convergence |
| `◈` | Proposal | Proposed tasks pending user acceptance |
| `▦` | Tasks | Graph ↔ Kanban toggle inside |
| `⊞` | Split toggle | Collapses / restores the artifact pane |

Clicking an already-open tab closes the artifact pane. State is persisted **per session**.

---

## 5. Chat pane — expanded (artifact panel open, split into 2 cols)

> The middle column is the **existing chat component** rendered at half width. No branch picker, no PR chip, no footer additions — just the chat-pane header (with the artifact toggles) + the existing chat body below it.

```
┌─────────────────────────────────┬──────────────────────────────────────┬─────────────────────────────────────────────────────┐
│ Projects             ＋   ⌕    │ Fix ideation stall       ⊙ ✓ ◈ ▦ ⋯ ⊞ │ Plan · Verify · Proposal · Tasks              ╳    │
├─────────────────────────────────┤──────────────────────────────────────┤─────────────────────────────────────────────────────┤
│  ＋ New agent                   │                                      │                                                     │
│                                 │                                      │                                                     │
│  ▾ 📁 reefbot.ai        ＋ ⋯  │                                      │                                                     │
│     ● Fix ideation stall  ⋯   │                                      │                                                     │
│     ○ QA kanban regression     │   (existing chat component            │                                                     │
│     ⚠ Refactor splash    1    │    — rendered at half width)          │                (tab body empty)                     │
│                                 │                                      │                                                     │
│  ▾ 📁 ralphx-core       ＋ ⋯  │                                      │                                                     │
│     ◐ Theme system running·12m │                                      │                                                     │
│     ○ Merger conflict audit    │                                      │                                                     │
│     ○ Rust lint sweep          │                                      │                                                     │
│                                 │                                      │                                                     │
│  ▸ 📁 blog-gen             3  │                                      │                                                     │
│  ▸ 📁 reports                  │                                      │                                                     │
│  ▸ 📁 sandbox                  │                                      │                                                     │
│                                 │                                      │                                                     │
└─────────────────────────────────┴──────────────────────────────────────┴─────────────────────────────────────────────────────┘
```

`╳` on the artifact header closes the pane (equivalent to `⊞` split toggle).

---

## 6. Artifact pane — per-tab

> ### ⚠ NON-NEGOTIABLE — REUSE ONLY, NO NEW SCREENS
>
> The artifact pane is a **thin wrapper**. It mounts the **existing** Plan, Verification, Proposal, and Tasks (Graph + Kanban) screens exactly as they are today. **Nothing new gets built inside it** — not headers, not empty states, not actions, not data fetches, not stores.
>
> | What you do | What you don't do |
> |-------------|-------------------|
> | ✅ Move the existing `PlanView`, `VerificationView`, `ProposalView`, `TaskGraphView`, `KanbanBoard` components into the pane slot | ❌ Rewrite any of them |
> | ✅ Reuse current hooks, stores, Tauri `invoke` calls, and event wiring unchanged | ❌ Add new commands or new query layers |
> | ✅ Reuse the current tab switcher / view toggle (Graph ↔ Kanban) logic verbatim | ❌ Re-implement tab state or view toggling |
> | ✅ Preserve all existing action buttons, approval gates, and side effects in-place | ❌ Add new buttons, new copy, new confirmations |
> | ✅ Let the wrapper handle only: mount point, close button (`╳`), tab header strip, per-session persistence of "last open tab" | ❌ Own any artifact data or behavior |
>
> The ASCII below is **purely a positioning reference** for where the existing screens render inside the wrapper — not a redesign of those screens. If the ASCII shows copy or controls that differ from what ships today, **the existing screen wins** and the ASCII should be treated as approximate.
>
> **Implementation consequence:** this refactor should be a move + mount change, not a rewrite. PRs that touch the internals of Plan / Verification / Proposal / Tasks components are out of scope for the layout refactor.

> **Tab content is intentionally left blank in this spec.** For now we only ship the wrapper — empty tab bodies are fine. The existing `PlanView` / `VerificationView` / `ProposalView` / `TaskGraphView` / `KanbanBoard` components will be mounted into these empty slots in a later pass. Do not build placeholder copy, spinners, or empty-state illustrations inside the tabs; leave the content region blank.

### 6.1 Plan

```
┌─ Plan · Verify · Proposal · Tasks ──────────────────╳─┐
│                                                       │
│                                                       │
│                                                       │
│                    (tab body empty)                   │
│                                                       │
│                                                       │
│                                                       │
└───────────────────────────────────────────────────────┘
```

### 6.2 Verification

```
┌─ Plan · Verify · Proposal · Tasks ──────────────────╳─┐
│                                                       │
│                                                       │
│                                                       │
│                    (tab body empty)                   │
│                                                       │
│                                                       │
│                                                       │
└───────────────────────────────────────────────────────┘
```

### 6.3 Proposal

```
┌─ Plan · Verify · Proposal · Tasks ──────────────────╳─┐
│                                                       │
│                                                       │
│                                                       │
│                    (tab body empty)                   │
│                                                       │
│                                                       │
│                                                       │
└───────────────────────────────────────────────────────┘
```

### 6.4 Tasks — Graph view

```
┌─ Plan · Verify · Proposal · Tasks ──────────────────╳─┐
│                                                       │
│  [ Graph ]   [ Kanban ]                               │
├───────────────────────────────────────────────────────┤
│                                                       │
│                                                       │
│                    (tab body empty)                   │
│                                                       │
│                                                       │
└───────────────────────────────────────────────────────┘
```

### 6.5 Tasks — Kanban view

```
┌─ Plan · Verify · Proposal · Tasks ──────────────────╳─┐
│                                                       │
│  [ Graph ]   [ Kanban ]                               │
├───────────────────────────────────────────────────────┤
│                                                       │
│                                                       │
│                    (tab body empty)                   │
│                                                       │
│                                                       │
└───────────────────────────────────────────────────────┘
```

---

## 7. Composer — reuse existing

The composer is whatever the existing chat component ships today: textarea, send / stop, model picker, voice, attachments, streaming, queued-message stacking, etc. This spec does **not** change it, skin it, or add controls to it.

| Concern | Rule |
|---------|------|
| Composer | Reuse the existing chat-component composer verbatim |
| Branch picker | **Not part of this spec.** Do not add to the composer or anywhere in the chat pane |
| PR chip | **Not part of this spec.** Do not add |
| Working-tree / CI badges | **Not part of this spec.** Do not add |
| Future work | If branch/PR surfacing is needed later, it gets its own spec that explicitly adds it — this spec is silent on the topic |

---

## 8. Top navbar — additive change only

> The navbar stays **100% as it is today**. No items are removed, reordered, renamed, or restyled. The only change is **one new link: `Agents`**, added to the navbar's link group.

| Change | Detail |
|--------|--------|
| ❌ Removals | None. Ideation, Graph, Kanban, Insights, Settings, project select — all untouched. |
| ❌ Reorders | None. Existing order is preserved. |
| ❌ Rewrites | None. Icons, labels, styles, click handlers of every existing link stay as-is. |
| ✅ Addition | A single new `Agents` link, appended to the existing link group. |

### Navbar before (unchanged)

```
┌────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┐
│ ⬤⬤⬤  ◂ ▸   Ideation · Graph · Kanban · Insights · Settings · Project ▾                              ⌕ Search         👤  │
└────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┘
```

### Navbar after (one link added)

```
┌────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┐
│ ⬤⬤⬤  ◂ ▸   Ideation · Graph · Kanban · Insights · Settings · Project ▾ · [ ◧ Agents ]            ⌕ Search         👤  │
└────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┘
                                                                               ↑
                                                                          only new link
```

### Agents link behavior

| Concern | Rule |
|---------|------|
| Placement | Appended at the end of the existing link group (do not insert between existing items) |
| State | Active while the Agents view is visible; idle otherwise — same active/idle styling as every other navbar link |
| Click | Toggles the Agents 3-pane layout into the main content area |
| Deep-link | `ralphx://agents` opens the app with the Agents view active |
| Keyboard | `⌘⇧A` toggles Agents view (chosen to avoid collisions with existing shortcuts) |
| Label / icon | Text `Agents` with a lightweight leading glyph (`◧`) that mirrors the artifact-split visual |

### What lives inside the Agents view

Every tab below stays exactly where it is in the navbar. Inside the Agents view, the artifact pane **mounts the existing component** so the same screen is reachable via two entry points (no duplicate code).

| Artifact tab inside Agents view | Reused component (shipping today) |
|-----------------------------------|------------------------------------|
| Plan | existing `PlanView` from the Ideation tab |
| Verify | existing `VerificationView` from the Ideation tab |
| Proposal | existing `ProposalView` from the Ideation tab |
| Tasks — Graph | existing `TaskGraphView` from the Graph tab |
| Tasks — Kanban | existing `KanbanBoard` from the Kanban tab |

---

## 9. Empty & edge states

### 9.1 No projects yet (first launch)

```
┌─────────────────────────────────┐
│ Projects             ＋   ⌕    │
├─────────────────────────────────┤
│                                 │
│                                 │
│                                 │
│      No projects yet.           │
│                                 │
│      A project groups your      │
│      chats, tasks, and repo.    │
│                                 │
│   [  ＋  Create first project ] │
│                                 │
│                                 │
│                                 │
└─────────────────────────────────┘
```

### 9.1b Project with no chats

```
│  ▾ 📁 reefbot.ai        ＋ ⋯  │
│     No chats yet.  ＋ Start    │   ← inline helper row
│                                 │
│  ▾ 📁 ralphx-core       ＋ ⋯  │
│     ...                         │
```

### 9.2 No session selected

```
┌──────────────────────────────────────────────────────────────────────────────┐
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
│              Pick a conversation from the sidebar                            │
│              or start a new one.                                             │
│                                                                              │
│                           [  ＋  New agent  ]                                 │
│                                                                              │
│                                                                              │
└──────────────────────────────────────────────────────────────────────────────┘
```

### 9.3 Agent running — composer locked

```
                                                                   🎙     [ ■ ]
                                                                           ↑
                                                              stop-agent button
```

Input stays editable; queued messages are shown as a stacked pill above composer with a small "queued — sends after current turn" hint.

---

## 10. Behavior notes

| Concern | Rule |
|---------|------|
| Navbar scope | Navbar is **read-only** for this spec — nothing is removed, reordered, renamed, or restyled. The only edit is appending one `Agents` link. |
| Agents toggle | Clicking the `Agents` navbar link shows the 3-pane layout; clicking any other navbar link returns the app to that link's existing screen |
| Agents persistence | Last sidebar-selected session and artifact tab are remembered across Agents-view toggles |
| Project ownership | Every chat belongs to exactly one project; assigned at creation and immutable (move = duplicate, not reassign) |
| New agent gate | Top `＋ New agent` always opens the dialog; a session cannot be created without project + provider selection |
| Fast path | `＋` on a project row skips the picker and creates a chat in that project |
| Project focus | Clicking a project name scopes the artifact-panel Tasks view; it does **not** switch the active chat |
| Artifact panel state | Persists per session (last-open tab remembered) |
| Sidebar expand/collapse | Persists per project per user |
| Keyboard | `⌘⇧A` toggles Agents view; `⌘1..4` focuses a chat-pane artifact toggle; `⌘\` toggles split; `⌘N` opens New agent; `⇧⌘N` opens New project |
| Responsive < 1024px | Artifact pane overlays chat instead of splitting |
| Ideation entry | `＋ New agent` always creates a chat — no "new ideation"; plan emerges from the first turns |
| Plan approval gate | `Approve plan` in the Plan tab unlocks `Proposal` and `Tasks` tabs |
| Chat body | Reuse the existing chat component inside the middle pane — messages, composer, model picker, streaming, attachments unchanged |
| Running agents | `◐` badge in the sidebar row + count badge on collapsed project; clicking row jumps to the live turn |
| Project delete | Archiving hides sessions from the sidebar; hard delete requires typing the project name to confirm |
| WKWebView theming | All canvas tokens for the new panes use literal colors per `.claude/rules/wkwebview-css-vars.md` |

---

## 11. Agent creation & External MCP orchestration

> In the Agents view, a **session is an agent**. Creating a session spawns a provider process (Claude or Codex) configured with a scoped External-MCP endpoint. The provider orchestrates RalphX by calling MCP tools that bind ideations, plans, proposals, and tasks to its own session. The artifact pane renders those bindings through the existing `PlanView` / `VerificationView` / `ProposalView` / `TaskGraphView` / `KanbanBoard` components — no new UI logic.

### 11.1 Principles (NON-NEGOTIABLE)

| # | Principle |
|---|-----------|
| 1 | **Parity over duplication.** External MCP (:3848) exposes the same tool surface as Internal MCP (stdio → :3847). Handlers live once, in the Tauri backend — both transports call them. |
| 2 | **Session-scoped.** Every External-MCP call is scoped to exactly one chat session via a bearer token issued at agent-creation time. Tools auto-bind artifacts to that session; agents cannot mutate other sessions. |
| 3 | **Provider-agnostic.** Claude and Codex are interchangeable providers. The session, token, MCP endpoint, and artifact-binding behavior are identical across providers. |
| 4 | **Reuse, don't rebuild.** No new artifact stores, no new data models. Plans / ideations / proposals / tasks write through existing repo + service layers. |
| 5 | **Token is the session.** Revoking the token closes the session's MCP access; regenerating one ends the old agent's authority immediately. |

### 11.2 Wire diagram

```
   ┌──────────────────────────┐      ┌──────────────────────────┐
   │  Agent (Claude CLI)      │      │  Agent (Codex / GPT-5.4) │
   │  .mcp.json → HTTP MCP    │      │  tools config → HTTP MCP │
   └──────────┬───────────────┘      └──────────────┬───────────┘
              │  Bearer <session-token>             │  Bearer <session-token>
              │  (Streamable HTTP / SSE)            │
              ▼                                     ▼
   ┌──────────────────────────────────────────────────────────┐
   │              ralphx-external-mcp  ( :3848 )              │
   │   auth → session-scope middleware → tool dispatcher      │
   └──────────────────────────┬───────────────────────────────┘
                              │  HTTP (loopback)
                              ▼
   ┌──────────────────────────────────────────────────────────┐
   │              Tauri backend HTTP  ( :3847 )               │
   │   ┌── same handlers used by Internal stdio MCP ──┐       │
   │   │  plan_service · ideation_service · tasks …  │       │
   │   └──────────────────────────────────────────────┘       │
   │   repos · SQLite (ralphx.db) · event bus                 │
   └──────────────────────────────────────────────────────────┘
```

**Key invariant:** the tool dispatcher in External MCP forwards to the **same backend HTTP handlers** the Internal MCP uses. No business logic duplicated at the MCP layer.

### 11.3 Session-token lifecycle

| Event | Effect |
|-------|--------|
| User clicks `＋ New agent` and confirms | Backend creates `agent_session`, mints `token = hmac(session_id, secret)`, returns `(session_id, token, mcp_url)` |
| UI spawns provider | Provider process is launched with env `RALPHX_MCP_URL` + `RALPHX_MCP_TOKEN` |
| Provider calls any MCP tool | External MCP resolves token → session → injects `session_id` into the tool call before forwarding to :3847 |
| User clicks stop / closes session | Backend revokes token; in-flight MCP calls fail with `401` + close-code |
| Session resumed from sidebar | Same `session_id`, **new** token minted; old token is dead |

The token never appears in the chat transcript and is never rendered in the UI after creation.

### 11.4 External-MCP ↔ Internal-MCP parity matrix

Every tool the Internal MCP exposes today must be reachable via External MCP with identical semantics. The External-MCP layer injects `session_id` from the token so the agent doesn't pass it explicitly.

| Tool group | Existing tools | Internal MCP | External MCP (required) |
|------------|----------------|--------------|-------------------------|
| Ideation | `create_task_proposal`, `update_task_proposal`, `create_plan_artifact`, `update_plan_artifact` | ✅ | ✅ parity |
| Chat-task | `update_task`, `add_task_note`, `get_task_details` | ✅ | ✅ parity |
| Chat-project | `suggest_task`, `list_tasks` | ✅ | ✅ parity |
| Worker | `get_task_context`, `get_artifact*`, `*_step`, `execution_complete` | ✅ | ✅ parity |
| Reviewer | `complete_review`, `get_task_context` | ✅ | ✅ parity |
| Merger | `report_conflict`, `report_incomplete`, `get_merge_target`, `get_task_context`, `complete_merge` | ✅ | ✅ parity |
| Session binding (NEW) | `bind_plan`, `bind_ideation`, `bind_proposal`, `list_session_artifacts`, `set_active_artifact` | — (implicit via stdio session) | ✅ explicit tools |
| Discovery | `list_projects`, `get_project`, `list_sessions`, `get_session` | ✅ | ✅ parity |

> **"Session binding" tools** are the only new additions. They exist because External MCP needs explicit artifact-to-session attachment that the Internal stdio MCP gets implicitly from its launcher-passed context. Every other row is a straight transport-layer mirror of existing tools — **no new handlers**.

### 11.5 Session-binding tool contract

The five new session-binding tools — the only new server-side work this section authorizes — follow one shape:

| Tool | Input | Effect |
|------|-------|--------|
| `bind_plan` | `{ plan_id }` | Marks the plan as belonging to the caller's session; emits `session.plan.bound` event that the artifact pane's Plan tab subscribes to |
| `bind_ideation` | `{ ideation_id }` | Same for an ideation artifact; powers the Verify tab |
| `bind_proposal` | `{ proposal_id }` | Same for a task proposal; powers the Proposal tab |
| `set_active_artifact` | `{ kind, id }` | Moves a bound artifact to "currently displayed" so the artifact-panel wrapper knows which to render first |
| `list_session_artifacts` | `{}` | Returns `{ plans[], ideations[], proposals[], tasks[] }` scoped to the caller's session |

All five **dispatch into existing services** (`plan_service.attach_to_session`, etc.). If a service method is missing for an attach operation, add the service method — do not put the logic in the MCP tool.

### 11.6 Provider adapters

Two providers, one protocol.

| Concern | Claude | Codex |
|---------|--------|-------|
| Launcher | Claude Code CLI via plugin (`claude --plugin-dir ./plugins/app`) | Codex runner (OpenAI Agents / CLI equivalent) |
| MCP config | `.mcp.json` entry with `type: "http"`, `url: $RALPHX_MCP_URL`, `headers: { Authorization: "Bearer $RALPHX_MCP_TOKEN" }` | Provider-native HTTP-MCP config — same URL + bearer |
| Models | `claude-opus-4-7`, `claude-sonnet-4-6`, `claude-haiku-4-5-20251001` | `gpt-5.4`, subsequent GPT-5.4 variants |
| System prompt | Seeded from existing RalphX agent prompts (`agents/ralphx-ideation/...`) | Seeded from the same prompts — XML-like structure per `docs/ai-docs/openai/gpt-5.4-prompting.md` |
| Tool naming | `mcp__ralphx__*` prefix (Claude convention) | Provider-native naming — tools are the same MCP tools either way |
| Termination | `SIGINT` → agent graceful shutdown | same |

Model-selection defaults follow CLAUDE.md's team-management rule: default Sonnet-class, escalate to Opus-class / GPT-5.4 for deep investigation. The Agents view shows the default per-project and lets the user override per session in §3.1.

### 11.7 Create-agent flow end-to-end

```
 User              UI (Agents view)          Backend                 Provider
  │                     │                       │                       │
  │── click ＋ New ────▶│                       │                       │
  │                     │── POST /agent ──────▶ │                       │
  │                     │                       │── mint token ────┐    │
  │                     │                       │  allocate session│    │
  │                     │                       │  bind project   ◀┘    │
  │                     │                       │                       │
  │                     │◀── (session_id,       │                       │
  │                     │     token, mcp_url) ──│                       │
  │                     │── spawn provider ─────────────────────────────▶│
  │                     │   (env: MCP_URL,                               │
  │                     │         MCP_TOKEN)                             │
  │                     │                       │◀── MCP: list_session_artifacts
  │                     │                       │    bind_plan, etc.    │
  │                     │                       │── artifact events ───▶│
  │                     │◀── artifact events ───│                       │
  │                     │   (Plan/Verify/Proposal/Tasks tabs hydrate)    │
  │── send message ────▶│── POST /chat ────────▶│                       │
  │                     │                       │── stream to provider ▶│
  │                     │◀────── stream tokens ◀────────────────────────│
  │                     │                       │                       │
```

No UI polling. The artifact tabs hydrate from `session.plan.bound` / `session.ideation.bound` / `session.proposal.bound` events over the existing event bus.

### 11.8 Security & scoping rules

| Rule | Detail |
|------|--------|
| Token-to-session binding | One token ↔ one session_id. Tokens are opaque, 256-bit, not guessable. |
| Session-only mutation | Every binding tool verifies the artifact being attached belongs to the same project as the session; cross-project attachment returns `403`. |
| Rate limits | External MCP's existing rate limiter (`rate-limiter.ts`) applies per-token; agent sessions inherit a per-session bucket. |
| Tool allowlist per provider | Defined per-provider in config; the sidebar-side spawn passes the allowlist via CLI args — no runtime privilege escalation. |
| Revocation | Backend `revoke_agent_token(session_id)` must immediately invalidate the token and drop any open streams. |
| Audit | Every External-MCP call logs `(session_id, tool, outcome, duration)` into the existing activity log (rule 6 of project CLAUDE.md). |

### 11.9 Implementation touchpoints

| Area | Change |
|------|--------|
| `plugins/app/ralphx-external-mcp/src/tools/` | Ensure every tool that exists in `plugins/app/ralphx-mcp-server/src/` (ideation, plan, step, issue, support, etc.) has a parity entry. Missing ones are the only net-new MCP-layer code. |
| `plugins/app/ralphx-external-mcp/src/auth.ts` | Add session-token verification; map token → `session_id` for injection |
| `plugins/app/ralphx-external-mcp/src/backend-client.ts` | Forward `session_id` on every internal HTTP call to `:3847` |
| `src-tauri/src/commands/agent_commands.rs` (new or extended) | `create_agent_session`, `revoke_agent_token`, `list_agent_sessions` — thin commands over existing services |
| `src-tauri/src/application/agent_session_service.rs` | Orchestrates provider spawn, token mint, binding cleanup — **service layer, not MCP layer** |
| `frontend/src/components/agents/NewAgentDialog.tsx` | The dialog in §3.1 |
| `frontend/src/stores/agentSessionStore.ts` | Subscribes to `session.*.bound` events, feeds artifact-panel wrapper |

### 11.10 Anti-scope

- ❌ No new Plan / Verification / Proposal / Tasks UI components. The existing screens render whatever the session has bound.
- ❌ No new data model for "agent" distinct from `chat_session` — an agent **is** a session with a provider + token.
- ❌ No per-tool handler rewrites for External MCP. It forwards; it does not compute.
- ❌ No ad-hoc provider plumbing outside `agent_session_service` — every provider launch flows through one service.

---

## 12. Open questions

1. Should the artifact pane support a **4th column** (Plan + Tasks side-by-side) for wide monitors, or keep it single-tab-per-session to avoid cognitive load?
2. Where do **multi-session reviews** (comparing 2 agents) live now that the "right project select chat reviews" is gone — a dedicated tab in Insights, or a compare action on sidebar rows?
3. Branch / PR surfacing inside the chat pane is deliberately out of scope — when we revisit, does it belong in the composer, the sidebar row, or the artifact panel?
4. For projects with no active agent, should Insights open full-screen (takeover) or still render inside the chat pane region?
5. Should the Codex adapter proxy via **AI Gateway** for provider failover, or hit OpenAI directly for session-lifetime determinism? (§11.6)
6. Token revocation UX — silent (just kill the agent) or prompt the user to confirm on "stop"? (§11.3 / §11.8)
7. Do we expose a **rotate-token** action in the UI for long-running agents, or is rotation purely session-lifecycle driven? (§11.8)
8. For multi-turn agents that go idle for hours, should the External-MCP token TTL be shorter than the session TTL with auto-refresh, or equal? (§11.3)
