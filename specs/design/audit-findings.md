# Theme Audit Findings — 2026-04-18

Source: `.artifacts/theme-audit/{dark,light,high-contrast}/*.png` captured by `frontend/tests/visual/theme-audit/theme-audit.spec.ts` at commit `a9ca2a971`.

## Summary

- Views reviewed: 9 (activity, chat, extensibility, graph, ideation, kanban, reviews, settings, task-detail)
- Themes: dark / light / high-contrast
- Critical issues: 5
- High issues: 7
- Medium issues: 6
- Dominant pattern: ~19 components hardcode `bg-[hsl(220_10%_...)]` tailwind classes that never flip; floating overlays (Reviews, ideation sidebar, graph filter stack) also hardcode dark HSLA backgrounds inline.

## Per-View Findings

### Activity
- **Dark:** Clean. Filter chips, search field, tab strip all read correctly. Brand mark orange bleed-over on the "Activity" nav item is the intended active state.
- **Light:** Mostly clean. Loading spinner is still the dark-theme orange tint (reads OK on white). Active "Activity" nav pill is orange pill on white — OK.
- **High-Contrast:** Nav active state correctly flips to yellow (`#FFDD00`). Activity icon tile (top-left) flips to yellow outline — good. Loading spinner remains orange (should be yellow under HC).
- **Recommended fixes:**
  - Route the loading spinner color through `var(--accent-primary)` so HC gets yellow, not orange. Likely in the generic `Spinner` / `LoadingIndicator` component under `frontend/src/components/ui/`.

### Chat
- **Note:** all 3 "chat" captures show the Kanban empty-state page, not the dedicated Chat view. The spec selector for `/chat` currently resolves to the Kanban split-layout fallback (no plan selected). Treat "chat" view as **unverified** — rerun with an explicit route to the Chat panel (see Known Gaps).
- **Dark / Light / HC:** empty-state file icon with yellow sparkles reads on all three. HC correctly tints the icon tile yellow; Light keeps the peach-tinted tile (good).

### Extensibility
- **Dark:** Clean — tab strip with underline on active "Workflows", workflow card reads well, `DEFAULT` chip has a pale background.
- **Light:** Mostly clean. The workflow card background is pale gray on white, workflow bullet dot stays orange — OK. Active tab underline visible. `DEFAULT` chip looks almost invisible — uppercase "DEFAULT" text on a near-white chip with no border.
- **High-Contrast:** Active-tab underline is white (correct). `DEFAULT` chip flips to dark with white border (good). Workflow bullet dot flips to yellow — good.
- **Recommended fixes:**
  - Light-theme `DEFAULT` chip needs a visible border or darker bg. Likely a shared `<Badge variant="muted">` token pulling `--bg-hover` which is too close to `--bg-elevated` in light. Bump contrast in `src/styles/themes/light.css`.

### Graph
- **Dark:** Clean — left-side floating filter stack (Status / Vertical / Standard / Plan + Tier) has dark translucent bg, empty-state info icon and "Select plan" button read well.
- **Light:** CRITICAL — the floating filter stack (Status, Vertical, Standard, Plan + Tier) stays a DARK translucent panel on a white page; labels and chevrons are unreadable. The "No plan selected" icon + text read OK.
- **High-Contrast:** Same panel is dark on black. Shapes blend into the page; borders almost invisible. Chevrons visible, but the group is unreadable as a cohesive control.
- **Recommended fixes:**
  - `frontend/src/components/TaskGraph/controls/GraphControls.tsx` — lines ~193, 221, 279–289, 371, 383, 399, 407, 414–415, 432–433 hardcode `bg-[hsl(220_10%_10%)]` / `bg-[hsl(220_10%_12%)]` / `bg-[hsl(220_10%_15%)]` / `bg-[hsl(220_10%_10%_/_0.9)]` and matching borders. Replace with `bg-[var(--bg-elevated)]` / `bg-[var(--bg-hover)]` / `border-[var(--border-subtle)]` so they flip.
  - Same pattern exists in `TaskGraph/controls/FloatingGraphFilters.tsx`, `GraphLegend.tsx`, `groups/PlanGroupHeader.tsx`, `groups/PlanGroup.tsx`, `groups/TierGroup.tsx`, `groups/TierGroupHeader.tsx`, `groups/PlanGroupSettings.tsx`, `nodes/TaskNode.tsx`, `nodes/TaskNodeCompact.tsx`, `timeline/ExecutionTimeline.tsx` (all found via `rg 'bg-\[hsl\(220' frontend/src/components`).

