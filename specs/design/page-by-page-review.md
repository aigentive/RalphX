# Per-Page Theme Review — 2026-04-18 (commit 66a6fbde8)

> Evaluation of 24 screenshots (8 views × Dark / Light / High-Contrast) captured via the Settings → Theme switch audit flow. Target baseline: `specs/design/styleguide.md`, `specs/design/accessibility.md`, `specs/design/themes/light.md`.

## Styleguide anchor points (summary)

- **Token tier discipline:** Components consume semantic (`--bg-surface`, `--text-primary`, `--accent-primary`) or component-tier tokens only. No primitives, no raw rgba/hex.
- **Surfaces:** Dark `--bg-base` hsl(220 10% 8%) → `--bg-surface` 12% → `--bg-elevated` 16% → `--bg-hover` 20%. Light flips to 99% / 97% / 100% / 92%. HC collapses to `#000000` with `#FFF` borders.
- **Typography:** SF Pro. H1 20–24/600, section title 14/600, body 14/400, eyebrow 11/600 uppercase tracking 0.08em.
- **Accent:** Orange only (`--accent-primary`: `#FF6B35` Dark, `#EA5A26` Light, `#FFDD00` HC). Used sparingly on primary CTAs, focused borders, active tab pills.
- **Shadows:** Tahoe-subtle. Light must use low-alpha black drop shadows (0.05–0.10). ❌ Harsh black halos on white.
- **Spacing:** 8pt grid. Card inner `p-5` (20 px). Dialog header `px-4 py-3`.
- **Radii:** 4 / 8 / 12 / 16 / full. HC collapses `--radius-lg` to 8 px.
- **Focus:** Always visible ring via `--focus-ring`; 2 px Default, 3 px HC.
- **Status vocabulary:** Icon + text + color — color never alone.
- **Active nav:** Default → white bg / `#0A0A0A` text. Light → `--accent-primary` bg / white text. HC → `#FFDD00` bg / black text.

---

## Page reviews

### Ideation

**Layout**
- Three-region: top nav bar (`h-14` approx), left rail Plans sidebar (~220 px), main empty-state column, bottom status bar.
- Empty-state is vertically centred; lightbulb tile → title → description → "IDEATION MODE" eyebrow → 3 mode pills → primary CTA → two ghost actions → kbd hints stack with consistent ~16 px gaps. Rhythm is clean.
- Bottom status bar full-width with 24 px horizontal padding, grouped stats on left, Pause/Stop on right.

**Typography**
- H2 "Ideation Studio" renders at ~20/600, body paragraph at 14/400 `--text-secondary`, eyebrow "IDEATION MODE" at ~11/600 uppercase (matches spec tracking). Hierarchy is consistent across all three themes.

**Components**
- Pill group (Solo / Research Team / Debate Team), primary CTA (Start New Session), ghost buttons (Seed / Import), kbd chips, sidebar "New Plan" CTA, Plans header tile, status bar chips with info icons, Pause/Stop split buttons.
- "Solo" selected pill uses `--accent-muted` bg + 1 px `--accent-border`. Consistent across all themes (orange tint → orange on Light → yellow tint on HC).

**Color tokens**
- Surfaces flip cleanly: Dark navbar `--bg-surface` vs Light near-white vs HC pure black. `--text-primary` / `--text-secondary` / `--text-muted` stay distinct on all 3.
- Accent usage correct on pill border + "New Plan" CTA + active "Ideation" nav pill.

**Shadows/elevation**
- Bottom status bar sits flush on the base surface; no elevation seam. Fine on Dark/HC. On Light it reads as a near-white band with only a subtle top hairline — acceptable but flat.

**Accuracy vs spec**
- Light `--bg-hover` (`hsl(220 10% 92%)`) on the "New Plan" CTA-ish ghost chip is not visible here because there is no hover state captured; worth verifying live.
- HC "Start New Session" renders solid yellow with black glyph + text — matches spec `#FFDD00` on black + `--text-inverse`.
- `kbd` chips on HC keep the dark inset look (light outline) — acceptable but slightly low-contrast; consider `--bg-elevated` + 2 px white border.

**Polish opportunities**
- The bottom status bar's leading grey dot has no status text (just "Running: 0/3"). Consider pairing with an icon shape (the paused pill uses `Pause` glyph correctly, so this is consistent).
- The sidebar "Plans" tile icon sits in an `--accent-muted` box on Dark/Light but turns yellow-tinted on HC — correct cascade.
- On Light, the "New Plan" CTA drops a slight shadow that reads as a dark halo; inspect `--shadow-xs` value vs `PrimaryCTA` wrapper.

---

### Graph

**Layout**
- Top nav, left floating filter panel (`Status / Vertical / Standard / Plan + Tier` group), centred empty state ("No plan selected" + Select plan button). Rest is open canvas area.
- Floating filter panel is ~140 px wide, positioned at `top-48 left-4` approximately. It hangs as an island with rounded corners and a subtle border — correct elevated-card treatment.

**Typography**
- "No plan selected" H2 at 18–20/600, description 14/400 muted, filter labels 13/500. Consistent in all themes.

**Components**
- Filter panel = stacked `Select` triggers with chevron-right icons on "Status" and "Plan + Tier". "Vertical" and "Standard" buttons appear as plain rows (no chevrons) — likely toggle/direction indicators.
- "Select plan" pill with info icon + chevron.

**Color tokens**
- Filter panel uses `--bg-elevated`. On Light it sits as white on near-white with only `--border-subtle` separating — acceptable but loses hierarchy.
- HC: panel is pure black with 2 px white border — clean.

**Shadows/elevation**
- Dark: subtle `--shadow-xs` makes the filter panel feel detached. Light: no perceptible shadow — the panel is only distinguishable by a hairline border. This is the correct Tahoe/Light approach but risks looking unglued.

**Accuracy vs spec**
- Filter panel follows Section card pattern (rounded `lg`, `--bg-elevated`, `--border-subtle`, `--shadow-xs`).
- On Light, the empty-state icon's warning-circle outline uses `--text-muted` — reads fine (5.3:1).
- HC: nav bar's Chat / Reviews buttons inherit the black base — the Chat button's right side "⌘K" chip reads as a nested dark pill, but without an outline it blends. Consider an outline on kbd chips in HC.