### Ideation
- **Dark:** Clean. Left "Plans" sidebar, centered lightbulb hero, mode chips (Solo active with orange outline), orange "Start New Session" CTA, and bottom status bar all read well.
- **Light:** HIGH — the left plans sidebar stays dark gray while main canvas is white. Search input, "No plans yet" empty state, and sidebar body appear to sit on the dark-theme `--bg-secondary` but the main canvas is already `--bg-base` white. The bottom execution status bar (Running: 0/3 ...) also remains dark-theme colored. Orange "New Plan" CTA and orange "Start New Session" CTA correctly stay orange (brand); Solo chip has orange outline on peach fill — OK.
- **High-Contrast:** Sidebar flips to black with subtle borders — good. CTAs correctly flip to solid yellow. Mode chip "Solo" selected state is yellow outline — good. Lightbulb hero tile flips to yellow — good. Bottom status bar correctly goes solid black with readable white labels.
- **Recommended fixes:**
  - Ideation sidebar container (left rail with "Plans / Search sessions / No plans yet") — find the wrapping component under `frontend/src/components/Ideation/` (likely `PlanBrowser.tsx` or its parent layout) and verify the root `<aside>` uses `bg-[var(--bg-secondary)]` rather than a hardcoded gray. The sidebar flips correctly on HC but not Light, implying the Light token `--bg-secondary` may be set but is too close to `--bg-base` — easier to use `--bg-elevated` for the rail.
  - `ExecutionControlBar` (bottom status bar) — remains dark on Light. Confirm it uses `--bg-elevated` / `--bg-base` via theme tokens (not a hardcoded `bg-neutral-900` / `bg-[hsl(220_...)]`). If the component is `frontend/src/components/execution/ExecutionControlBar.tsx`, audit its root container className.

### Kanban
- **Dark:** Clean. Columns (BACKLOG / READY / EXECUTING) with group headers, task cards with feature/Backlog chips, progress bar for Executing task, right-side chat rail all read.
- **Light:** HIGH issues:
  - Executing Task's progress bar retains its dark-theme gradient fill (orange→blue) — this actually reads on white but the dark background stripe of the track is still dark.
  - The send-message button at the bottom-right (paper-plane icon) is a dark square with a faint icon — should flip to light surface or white-on-orange.
  - "Pause" button in the bottom bar has readable text but its hover/pressed background stays dark.
- **High-Contrast:** Mostly clean — cards flip to black-on-white text. BUT:
  - "Ready Task" card retains a blue left border (status accent stripe). Blue on black reads, but HC theme should use yellow or white, not a cold blue.
  - "Executing Task" card's left border is a warm orange (should likely become yellow for HC).
  - The "Feature" chip retains an orange icon on HC — per the rule, accent bleed should be replaced by yellow in HC.
  - Progress bar fill is yellow on HC — good.
- **Recommended fixes:**
  - Progress-bar track color — likely in `TaskCard.tsx` or `ProgressBar.tsx`; ensure track uses `var(--bg-hover)` and fill uses `var(--accent-primary)`.
  - Status-stripe color for card-left-border: map `status → color` to use `var(--status-info)` / `var(--status-success)` tokens that have HC overrides, not a literal `#3b82f6` blue or `#ff6b35` orange.
  - Send-message composer button: likely `Chat/MessageInput.tsx`. Confirm its background uses `--bg-elevated` and the icon uses `--text-primary`.

### Reviews
- **Dark:** Clean. Floating frosted-glass panel with "Reviews" header, All/AI/Human tabs, empty-state disc + "No pending reviews" read well.
- **Light:** CRITICAL — the entire Reviews floating panel renders as a dark translucent rectangle on a white page, making the tab labels, counts, and empty-state all low-contrast ghosts. The floating panel never flipped.
- **High-Contrast:** Panel is black with thin borders; tab labels and counts read OK. Empty-state disc icon is dark gray on black — barely visible; should be white-ish on HC.
- **Recommended fixes:**
  - `frontend/src/App.tsx:1140-1149` — the floating Reviews wrapper hardcodes `background: "hsla(220 10% 10% / 0.92)"`, `border: "1px solid hsla(220 20% 100% / 0.08)"`, and heavy dark shadow. Replace with theme-token-driven inline style (e.g. `background: "var(--bg-elevated-translucent)"` with theme files defining that token) OR move this styling to a CSS class that branches via `[data-theme='light']` / `[data-theme='high-contrast']`.
  - Empty-state icon in `ReviewsPanel` should use `var(--text-muted)` / `var(--text-secondary)` so HC gives visible white disc.

### Settings
- **Dark:** HIGH — the selected sidebar item ("Execution") renders as a bright white/near-white pill with dark text. This looks inverted — everywhere else the selected state is orange-tinted. All other category items read properly.
- **Light:** Clean. Selected "Execution" pill is orange on white, correct. Input fields (Max Concurrent Tasks, Project Ideation Cap) have subtle borders and readable values. Close (X) button has a subtle peach outline — fine.
- **High-Contrast:** Selected item flips to solid yellow with black text — good. Input fields have white borders on black — readable. Close (X) button has yellow outline — good. Execution icon tile is dark with yellow lightning icon — good.
- **Recommended fixes:**
  - Dark-theme selected-nav-item bg is wrong. Likely the Settings sidebar item uses a component whose `data-state=active` style resolves to `--color-white` / `--bg-base` inversion. Check `frontend/src/components/settings/SettingsSidebar.tsx` (or equivalent) — expected: `data-[state=active]:bg-[var(--accent-primary)]/15 data-[state=active]:text-[var(--accent-primary)]` OR a subtle elevated bg, not solid white.

### Task Detail (Kanban split view)
- **Dark:** Clean. Task title, feature/Backlog chips, description, empty "No steps defined yet", and right-side Task chat with "Ask about this task..." composer all read.
- **Light:** MEDIUM — "No steps defined yet" placeholder is light gray on near-white, borderline unreadable. "Ask about this task..." placeholder reads a bit faintly. Send button in composer is a dark peach tile with orange icon — low contrast vs white bg.
- **High-Contrast:** Clean — all text reads white-on-black. "No steps defined yet" placeholder correctly appears as dim gray, still legible. Send composer button flips to white icon on black — good.
- **Recommended fixes:**
  - "No steps defined yet" — should use `var(--text-muted)` not a fixed gray. Likely in the component that renders the empty steps list (`AgentsTaskDetailPanel` or `StepList` subcomponent).
  - Send button background: same fix as Kanban composer above.

## Cross-View Patterns

1. **Hardcoded `bg-[hsl(220_10%_...)]`** — 19 files use this pattern (confirmed via `rg 'bg-\[hsl\(220' frontend/src/components`). All of them will stay dark regardless of theme. Affected files:
   - `TaskGraph/controls/GraphControls.tsx`
   - `TaskGraph/controls/FloatingGraphFilters.tsx`
   - `TaskGraph/controls/GraphLegend.tsx`
   - `TaskGraph/groups/PlanGroupHeader.tsx`
   - `TaskGraph/groups/PlanGroup.tsx`
   - `TaskGraph/groups/TierGroup.tsx`
   - `TaskGraph/groups/TierGroupHeader.tsx`
   - `TaskGraph/groups/PlanGroupSettings.tsx`
   - `TaskGraph/nodes/TaskNode.tsx`
   - `TaskGraph/nodes/TaskNodeCompact.tsx`
   - `TaskGraph/timeline/ExecutionTimeline.tsx`
   - `tasks/TaskFormFields.tsx`
   - `tasks/TaskFormFields.constants.ts`
   - `tasks/TaskCreationForm.tsx`
   - `tasks/TaskCreationOverlay.tsx`
   - `agents/task-details/AgentsTaskDetailOverlay.tsx`
   - `Chat/ConversationStatsPopover.tsx`
   - `ui/ResizeHandle.tsx`
   - `Team/TeammatePane.test.tsx`

2. **Hardcoded inline `hsla(220 ...)` in App.tsx** — the Reviews floating wrapper at `App.tsx:1140-1149`. Same class of problem.

3. **Dark-shadow bleed** — `boxShadow: "0 4px 16px hsla(220 20% 0% / 0.4)..."` on Reviews panel reads as a heavy pool of black on Light/HC. Needs theme-aware shadow token.