**Polish opportunities**
- The floating filter panel's internal divider lines between option groups look uneven (Status has no underline, Vertical/Standard do). On Light this visual inconsistency is more pronounced since both rows share the same surface color.
- Nothing orients the user to where to click when a plan is loaded — consider promoting the "Select plan" button to a `PrimaryCTA` for stronger affordance.

---

### Kanban

**Layout**
- Top nav + search / "Select plan" / bar-chart icon row + main empty state column (centred lightbulb-doc tile, title, description, CTA, kbd hint) + right project chat side-panel (~320 px wide) + bottom status bar.
- The empty-state tile uses a 96×96 rounded-xl box with an accent-muted fill, a sparkle icon top-right, and a doc-lines glyph centred — nice bespoke empty state.

**Typography**
- H1 "No plan selected" 20/600, secondary caption 14/400 `--text-muted`, CTA chip 13/500, kbd 11/500.

**Components**
- Search input (full width with trailing clear ✕), Select-plan pill, chart-icon button, PrimaryEmptyState tile, Select plan CTA with info icon, kbd hint, right panel header ("Project" + History icon), message composer "Send a message…" + send icon.

**Color tokens**
- Kanban's empty-state tile has an accent-muted fill + 1 px accent-border. On Dark: warm orange glow. On Light: soft peach (good AA). On HC: yellow-tinted — correct.
- Right panel ChatInput placeholder uses `--text-muted`. On HC the placeholder is almost invisible ("Send a message…" is very faint) — needs bump to `--text-secondary` or `#B0B0B0` at minimum.

**Shadows/elevation**
- Right panel is separated by a vertical divider + faint shadow. On Light, the divider is a hairline `--border-subtle`; the send-icon button has a faint bg on Light that hints at a slightly mis-tuned `--bg-hover` / `--bg-elevated` pair.

**Accuracy vs spec**
- Bar-chart icon button in the filter row has no tooltip-visible `aria-label` hint in the screenshot; cannot verify but should map to `aria-label="Show metrics"`.
- PrimaryCTA "Pause" in the bottom status bar reads correctly in all 3 themes with icon + label.
- HC: the "Stop" button looks disabled (`opacity-50`) — consistent with Dark/Light at this state (no active agents).

**Polish opportunities**
- The search input's `--text-muted` placeholder "Search tasks…" on HC is close to the minimum 7:1 (`#B0B0B0` on `#000`). Check contrast on search vs Message compose; consider `--text-secondary` for placeholders in HC only.
- The empty-state doc glyph has an orange `sparkle` accent — on Light that sparkle turns yellow because it renders on a light bg with the `--status-warning` token. Verify it was intentionally scoped to `--accent-secondary` not `--status-warning` (see `EmptyStates.tsx`).
- Right-panel ChatInput send-button uses a different hover-bg shade from the Kanban PrimaryCTA — align to `--bg-hover`.

---

### Activity

**Layout**
- Top nav + "Activity" page title row with icon tile (`--accent-muted` bg) + Clear button right-aligned + search input row + tab filter row (All / Thinking / Tool Calls / Results / Text / Errors / System) + Status / Role / Task / Session filter selects right-aligned + Live/History toggle + centred loading spinner.
- Spacing is generous and even (`py-6`, `px-6`).

**Typography**
- H1 "Activity" ~20/600, tab labels 13/500, filter-select triggers 13/500. Clear CTA 13/500 muted. Loading copy 14/400.

**Components**
- Icon tile (28 px rounded-md, accent-muted), Clear ghost button, search input (full width, `--bg-surface`), tab strip (current = "All" with underline + filled bg), dropdown filters (4×), Live / History segmented toggle, Loader2 spinner.
- "All" active tab uses `--bg-elevated` pill + underline. The underline accent is `--accent-primary` — fine on Dark/Light. On HC it's yellow.

**Color tokens**
- Dark: search bar `--bg-surface`, tab strip `--bg-surface`. Readable.
- Light: search bar border reads crisply against the slightly-off-white main bg. Good.
- HC: search bar is pure black with white 2 px border — good, but the right-edge "History" button reads as inset without a strong bg. Verify `--bg-elevated` token coverage.

**Shadows/elevation**
- No elevated components here; all flush — correct.

**Accuracy vs spec**
- Tab underline on "All" should use `--accent-primary` token; verify the Dark screenshot shows the orange accent not muted.
- Loader2 spinner circle color on Light is `--accent-primary` (orange) — spec-compliant.

**Polish opportunities**
- The filter `Status / Role / Task / Session` triggers all show chevron-down at consistent positions — good.
- The Live (radio) / History (clock) toggle on Light reads as a plain light pair with no selected-state differentiation. Only "History" has the filled pill. Should match a proper segmented-control treatment (pill bg for the active segment, plain for the inactive). Check `ActivityView` tabstrip impl.
- Consider tightening the gap between the Clear button and the Activity title icon + label — the `Clear` right-sits with ~48 px empty space on Light (looks abandoned).

---

### Extensibility

**Layout**
- Top nav + tab strip (Workflows / Artifacts / Research / Methodologies) + main column with section title "Workflow Schemas" + right-aligned "+ New Workflow" CTA + workflow card list.
- Only one workflow card visible (`Default Kanban`). Card takes full available width minus page margins.

**Typography**
- Tab labels 13/500 with underline on "Workflows" active. H2 "Workflow Schemas" 16/600. Card title 15/600. Card meta (4 columns · Created Jan 2026) 12/400 muted.

**Components**
- Tabs (Radix), primary ghost "+ New Workflow" button, SectionCard with status dot + title + DEFAULT chip + description + meta row.

**Color tokens**
- DEFAULT chip uses `--bg-elevated` + 1 px `--border-default` + `--text-secondary` uppercase 11/600. On HC the chip sits on pure black with white 2 px border — matches spec.
- Status dot color is `--accent-primary` on Dark/Light (orange), `--status-warning` yellow on HC (correct cascade).

**Shadows/elevation**
- Workflow card has `--shadow-xs` + `--border-subtle` — reads fine on Dark. On Light it reads slightly flat — hard to distinguish "card" vs "page"; rely more on the border. Consider bumping `--shadow-xs` alpha 0.03 → 0.05 for Light.