4. **Accent bleed in HC** — orange accents still leak in HC for:
   - Feature chip icon on Kanban cards
   - Executing card left status-stripe (orange)
   - Ready card left status-stripe (blue)
   - Activity loading spinner
   These should map to `var(--accent-primary)` (yellow on HC) or status-token overrides.

5. **Status bar (bottom) doesn't flip on Light** — visible in Ideation and partially in Kanban. `ExecutionControlBar` is suspect.

6. **"Muted" text tokens too close to bg on Light** — "No steps defined yet" and the `DEFAULT` chip are both near-invisible. Light theme's `--text-muted` / `--bg-hover` may need more separation.

## Priority Fix List (sorted by severity)

1. **CRITICAL — Reviews panel** (all themes except Dark): hardcoded dark glass panel in `App.tsx:1140-1149`. Thousands of pixels of the Light view are an opaque dark rectangle. Fix: route bg/border/shadow through theme tokens or CSS class.
2. **CRITICAL — Graph floating filter stack** (Light + HC): `TaskGraph/controls/GraphControls.tsx` hardcoded `hsl(220 10% ...)` tokens. Replace with `var(--bg-elevated)` / `var(--bg-hover)` / `var(--border-subtle)`.
3. **CRITICAL — Ideation left sidebar** (Light): sidebar does not flip, stays dark gray on white canvas. Needs `bg-[var(--bg-elevated)]` or similar on the `<aside>` root.
4. **CRITICAL — Execution control bar** (Light): bottom status bar across Kanban/Ideation remains dark. Likely component `execution/ExecutionControlBar.tsx` — audit root container for theme tokens.
5. **CRITICAL — Dark theme Settings selected item**: renders as bright white instead of accent-tinted. Audit `SettingsSidebar` active-state classes.
6. **HIGH — Kanban status-stripe colors in HC**: Ready (blue) and Executing (orange) card left borders should switch to yellow or a neutral HC-safe token.
7. **HIGH — Send / composer buttons** (Light): Kanban + Task Detail composer buttons are dark peach tiles with orange icon on white canvas. Flip to `--bg-elevated` + `--text-primary` icon OR `--accent-primary` solid bg with white icon.
8. **HIGH — Feature chip icon stays orange in HC**: chip icons should resolve to `var(--accent-primary)` (yellow) on HC.
9. **HIGH — Progress bar track** (Light): Executing Task progress bar track retains dark color. Track uses wrong token.
10. **HIGH — Status-stripe / progress gradient tokens**: all hardcoded color gradients (orange→blue) should be theme-aware.
11. **HIGH — TaskGraph nodes/groups hardcoded colors** (10+ files): every TaskGraph subcomponent uses `bg-[hsl(220_...)]`; if any graph data is present these will all look wrong on Light/HC.
12. **HIGH — Activity loading spinner in HC**: should be yellow, not orange.
13. **MEDIUM — Light theme "DEFAULT" chip invisibility**: add border or bump bg contrast in `light.css` or chip token.
14. **MEDIUM — "No steps defined yet" text in Light**: uses too-light gray on near-white; tighten `--text-muted` or switch to `--text-secondary`.
15. **MEDIUM — "No pending reviews" empty-state disc in HC**: dark gray disc on black barely visible.
16. **MEDIUM — Shadow tokens**: heavy black shadows in floating overlays look wrong on Light / wash out on HC. Add theme-aware `--shadow-elevated` token.
17. **MEDIUM — TaskCreation* + ConversationStatsPopover + ResizeHandle**: same `hsl(220_...)` hardcoding audit.
18. **MEDIUM — Solo/Research/Debate chips backgrounds**: mode chips look OK but worth verifying active/inactive backgrounds use theme tokens (not hardcoded peach fill).

## Known Gaps

- **Chat view was not captured correctly** — all 3 "chat" screenshots actually show the Kanban empty-state page. Rerun `theme-audit.spec.ts` with an explicit route or state that forces the Chat panel to render with content.
- **Insights view was not captured** (nav button gated by taskCount ≥ 10 in mock data).
- **Reviews panel fixtures are empty** — only empty-state was captured. Status badges (green success, red error, yellow warning) in real review cards were not observed across themes. Schedule a follow-up with seeded review data.
- **Kanban columns with many task states not captured** — only Backlog / Ready / Executing columns with 1 card each. Review, Merge-conflict, Failed, Approved states not seen.
- **Modal/overlay variants** — Accept modal, confirm dialogs, proposal edit modal, team artifact drawer — none captured; these are high-risk for dark-on-light scrims.
- **Task detail specialized views** (ExecutionTaskDetail, ReviewingTaskDetail, MergeConflictTaskDetail, etc.) — only BasicTaskDetail captured for a backlog task.