**Accuracy vs spec**
- Tab underline present on "Workflows" — `--accent-primary` on Dark/Light, yellow on HC — correct.
- On HC, "+ New Workflow" button has no visible CTA treatment — it's plain text with icon. Spec calls for button containers even on HC. Verify `.icon-button-ghost` has an HC border override.

**Polish opportunities**
- Card's DEFAULT chip sits 12 px after the title; leading indicator dot is 6 px round + 8 px gap. Rhythm is tight.
- Card description has a lot of leading whitespace on the right; consider a two-column meta if future cards get more fields (Owner / Updated / Tasks Count).
- On HC the "+ New Workflow" button should gain a 2 px white border per spec to distinguish it as an interactive element — currently it's just underline-on-hover.

---

### Settings

**Layout**
- Modal dialog overlay + dialog body with left rail (~300 px) + main content panel.
- Left rail groups: GENERAL / WORKSPACE / IDEATION / ACCESS / PREFERENCES with items inside each.
- Main panel: header strip with icon tile + title + description + section body with `SettingRow` list.

**Typography**
- Dialog title 18/600, breadcrumb "/ Accessibility" 14/400 muted, group eyebrows "GENERAL" / "PREFERENCES" 11/600 uppercase tracking 0.08em, item labels 14/400.

**Components**
- Left rail nav items (active = "Accessibility"), SectionCard wrapper (icon tile + title + desc + separator + body), SettingRow × 4 (Theme select, High contrast switch, Motion select, Font size select), Close button (X), Switch component.
- Theme select trigger shows both the label and description stacked. This violates the Select rule: **"Triggers show ONLY the primary label (use SelectValue children). Descriptions belong in dropdown items — never truncated inside the trigger."** On Light: "Light / Near-white surfaces with dark" is truncated. On Dark: "Dark (default) / Warm-orange accent on blue" is truncated. On HC: "High contrast / WCAG AAA palette — yellow a…". This is a concrete styleguide violation.

**Color tokens**
- Dark active nav item: white bg + `#0A0A0A` text. ✅
- Light active nav item: orange bg + white text. ✅
- HC active nav item: yellow bg + black text. ✅
- All three match the `--nav-active-bg` / `--nav-active-text` token table in `accessibility.md §11`.

**Shadows/elevation**
- Dark/HC: dialog panel has a subtle drop. Light: the dialog has a light drop shadow, consistent with `--shadow-lg` in Light theme. Clean.
- HC: the dialog background is pure black and the 1 px white border is visible around the whole dialog — correct.

**Accuracy vs spec**
- **DEVIATION:** Theme select trigger breaks the "label only" rule — descriptions leak into the trigger and truncate. Reference `src/components/settings/AccessibilitySection.tsx` (or equivalent), the `Select` children should render only `SelectValue` pointing at `theme.label`.
- High-contrast toggle (switch) uses `--accent-primary` on Dark/Light (orange) and `--accent-warning-yellow` on HC — correct.
- Left rail group eyebrows render uppercase tracked — ✅.

**Polish opportunities**
- Replace multi-line Select triggers with label-only; move descriptions into the dropdown item's secondary line.
- The "Accessibility" header row uses a `--accent-muted` icon box with a wheelchair glyph. On HC the icon color is yellow on yellow-tinted bg — verify the HC override pushes icon fill to `#FFDD00` on `rgba(255,221,0,0.15)` so the icon remains visible (currently appears as faint yellow on dim yellow — check contrast).
- Breadcrumb "/ Accessibility" text color should match the bread-crumb hierarchy (current vs parent) — consistent on all three themes.

---

### Chat

**Layout**
- Chat overlay panel docked to right side, `~320` px wide, full height of viewport.
- Header: chat icon + "Chat" + right-side icon group (History / Snap-to-side / Close). Body: empty-state "Start a conversation" vertically centred. Footer: ChatInput with attachment icon + textarea + send button.
- Important: in the Chat screenshots, the Chat overlay is open on top of the Extensibility view. The Extensibility tab strip is visible behind but the "+ New Workflow" button is occluded — a floating-panel-without-scrim pattern.

**Typography**
- Panel header 14/600, empty-state 14/400 muted, send button icon-only.