---

## 2nd Pass — post-fix validation (2026-04-18, commit 991fbe444)

Fresh captures at `.artifacts/theme-audit/{dark,light,high-contrast}/*.png`. Reviewed against the 18-item Priority Fix List above.

### Fixed

- **P1 — Reviews floating panel (Light / HC):** resolved — panel now flips to light translucent background on Light, black with subtle border on HC, dark translucent on Dark. Tab labels, counts, and empty-state disc all legible across themes. Fix at `App.tsx:1140-1149` (`color-mix(var(--bg-surface) 92%, transparent)` + `var(--border-subtle)` + `var(--shadow-md)`) is visually confirmed.
- **P2 — Graph floating filter stack (Light + HC):** resolved — filter pills (Status / Vertical / Standard / Plan + Tier) are readable on white in Light and black in HC. Container bg now uses `var(--bg-elevated)` family; pill dividers use `var(--border-subtle)`.
- **P3 — Ideation left sidebar (Light):** resolved — "Plans / 0 plans" rail flips to `var(--bg-elevated)` light surface on Light, black on HC, dark on Dark. "New Plan" CTA correctly stays orange (Dark/Light) / yellow (HC). "No plans yet" empty state is legible on all themes.
- **P4 — Execution control bar (Light / Ideation + Kanban):** resolved — bottom status bar ("Running / Queued / Ideation / Merging" + Pause/Stop) now flips to light in Light theme. On HC the bar is solid black with yellow Pause pill. No longer stuck dark.
- **P6 — Kanban status-stripe colors in HC (Executing orange):** resolved — Executing card left border is now yellow on HC (via `status-icons.ts` mapping executing → `accent-primary` which is yellow in HC).
- **P9 — Progress bar track / fill (Executing task, Light):** resolved — progress bar fill is orange gradient in Dark/Light and yellow in HC; track color reads correctly against the card bg on all themes.
- **P11 — TaskGraph hardcoded `bg-[hsl(220_...)]` in 10+ subcomponents:** resolved at class level — no graph data was rendered to verify visually, but the filter container (the only visible TaskGraph surface in these captures) now flips. Given the 16-file migration mentioned in the fix wave, the underlying class-level problem is cleaned up; revisit when graph has seeded nodes.
- **P12 — Activity loading spinner in HC:** resolved — spinner ring is yellow on HC, orange on Light, orange on Dark. All three themes correct.
- **P17 — TaskCreation* / ConversationStatsPopover / ResizeHandle hardcoded HSL:** resolved at class level (per reported 16-file migration). Not visually verifiable from these captures (none of those surfaces were seeded); trust the code migration.

### Still broken

- **P5 — Dark theme Settings selected nav item:** unfixed — `dark/settings.png` still shows "Execution" as a bright white pill with dark text (visually inverted). Should be accent-tinted (`var(--accent-primary)/15` bg + accent text) or subtle elevated bg. Next step: audit `SettingsSidebar` / whatever renders `data-state=active` and override the default shadcn sidebar active state, which is leaking `bg-primary-foreground` (near-white) on Dark.
- **P7 — Send / composer button (Light, Kanban + Task Detail):** partial — the composer "send" button is now a peach tile with orange paper-plane icon on white. It's readable at a glance but still a warm low-contrast tile rather than a crisp filled orange CTA. Next step: switch to `bg-[var(--accent-primary)] text-white` for the active/enabled state on Light so the send button mirrors the "Start New Session" CTA pattern.
- **P8 — Feature chip icon stays orange in HC:** partial — on HC Kanban, the "Feature" chip's doc icon on the Backlog card still renders orange (see `high-contrast/kanban.png`, "Additional Task 1" card). Ready/Executing card chips appear lighter/yellowed. Suggests the feature-type icon color still has a hardcoded `#ff6b35`/orange fill rather than going through `var(--accent-primary)`. Next step: grep `Feature` / `feature` icon component (likely `TaskTypeBadge` or `TaskTypeIcon` under `components/tasks/`) for hardcoded orange.
- **P13 — Light-theme `DEFAULT` chip invisibility:** unfixed — `light/extensibility.png` still shows "DEFAULT" text barely visible on the near-white workflow card chip. Token contrast too close to `--bg-elevated` on Light. Same issue visible on Dark (chip background is nearly invisible against the dark workflow card). Next step: add a border to the `<Badge variant="muted">` or bump `--bg-hover` contrast in `light.css` (and possibly dark too).
- **P14 — "No steps defined yet" text in Light:** partial — still appears as very light gray on near-white in `light/task-detail.png`, though the surrounding task-detail area now flips correctly. The text itself needs `var(--text-muted)` → `var(--text-secondary)` upgrade on Light specifically.
- **P6 — Kanban status-stripe colors in HC (Ready blue):** unfixed — Ready card still has a blue left border and blue play-icon on HC. Blue on black reads, but per HC rule the stripe should be yellow or neutral white. Status-icons migration mapped `ready → status-info` which is blue; HC override for `--status-info` needs to resolve to white or yellow. Next step: in `high-contrast.css` set `--status-info` to a HC-safe value (white or pale yellow) rather than inheriting the blue.
- **P15 — Empty-state review disc in HC:** partial — the "No pending reviews" disc icon on HC reviews panel is still a dim gray circle that's barely distinguishable from the black panel bg. Marginally better than before but still low-contrast. Next step: the disc should use `var(--text-muted)` which on HC should be closer to `#A0A0A0`, not the current near-black.
- **P16 — Shadow tokens (floating overlays):** partial — Reviews panel shadow now routes through `var(--shadow-md)` (good in Dark), but on Light the drop shadow under the panel is still a cool dark cast on a warm white bg (visible as a faint gray edge around the floating rectangle). HC shows no shadow, which is correct. Acceptable, low priority.
- **P18 — Solo/Research/Debate mode chip backgrounds:** unfixed — on Light ideation, the "Solo" selected chip is peach-filled with orange outline (readable, but should likely be a more prominent `bg-[var(--accent-primary)]/10` pattern with no separate peach fill). Minor — lower priority than the others.

### New issues surfaced

- **`DEFAULT` chip also weak on Dark:** pre-existing but more visible now — `dark/extensibility.png` shows the DEFAULT chip almost blending into the workflow card. Same root cause as Light. Raise this as a cross-theme chip contrast problem, not just Light-only.
- **Kanban Light bottom "Stop" button:** the Stop button next to Pause (`light/kanban.png`) has a slightly faded dark chip treatment — not broken but noticeably flatter than the Pause button next to it. Possible disabled-state styling leaking into the enabled look.
- **Task-detail header action icons (edit/archive/duplicate/close):** on Light `task-detail.png` and Dark `task-detail.png` the inline icons in the top-right header (pencil, archive, duplicate) render as faint gray glyphs, nearly blending into the bar. The active "X" close button has a visible peach outline treatment. Consider tightening icon-button default contrast or adding hover states that flip the bg.
- **Kanban HC Feature chip icon inconsistency:** confirmed from close inspection — feature chip icons are inconsistently colored across cards in the same view (Backlog orange, Ready/Executing near-yellow). Either a state-based color (which is wrong) or a random mix of two different components. Not previously noted.

### Updated Priority List (top 5)

1. **P5 — Dark theme Settings sidebar selected item** renders as bright white pill on dark bg. Clear visual regression; highest-visibility default sidebar state is wrong. Fix: override shadcn sidebar `data-state=active` token to use accent-tinted bg on Dark.
2. **P6b — HC Ready status stripe is blue, not HC-safe.** Fix: in `high-contrast.css` set `--status-info` to white or pale yellow (or have the Kanban stripe component use `--text-primary` / `--accent-primary` specifically in HC).
3. **P8 — Feature chip icon orange in HC (Backlog card).** Fix: locate the feature-type icon component (likely in `components/tasks/TaskTypeBadge` or similar) and route its color through `var(--accent-primary)` rather than a hardcoded orange, so HC resolves to yellow.
4. **P7 — Light-theme send/composer button is a peach tile with orange icon, not a crisp CTA.** Fix: `bg-[var(--accent-primary)] text-white` on the send button, matching the "Start New Session" pattern.
5. **P13 — `DEFAULT` chip invisibility in both Light and Dark** (Extensibility workflow card). Fix: add a 1px `border-[var(--border-subtle)]` + bump chip bg to `var(--bg-surface)` so it shows on both themes.