**Components**
- Panel frame (rounded-xl on the top-left corner only on Dark/Light — verify; on HC it's full-rounded with 2 px border), icon buttons (History / Snap / Close), ChatInput (attachment + textarea + send).
- Composer send button has a faint `--bg-hover` bg on Dark/Light; on HC it reads as a white-glyph-on-black button with no outline.

**Color tokens**
- Panel bg uses `--bg-elevated`. Dark: matches nicely. Light: white on near-white — the panel only separates via a left-edge shadow. HC: black on black — panel is distinguished only by the white 2 px border.
- **Hover overlap:** The underlying "Chat" nav tab (top right) has `--accent-muted` background + accent border; this stays correct across themes.

**Shadows/elevation**
- Dark: `--shadow-md` on the panel left edge. Light: much lighter — correct. HC: no shadow, relies on border — correct.
- A visible bug on Light: the panel left shadow looks slightly darker than the spec's light-theme tuning. Verify `--shadow-md` Light value matches `0 4px 6px hsla(0 0% 0% / 0.08)`.

**Accuracy vs spec**
- **DEVIATION:** The `+ New Workflow` button beneath the Chat panel on Light leaks through the Chat panel's right edge at `x≈1190` — text is partially overlaid. Either the panel needs a stricter z-index/opacity or the underlying button should be offset when the chat docks open. On HC this leak is worse because the Chat panel has a transparent-ish left edge area; "New Workflow" text reads through the panel.
- Close icon (X) color uses `--text-secondary` on Dark/Light and `#E0E0E0` on HC — correct.

**Polish opportunities**
- Force the Chat panel to a solid `--bg-elevated` on HC (currently reads as if there's a subtle transparency letting the underlying Workflow card peek through) — inspect `ChatPanel.tsx` / `ResizeablePanel.tsx`.
- The send button's right-edge offset is ~16 px; the overall composer has 12 px padding. Align to a consistent 16 px.
- The empty-state "Start a conversation" lacks any affordance/icon; consider a small accent chat glyph (like Kanban's empty state) to make the empty screen feel more intentional.

---

### Reviews

**Layout**
- Reviews overlay docked right, similar dimensions to Chat (~340 px).
- Header: icon + "Reviews" + count badge (0) + Close (X). Tabs row: All (0) / AI (0) / Human (0). Empty-state centred: dashed-circle check icon + "No pending reviews" + "All reviews have been handled".

**Typography**
- Panel title 14/600, tab labels 13/500, empty-state heading 14/600, description 12/400 muted.

**Components**
- Panel frame, icon-only buttons, segmented-tab selector (3 options), dashed-circle icon (unique to empty reviews state), title, description.
- Segmented tabs don't clearly highlight the active one on Light — "All (0)" shows slightly deeper background. On HC: "All (0)" has a distinct black-on-white look — clearer.

**Color tokens**
- The "0" count in the header pill uses `--text-muted` — on HC it sits on black with a rounded dim-orange bg. Verify the badge's HC override.
- Dashed-circle icon uses `--text-muted` stroke — reads fine on all themes.

**Shadows/elevation**
- Panel: same as Chat. Light: light shadow; HC: border only.

**Accuracy vs spec**
- **CRITICAL DEVIATION:** The "+ New Workflow" button from the underlying Extensibility view is clearly visible overlapping the Reviews panel's right-edge header area across ALL three themes. See coordinate `x≈1147, y≈140` — "+ New Workflow" reads through. The Reviews panel header row (`Reviews` + `(0)` + `X`) is sandwiched visually with "+ New Workflow". This is a z-index / panel-opacity / layout leak.
- On Light and HC the leak is most egregious because both panels share the near-white / black palette.

**Polish opportunities**
- Fix the Reviews panel bg opacity — it should be fully `--bg-elevated`, not translucent. Likely `bg-[var(--bg-elevated)]/80` or similar in `ReviewsPanel.tsx`.
- Add proper z-index separation: the Reviews overlay should raise above any underlying view CTAs when open.
- The 3 tab pills `All / AI / Human` look like non-distinguishable text on Light — the active one is almost invisible. Ensure the active tab has a filled `--bg-elevated` + border + `--text-primary` per styleguide §7 Tabs spec.
- Count badge "(0)" in header uses a weird peach-on-orange-muted chip on Dark — verify it's actually rendering `--accent-muted` bg with `--accent-primary` text, not doubled accent.

---

## Cross-view patterns

**Recurring inconsistencies:**

1. **Panel overlay leak (Chat, Reviews).** Both right-docked overlays let the underlying view's top-right CTA bleed through. Shared root cause: either `--bg-elevated` resolves to a translucent value, or `z-index` is too low vs the underlying interactive buttons. Affects: `ChatPanel.tsx`, `ReviewsPanel.tsx`, probably via `ResizeablePanel.tsx`. Visible in all 6 screenshots (2 panels × 3 themes).

2. **Select trigger descriptions leaking.** Settings→Theme select shows `Label / Description` in the trigger, truncated. The styleguide §8 mandates trigger = label only; descriptions belong in dropdown items. Applies anywhere a `Select` is built with a subtitle. Audit: `AccessibilitySection.tsx` Theme/Motion/Font-size triggers.

3. **Segmented-control active state on Light.** Activity `Live/History`, Reviews `All/AI/Human`, and Kanban Chat-panel header tabs all look ambiguous on Light — active segment not strongly differentiated. Likely a `--bg-hover` vs `--bg-elevated` mix-up in the Light overrides. Check `ui/tabs.tsx` and any custom `SegmentedControl`.

4. **Icon-button ghost inconsistency on HC.** "+ New Workflow" on Extensibility and the send button in Kanban's chat sidebar look like plain text with only underline on hover. On HC every interactive affordance should gain a 2 px white border or a distinguishing bg per `themes/high-contrast.md`. Audit ghost button variants.

5. **Empty-state visual weight.** Ideation / Graph / Kanban / Reviews all use different empty-state patterns (bulb tile, warning-circle, doc-with-sparkle, dashed-circle-check). Reviews is the simplest; Kanban the most bespoke. Consider consolidating into a single `EmptyState` component with variants for `neutral|success|warning|info`.

6. **Shadow drift on Light.** Cards (Workflow schema card) and floating panels (Graph filter, Chat overlay) feel flat on Light because `--shadow-xs` is at alpha 0.03. Many cards rely on the shadow to feel elevated and instead look glued to bg. Bump `--shadow-xs` Light from 0.03 → 0.05 (still well under spec max 0.10).

7. **Placeholder contrast on HC.** Search inputs across views (Kanban "Search tasks…", Activity "Search activities…", Chat "Send a message…") use `--text-muted` placeholders. On HC the muted is `#B0B0B0` on `#000000` = 8.2:1 — technically passes AAA, but visually the placeholder reads as barely-there. Consider `--text-secondary` (`#E0E0E0`) for placeholders in HC only via a `placeholder:` variant.

8. **Accent icon color on tinted backgrounds in HC.** The Settings accessibility icon tile, the Kanban sparkle glyph, and the Ideation lightbulb tile all use `--accent-primary` on `--accent-muted`. In HC this becomes yellow on yellow-tinted = low visual contrast between glyph and container. Add `[data-theme="high-contrast"]` icon-fill override to push these to pure `#FFDD00` and drop the container tint.

9. **Status-bar `Pause` icon-button.** Consistent across themes — good precedent for how icon-buttons should render in the bottom persistent status bar. Use this as the reference for other icon-ghost buttons.

10. **Page-level padding inconsistency.** Ideation's empty state has ~56 px top padding, Graph's has ~220 px (filters push content down), Kanban's has ~200 px. Not a bug per se, but a consistent "empty-state vertical centering" helper would make the views feel more uniform.

---

## Priority polish list (top 10)

Ranked by visual impact × effort.

1. **Fix Chat/Reviews panel overlay opacity.** Both panels let underlying content bleed through. In `src/components/Chat/IntegratedChatPanel.tsx` and `src/components/reviews/ReviewsPanel.tsx` (around the outer container), ensure `bg-[var(--bg-elevated)]` is fully opaque — remove any `/80` or `/90` alpha on the container. Also raise panel `z-index` above the main view's top-right CTAs. (High impact, low effort)

2. **Settings Select trigger: label only.** In `src/components/settings/AccessibilitySection.tsx` (Theme/Motion/Font-size selects), change `<SelectTrigger>` children to render only the top-line label via `SelectValue`. Move `description` into `SelectItem` secondary text. Style fix is a 10-line diff per select. (High impact, low effort)

3. **Bump Light `--shadow-xs` from 0.03 → 0.05.** In `src/styles/themes/light.css`, edit the `--shadow-xs` token to `0 1px 2px hsla(0 0% 0% / 0.05), 0 1px 3px hsla(0 0% 0% / 0.03)`. Cards and floating filter panels will gain needed lift. (Medium impact, trivial effort)

4. **Segmented-control active state on Light.** In `src/components/ui/tabs.tsx` (or equivalent `SegmentedControl`), the active Trigger should render with `bg-[var(--bg-elevated)]` + border + `shadow-sm` per styleguide §7. Light theme currently reads as flat text. Update the `data-[state=active]:` classes. (Medium impact, low effort)

5. **HC icon-button ghost affordance.** Add a `[data-theme="high-contrast"] .btn-ghost, .icon-button-ghost { border: 2px solid var(--border-default); }` rule in `src/styles/themes/high-contrast.css`. Applies to "+ New Workflow" and the Kanban chat sidebar send button. (Medium impact, low effort)

6. **Accent icon on tinted bg in HC.** In `src/styles/themes/high-contrast.css`, add an override: `[data-theme="high-contrast"] .icon-tile { background: transparent; border: 2px solid var(--accent-primary); }`. Applies to Settings accessibility tile, Ideation lightbulb, Kanban doc tile. (Medium impact, low effort)

7. **Placeholder contrast in HC.** Add `[data-theme="high-contrast"] input::placeholder, textarea::placeholder { color: var(--text-secondary); }` in `src/styles/themes/high-contrast.css`. Fixes Kanban/Activity/Chat search placeholders reading as barely-there. (Medium impact, trivial effort)

8. **Reviews header count badge contrast.** In `src/components/reviews/ReviewsPanel.tsx` around `PanelHeader`, the `(0)` count pill currently uses an accent-muted bg. On Dark it reads as peach-on-black (fine); on Light it reads as peach-on-near-white (barely there). Switch the badge bg to `--bg-elevated` + `--text-secondary` text for a neutral count chip. (Low impact, low effort)

9. **Light theme `--shadow-md` on Chat panel.** In `src/styles/themes/light.css`, the Chat panel left-edge shadow reads as slightly harsh. Reduce to `0 4px 6px hsla(0 0% 0% / 0.06)`. (Low impact, trivial effort)

10. **Consolidate empty-state components.** Promote Kanban's `EmptyStates.tsx` pattern into a shared `src/components/ui/EmptyState.tsx` with `variant: neutral | info | success | warning` and consistent icon / title / description / CTA stack. Reviews / Graph / Ideation currently each rebuild the layout. (Lower visual impact, higher effort but pays off on future views)

---

## Design decisions audit — 2026-04-19

> Focused pass on **padding · shadows · borders · layout split** across the 8 captured views (24 screenshots). Builds on the 2026-04-18 review above; does not re-flag the panel-leak / Select-trigger / placeholder / HC icon-tile items that have since been resolved (confirmed in the current screenshots and `themes/high-contrast.css` overrides).

### Global patterns

#### Spacing (styleguide §3 — 8 pt grid)

| Observation | Where | Adheres to spec? |
|---|---|---|
| App header height `h-14` (56 px), left pad `pl-24` (96 px for window chrome), right pad `pr-4` (16 px) | `App.tsx:793` | ✅ Tahoe window-chrome convention |
| Dialog header `px-4 py-3` | `SettingsDialog.tsx:113` | ✅ matches §7 dialog shell spec |
| SettingRow `py-3 -mx-2 px-2` | `SettingsView.shared.tsx:69` | ✅ matches §7 setting row spec |
| Section card body `p-5` (Settings) | spec'd in styleguide §6 | ✅ |
| Right-dock panel outer container `margin: 8px` + `rounded-[10px]` inner | `App.tsx:1146`, `PlanBrowser.tsx:282`, `GraphSplitLayout.tsx:253` | ✅ consistent floating-glass rhythm |
| Ideation Plans sidebar inner pad `px-4 pt-4 pb-3` | `PlanBrowser.tsx:292` | ≈ 16/16/12 — half-step off 8 pt but justified for visual weight under the title |
| Review cards `p-4` (16 px) inside panel body `p-4 space-y-3` | `ReviewsPanel.tsx:86, 284` | ⚠ Drift — other cards in the app use `p-5` per §6 |
| Activity filters row `px-4 py-3 space-y-3` | `ActivityView.tsx:365` | ✅ |
| Extensibility tab strip trigger `px-4 py-2.5` | `ExtensibilityView.tsx:149` | ½-step — matches shadcn tabs default but the styleguide §7 tabs block specifies `px-3 py-1` for compact triggers |

Overall rhythm is **strongly aligned to 8 pt**. The two noticeable half-step deviations are (a) Reviews card padding (`p-4` vs canonical `p-5`) and (b) Extensibility tab trigger (`py-2.5`), both of which are inherited component defaults rather than deliberate choices.

#### Shadows (styleguide §5 — Tahoe-subtle)

| Theme | Tokens shipped | Net effect in screenshots |
|---|---|---|
| Dark | `--shadow-xs/sm/md/lg` at 0.2 alpha range (`semantic.css`) | Cards and dock panels feel lifted; accent icon tiles use separate `--shadow-glow-accent-*` tokens (not visible at rest here) |
| Light | Retuned to 0.05 / 0.04 / 0.05 alpha with warm blue-gray tint (`light.css:84–87`) | Very restrained — Workflow card, Graph filter island, Chat/Reviews dock all sit almost flush. Border does most of the work. |
| High-Contrast | Heavy 0.6–0.9 alpha tokens (`high-contrast.css:104–107`) | In practice invisible on pure-black surface — elevation is carried entirely by 2 px white borders. Tokens exist as defense in depth. |

**Assessment:**
- Dark/HC shadow strategy is **considered** — HC explicitly defers to border-based separation per `themes/high-contrast.md §1`; Dark uses subtle multi-stop.
- Light is **borderline too quiet** — in the Extensibility, Graph and Reviews screenshots the elevated card's `--shadow-xs` (2 tiny black stops at 0.03/0.05 alpha) is barely perceptible on `#F7F7F8` against `#FFFFFF`. The prior audit flagged this (item #3) and it is **not yet applied** — `--shadow-xs` is still at `0.05, 0.03`; the app reads slightly "glued" on Light. Bumping to `0.07, 0.04` is cheap.

#### Borders (styleguide §1, §3.theme-specific)

| Theme | `--border-subtle` / `--border-default` / width | Use pattern |
|---|---|---|
| Dark | `hsl(220 10% 18%)` / `hsl(220 10% 22%)` / 1 px | Card edges, dialog edges, tab underline (`--accent-primary` on active) |
| Light | `hsl(220 10% 88%)` / `hsl(220 10% 78%)` / 1 px | Same role split — subtle vs default — visible against near-white |
| HC | `rgba(255,255,255,0.5)` / `#FFFFFF` / **2 px** (`--border-width-default`) | Every card/input/dialog has a hard white edge; double-bump on focus to 3 px yellow |

**Assessment:**
- HC stroke-width bump is **correctly implemented** via `--border-width-default: 2px` in `high-contrast.css:67`, and `:focus-visible` uses the 3 px `--border-width-focus` yellow ring — matches `accessibility.md §4` requirement (3 px + 2 px offset for HC).
- Light's `--border-subtle` (#E0E2E6) carries most of the separation burden (since shadow is deliberately minimal). It works, but on dense views like Extensibility the single-card list feels slightly over-bordered — a shadow lift would let the border step down.
- Dark's "section underline" usage (bottom-bar divider, filter row divider) uses `--border-subtle`; the Activity view drops back to `--overlay-weak` in two places (`ActivityView.tsx:334, 365`) — a minor token inconsistency (`--border-subtle` vs `--overlay-weak`) on the same functional element.

#### Layout split conventions

The app uses **five** distinct layout archetypes, applied consistently per view intent:

| Archetype | Where | Shape |
|---|---|---|
| A. Right-dock floating panel | Chat (`App.tsx:1163`), Reviews (`App.tsx:1134`), Graph timeline/chat (`GraphSplitLayout.tsx`) | Fixed or resizable right column; wraps contents in a `rounded-[10px]` inner panel with `margin:8px`, `--bg-elevated` bg, `--border-subtle` 1 px border, `--shadow-md`. Shares the exact same wrapper across the three components → good. |
| B. Sidebar + main | Agents conversation/artifact split, Settings (280 px rail + scrollable pane) | Left nav, flex-1 content. Both use the same eyebrow (`uppercase tracking-wider`) + active-row treatment. |
| C. Centered empty state | Ideation (no session), Graph (no plan), Kanban (no plan), Reviews (0 pending) | Shared `<EmptyState variant="neutral">` in Reviews; Ideation/Kanban have bespoke versions. |
| D. Full-width horizontal bar | Execution control bar at bottom of Kanban/Graph/Ideation | Flush, not elevated. `h-14` (~56 px) with a top hairline `--border-subtle`. |
| E. Modal dialog | Settings (`95vw × 95vh`), proposal/review detail modals | Full-bleed modal; header `px-4 py-3`, main body split sidebar + pane. |

**Responsive behaviour:** Settings dialog has an explicit `lg` breakpoint — left rail collapses to a native `<select>` (`SettingsDialog.tsx:181`); most other views rely on `isNavCompact` from `useNavCompactBreakpoint`. Chat panel has a resizable width with persisted `localStorage.ralphx-chat-panel-width`. No full-screen-width responsive pivot is attempted (Tauri desktop-first).

---

### Per-section analysis

#### Ideation

- **Layout split** — Ideation is conversation-scoped inside Agents. The main conversation remains visible beside the Plan/Ideation artifact pane.
- **Padding** — Artifact content follows the shared Agents pane spacing and empty-state rhythm.
- **Borders and shadows** — Use the Agents artifact surface hierarchy; there is no standalone planning sidebar or root-level execution bar.
- **Assessment** — Planning now shares navigation, conversation context, and artifact ownership with the rest of Agents.

#### Graph

- **Layout split** — Archetype A + centered empty state. Floating filter panel (~140 px wide, rounded-lg, absolute positioned) + open canvas + a fixed 320 px timeline, with an overlay variant for compact width. Task detail is an Agents-owned artifact overlay.
- **Padding** — Filter panel items ~8 px vertical/8 px horizontal. Empty-state block has strong ~220 px top padding (the floating filters push content down).
- **Borders** — Filter panel uses `--border-subtle`; the divider between Status and Vertical rows is invisible on Light because the panel bg and the surface bg are both near-white.
- **Shadows** — Filter panel: `--shadow-xs`. On Light this is barely perceptible — the floating affordance is carried by border alone.
- **Assessment** — The floating filter panel layout is good; it's the **only view** using an absolutely-positioned floating control group.
- **Recommendations** —
  - On Light, bump filter panel shadow to `--shadow-sm` so the island feels detached.
  - Verify the filter internal divider token (currently inconsistent between Status/Vertical blocks).

#### Kanban

- **Layout split** — The Kanban board fills the Agents Tasks artifact; task selection opens the Agents-owned detail overlay.
- **Padding** — Search row `py-2 px-4`, empty-state tile 96×96.
- **Borders** — The board and detail overlay follow the Agents artifact surface hierarchy.
- **Shadows** — Empty-state tile has an accent-muted fill + 1 px accent border + the `--shadow-glow-accent-soft` ambient glow (visible as orange halo on Dark). On HC it renders as yellow tint on black box with a solid border.
- **Assessment** — The **most polished empty state** in the app. Tile dimensions (96 px) + sparkle icon + doc glyph + single-line title + CTA is a strong pattern worth promoting.
- **Recommendations** —
  - Promote Kanban's empty-state tile (`EmptyStates.tsx`) to shared `ui/EmptyState` with an optional `illustration` slot (Reviews already uses shared; Ideation and Graph still rebuild).

#### Activity

- **Layout split** — Single-column view with a sticky filter toolbar. Header (icon tile + "Activity" title + Clear) → search row + Live/History segmented toggle → filter tab strip + 4 dropdown filters → scrollable message area.
- **Padding** — Header `px-4 py-3`; filter row `px-4 py-3 space-y-3`. Message list `p-3 space-y-1.5`.
- **Borders** — Uses `--overlay-weak` for the filter-row divider (`ActivityView.tsx:334, 365`) instead of the more common `--border-subtle`. The two tokens differ in hue/alpha; visually the divider is slightly fainter here than in Settings or Ideation.
- **Shadows** — None in the header/filter row (correct; non-elevated).
- **Assessment** — Density is correct; rhythm between the three horizontal bands (title/search/filters) is good.
- **Recommendations** —
  - Align the horizontal-band border to `--border-subtle` (match Extensibility tab strip and Settings dialog header) — currently `--overlay-weak`. File: `ActivityView.tsx:334, 365`.
  - Segmented Live/History toggle renders active state as a filled pill; inactive state is flat. On Light the inactive pill is almost invisible (`History` text on `--bg-surface`). Consider adding a 1 px `--border-subtle` around both pills to match the Reviews FilterTabs pattern.

#### Extensibility

- **Layout split** — Full-bleed tabbed page: `<Tabs>` strip at top (h-11) + main column with section title (16/600) + right-aligned primary CTA + card list.
- **Padding** — Tab triggers `px-4 py-2.5` (inherited shadcn default), card content `p-5` (via SectionCard equivalent), workflow list item has ~20 px internal pad.
- **Borders** — Tab strip has a bottom `--border-subtle` divider + active trigger has a bottom `-mb-px` accent-colored `border-b-2`. Card uses `--border-subtle` 1 px.
- **Shadows** — Workflow card uses `--shadow-xs` per spec.
- **Assessment** — The tab + card-list pattern is the cleanest full-width pattern in the app. "+ New Workflow" CTA is tucked top-right and follows the PrimaryCTA outline style.
- **Recommendations** —
  - Tab trigger `py-2.5` is half-step off the 8 pt grid; align to `py-2` or `py-3` for consistency. File: `ExtensibilityView.tsx:149`.
  - On Light, the single workflow card's shadow is imperceptible — it hangs on the 1 px border alone. Bumping Light `--shadow-xs` globally (below) resolves this in one token change.

#### Settings

- **Layout split** — Archetype E (modal dialog 95vw × 95vh) + Archetype B inside (280 px left rail + scrollable pane, `lg` breakpoint collapses rail to `<select>`).
- **Padding** — Dialog header `px-4 py-3`, scrollable pane body `p-6` (24 px — the only place in the app using `p-6` at the outer layout level), `SectionCard` internal `p-5`, `SettingRow` `py-3 -mx-2 px-2`.
- **Borders** — Rail has a right `--border-subtle` divider, section card has `--border-subtle`, row dividers use `--border-subtle` with `last:border-0`.
- **Shadows** — Dialog uses `--shadow-lg`, section card `--shadow-xs`.
- **Assessment** — **Reference implementation** for the rest of the app — `SectionCard` + `SettingRow` + `InlineNotice` + `SavingIndicator` is a clean composable set.
- **Recommendations** —
  - Scrollable pane outer pad `p-6` (24 px) is higher than every other view's content pad (`p-4`/`p-5`). Consider `p-5` to match the shared card tier and give the rail content more horizontal room on narrower windows. File: `SettingsDialog.tsx:207`.

#### Chat

- **Layout split** — Archetype A right-dock. `ChatPanel` wraps `ResizeablePanel` with a 11-row header (`h-11 px-3`), optional team context bar, streaming tool indicator row, queued messages list, question banner, and input area at the bottom.
- **Padding** — Header `px-3`, messages area `flex-1` (no pad), input area `p-3` with queued messages `p-3 pb-0`.
- **Borders** — Header has `borderColor: color-mix(--text-primary 4%, transparent)` (a hardcoded translucent variant — **not a semantic token**, though it does flip with theme via `--text-primary`). Input area uses `border-t var(--border-subtle)`.
- **Shadows** — Panel is contained inside the floating `rounded-[10px]` wrapper with `--shadow-md`; panel itself carries no extra elevation.
- **Assessment** — The `color-mix(var(--text-primary) 4%, transparent)` pattern for header tint works (flips theme), but it sidesteps the `--overlay-faint`/`--overlay-weak` token family that exists for exactly this purpose.
- **Recommendations** —
  - Replace `color-mix(in srgb, var(--text-primary) 2–4%, transparent)` calls in `ChatPanel.tsx:500, 501` with `var(--overlay-faint)` / `var(--overlay-weak)` — same visual result, aligns with styleguide §1 "never hardcode translucent values in components."
  - Composer send-button right-edge offset (~16 px) vs composer padding (12 px / `p-3`) is a 4 px asymmetry — if intentional, document it; otherwise normalize to `p-3` on all sides.

#### Reviews

- **Layout split** — Archetype A right-dock, ~400 px wide, fixed position (`App.tsx:1136`). Header (h-11) + tab strip (h-auto `px-4 py-3`) + scroll area with cards.
- **Padding** — Card `p-4` (16 px); card list outer `p-4 space-y-3`. Inner description card `p-2 rounded-sm`.
- **Borders** — Card has `--border-subtle` 1 px + hover reveals a faint `border-white/10`. Tab strip uses the ReviewsPanel FilterTabs with data-state-active border.
- **Shadows** — Card gains `--shadow-xs` on hover (`isHovered && "shadow-[var(--shadow-xs)]"`) — interesting micro-interaction: card only lifts on hover. On Light the lift is barely perceptible due to `--shadow-xs` being so low-alpha.
- **Assessment** — The most notable inconsistency: **Review cards use `p-4` while every other card in the app uses `p-5` per styleguide §6**. Cards are also rendered inside a panel body that has `p-4` (16 px) — compare to Ideation's sidebar items which use a separate inner pad.
- **Recommendations** —
  - Align card inner pad to `p-5` per styleguide §6, `ReviewsPanel.tsx:86`.
  - CountBadge now correctly switches between accent-muted (non-zero) and neutral (zero) — good defense against prior Light-theme invisibility. Keep as-is.
  - The hover-only `--shadow-xs` lift is cute but inconsistent with cards in Extensibility (which carry `--shadow-xs` at rest). Pick one rule.

#### Task Detail (inferred — no direct screenshot)

Task detail views are owned by the Agents Tasks artifact (`AgentsTaskDetailOverlay.tsx` → `AgentsTaskDetailPanel.tsx`). The screenshots above don't capture an open task detail, so recommendations stay high-level. Per `.claude/rules/task-detail-views.md`, the registry maps 12 status-specific view components. Each should consume the same card tier (`--bg-elevated` + `p-5` + `--shadow-xs` + `--border-subtle`). A follow-up capture run with the Agents-owned `selectedTaskArtifactId` would be worth doing.

---

### Cross-section drift

| # | Pattern | Observed | Target (per spec) |
|---|---|---|---|
| 1 | Card inner pad | Reviews `p-4` (`ReviewsPanel.tsx:86`) | `p-5` per styleguide §6 |
| 2 | Horizontal-band divider token | Activity uses `--overlay-weak` (`ActivityView.tsx:334, 365`); Settings/Extensibility use `--border-subtle` | `--border-subtle` for structural dividers |
| 3 | Right-dock panel shadow tier | Ideation sidebar uses `--shadow-lg` (`PlanBrowser.tsx:287`); Chat/Reviews/Graph dock use `--shadow-md` | Align sidebar to `--shadow-md` (reserve `--shadow-lg` for root dialogs per §5) |
| 4 | Tab trigger vertical pad | Extensibility `py-2.5` (`ExtensibilityView.tsx:149`); Reviews FilterTabs `py-1.5`; Settings shadcn tabs `py-1` | Pick one — styleguide §7 specifies `py-1` for compact, `py-2` for default |
| 5 | Header tint pattern | Chat uses `color-mix(var(--text-primary) 2%, transparent)` inline; everywhere else uses `var(--overlay-faint)` | Use `--overlay-faint` / `--overlay-weak` tokens |
| 6 | Panel outer pad for Settings | Settings pane `p-6` (24 px) | Other views' content pads are `p-4`/`p-5`; consider `p-5` |
| 7 | Light theme card elevation | `--shadow-xs` Light: `0.05, 0.03` alpha — barely visible | Consider bump to `0.07, 0.04` (still < 0.10 max) |
| 8 | Card "lift on hover" vs resting | Reviews = hover only; Extensibility = resting | Align to resting `--shadow-xs` + hover `--shadow-sm` for affordance parity |

### Priority action list (top 10)

Ranked by user-visible impact × effort. Each item: **what**, where, why.

| # | Action | File(s) | Why (spec/UX rationale) |
|---|---|---|---|
| 1 | Bump Light `--shadow-xs` to `0.07, 0.04` alpha | `src/styles/themes/light.css:84` | Many cards feel glued on Light; Extensibility / Graph filter / Reviews all improve in one token change. Still well under 0.10 max. (Carried over from 2026-04-18 review item #3; still not applied.) |
| 2 | Align Reviews card pad to `p-5` | `src/components/reviews/ReviewsPanel.tsx:86` | Styleguide §6 card inner pad; Reviews is the only card tier at `p-4`. |
| 3 | Drop PlanBrowser shadow from `--shadow-lg` to `--shadow-md` | `src/components/Ideation/PlanBrowser.tsx:287` | Styleguide §5 reserves `--shadow-lg` for root dialogs; sidebars should use `--shadow-md`. Unifies right-dock elevation rules. |
| 4 | Swap Chat header `color-mix()` inlines for `--overlay-faint` / `--overlay-weak` | `src/components/Chat/ChatPanel.tsx:500, 501` | Styleguide §13 contribution rule: never hardcode translucent values in components. |
| 5 | Normalize Activity filter row divider to `--border-subtle` | `src/components/activity/ActivityView.tsx:334, 365` | Section divider should use the same token family as Settings/Extensibility. |
| 6 | Align Extensibility tab trigger to `py-2` (or `py-1` for compact) | `src/components/ExtensibilityView.tsx:149` | 8 pt grid; `py-2.5` is the only half-step padding in a tab strip. |
| 7 | Settings pane outer pad `p-6` → `p-5` | `src/components/settings/SettingsDialog.tsx:207` | Content pad tier for modal bodies; gives nav rail more room at narrow viewports. |
| 8 | Add 1 px `--border-subtle` hairline to Light Activity `Live/History` inactive pill | `src/components/activity/ActivityFilters.tsx` (ViewModeToggle) | Inactive pill currently invisible on Light. |
| 9 | Promote Kanban empty-state tile into shared `ui/EmptyState` variant | `src/components/tasks/EmptyStates.tsx` → `src/components/ui/empty-state.tsx` | Carried over from #10 prior review. Ideation/Graph still rebuild; consolidation reduces drift. |
| 10 | Make Reviews card `--shadow-xs` resting (not hover-only) | `src/components/reviews/ReviewsPanel.tsx:89` | Match Extensibility/Workflow card resting elevation for affordance parity. |

### Known limitations / open questions

| Limitation | Why |
|---|---|
| Hover states not captured | Screenshots are static; hover elevation / border color changes on Reviews cards, Chat toggle, and filter pills cannot be evaluated visually. |
| Focus rings not captured | No keyboard-focused element visible; AAA 3 px yellow ring in HC + 2 px blue in Default/Light unconfirmed by screenshot (code inspection shows `--focus-ring` + `:focus-visible` rules are wired, and `high-contrast.css:159–162` enforces the 3 px HC outline globally). |
| Task detail view not captured | No task selected in any screenshot, so `AgentsTaskDetailOverlay` + 12 state-specific detail views (`BasicTaskDetail` / `ExecutionTaskDetail` / etc.) were not visible. Recommend a follow-up capture with the Agents-owned `selectedTaskArtifactId`. |
| Dialog / modal stacks not captured | Proposal edit modal, finalize confirmation, verification confirm, permission dialog — all are spawned from `App.tsx` but not exercised by the theme-switch spec. |
| Live motion / animations | Pulsing accent glow, drop-zone edges, status pulse — static screenshots can't verify the animation tokens (`--shadow-pulse-*`, keyframes). |
| Settings dialog `lg` collapse | Mobile collapse (to a native `<select>`) was not captured — spec only fires at viewports below `lg` (1024 px); audit was at desktop widths. |