Deferred / lower priority (for future pass):
- P14 "No steps defined yet" gray-on-white
- P16 floating panel shadow on Light
- P15 Reviews empty-state disc on HC
- P18 Ideation Solo chip peach fill
- New: Task-detail header icon button faintness
- New: Kanban Light Stop button flatness

---

## 3rd Pass — post-fix validation (2026-04-18, commit c6bdcaa70)

Fresh captures at `.artifacts/theme-audit/{dark,light,high-contrast}/*.png`. Reviewed against the 2nd-pass residual list and the 9 fixes reported as landed.

### Fixed

- **P6b — HC Ready status stripe (blue → yellow):** resolved. `high-contrast/kanban.png` now shows Ready card with a clear yellow left stripe + yellow play-icon. `--status-info` remap to yellow-500 is visually confirmed on the Ready card, and flows through to the card stripe component without any cold-blue bleed. Executing stripe is also yellow, as expected.
- **P7 — Chat composer send button on HC:** resolved. `high-contrast/kanban.png` bottom-right send button renders as a darker tile with a yellow paper-plane icon that inverts correctly. Disabled state flattens to bg-hover, enabled state pops. Same pattern on `high-contrast/task-detail.png` composer.
- **P7 (Light) — Kanban + Task Detail composer send buttons:** partial → still not a crisp filled CTA. Light captures still show a peach tile with orange icon (not the "Start New Session" style filled-orange treatment). No regression, but the 2nd-pass gripe remains — see Still Broken.
- **P8 — Feature chip icon orange bleed on HC:** resolved. All three Kanban cards in `high-contrast/kanban.png` (Backlog / Ready / Executing) now render the feature chip icon in yellow, consistent across the column. No more mixed orange-on-Backlog vs yellow-on-other inconsistency.
- **P13 — `DEFAULT` chip on Light + Dark (Extensibility):** resolved. Both `light/extensibility.png` and `dark/extensibility.png` now show the "DEFAULT" chip with a visible border and a distinct darker/lighter fill. HC keeps its white outline. Chip contrast problem cleared across all three themes.
- **P14 — "No steps defined yet" on Light:** resolved. `light/task-detail.png` shows the placeholder in a readable secondary-gray (muted → secondary bump is visible). Dark/HC also read normally.
- **P15 — Reviews empty-state disc on HC:** partial → resolved. `high-contrast/reviews.png` disc now reads as a mid-gray dashed circle against black. Not bright, but unambiguously visible and no longer near-invisible. Accept as resolved.
- **New-from-2nd — Task-detail header action icons (edit/archive/duplicate):** resolved. `dark/task-detail.png` and `light/task-detail.png` show the header icons at a clearly readable contrast now (muted → secondary + hover text-primary upgrade is visible). The close (X) still has its peach outline — intended active-state treatment.
- **Settings dialog solidity on Light:** resolved. `light/settings.png` renders the settings modal as a solid white surface — no more see-through effect. The `var(--bg-elevated)` swap landed cleanly.
- **Shadow scale softening on Light:** resolved. `light/ideation.png` and `light/graph.png` show the floating panels (execution control bar, graph filter stack) with subtle warm-tinted drops instead of the prior harsh near-black halos. Ideation bottom status bar edge is barely there — good.
- **ExecutionControlBar shadow on Dark/Light:** resolved. No longer a double-overlay scrim; uses `--shadow-md` + `--border-subtle`. Reads correctly across Dark/Light/HC.
- **Theme switching (bootstrap / data-theme attribute):** implicitly verified — captures for all three themes render distinctly. Dark is dark, Light is light, HC is black-and-yellow. No cross-theme bleed.
- **ChatInput test fix:** out of visual scope — trust the reported fix.

### Still broken

- **P5 — Dark theme Settings selected nav item** (unfixed for a THIRD pass): `dark/settings.png` still shows "Execution" as a bright near-white pill with dark text — visually inverted vs every other active state in the app (orange-tinted elsewhere, solid yellow on HC, orange-filled on Light). This is the single most glaring regression left. Likely cause: the Settings sidebar button uses a shadcn default `data-state=active` token that resolves to `bg-background` (near-white) on Dark, rather than the accent-tinted pattern used in the main app nav. Next step: override `SettingsSidebar` (or equivalent) active-state class to `bg-[var(--accent-primary)]/15 text-[var(--accent-primary)]` on Dark specifically, or switch to the shared nav-pill component that already has the correct Dark treatment.
- **P7 (Light) — Send/composer buttons remain a peach tile with orange icon:** `light/kanban.png` and `light/task-detail.png` composer send buttons still render as a soft peach square with an orange paper-plane icon. It's legible but doesn't read as a primary CTA — the "Start New Session" orange-filled pattern sets the expectation. Lower priority than P5 but unchanged from 2nd pass. Next step: `bg-[var(--accent-primary)] text-white` on the enabled send-button state (matching the Ideation primary CTA), or keep the subtle treatment but make the icon color route through `--text-inverse` / `--bg-elevated-hover` for better contrast.
- **P18 — Ideation Solo mode chip on Light:** unchanged. `light/ideation.png` still shows Solo as a peach-filled chip with orange outline vs the white-filled outline-only Research/Debate chips. Pattern inconsistency between selected/unselected mode chips. Low priority.
- **P16 (Light) — floating panel shadows:** partial/accepted. Shadows are now soft enough that this is no longer jarring on Light, but still reads as a cool-ish gray halo rather than a warm-tinted drop. The shadow-scale softening helped — treating this as closed at current severity.

### New issues surfaced

- **Light-theme Kanban card border contrast:** `light/kanban.png` — the Backlog card ("Additional Task 1") shows only the shadow as a visual separator against the white column; there's no visible border. Compared to the Ready/Executing cards (which get their distinct left stripe), the Backlog card looks unframed. Minor — card hierarchy relies on shadow alone on Light.
- **HC Reviews panel close/split icons (top-right of floating panel):** `high-contrast/reviews.png` — the numeric count badge "(0)" and the two small header icons (the 0-badge and the collapse-toggle) render at extremely low contrast. The panel itself is clean but those top-right affordances nearly disappear. Was marginally present in 2nd pass but more noticeable now that the panel itself is fixed.
- **Light-theme Stop button still flatter than Pause:** `light/kanban.png` — the Stop button next to Pause reads as a slightly lighter gray chip vs Pause's clearer border. Same observation as 2nd pass, still unresolved. Could be disabled-state styling leaking into the enabled look, or a hover-state color that's too close to base.
- **Tier/focus outlines on HC Kanban cards:** `high-contrast/kanban.png` — the Ready and Executing cards show subtle yellow outlines that could be mistaken for focus rings. Expected since status stripe → yellow, but the whole-card yellow perimeter edge reads as "keyboard-focused" — worth verifying that real focus rings don't stack with this treatment.
- **Light theme Kanban sidebar "Task" rail on right (chat-rail empty state):** the "Start a conversation" empty-state treatment is very light gray on near-white. Legible but low-emphasis. Similar to P14 pattern — muted-gray text on light-gray background — could benefit from a small contrast bump.

### Updated Priority List (top 3)

1. **P5 — Dark theme Settings sidebar selected item** remains bright-white pill instead of accent-tinted. Three passes unfixed. Single highest-visibility visual regression in Dark. Fix: override shadcn sidebar `data-state=active` token specifically on Dark to use `bg-[var(--accent-primary)]/15 text-[var(--accent-primary)]`, or route the whole item through the same shared nav-pill component used by the top bar's Ideation / Graph / Kanban active state.
2. **P7 (Light) — Composer send button treatment** is an orange icon on a peach tile instead of a filled accent CTA. Now the last remaining "doesn't look like a button" spot on Light. Fix: filled-orange bg + white icon on enabled state (match Start New Session pattern) OR keep subtle but swap icon color to `var(--text-primary)` on Light for crisper glyph contrast.
3. **New — HC Reviews panel top-right badge + toggle low contrast**: header chrome on the floating Reviews panel disappears on HC. Fix: route the "(0)" count badge and the collapse-toggle icon through `var(--text-secondary)` (which HC already biases toward white-ish), not the current near-black muted value.

Deferred / lower priority (for next pass):
- P16 Light floating panel shadow tint (accepted, monitor)
- P18 Ideation Solo chip peach fill
- New: Light Kanban Backlog card lacks visible border (shadow-only separator)
- New: Light Kanban Stop vs Pause button weight mismatch
- New: HC Kanban card yellow perimeter readable-as-focus-ring ambiguity
- New: Light Kanban right rail "Start a conversation" empty state contrast


