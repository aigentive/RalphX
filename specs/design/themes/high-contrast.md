# High-Contrast Theme

> **Purpose:** Accessibility-first theme for users with low vision, color-blindness, or who enable macOS *Increase contrast*. Not a cosmetic variant — it is a full remap of the palette plus shape/size reinforcements so every UI state is distinguishable without relying on hue.
>
> **Targets:** WCAG 2.1 **AAA** — 7:1 normal text, 4.5:1 large text / UI components.
>
> **Related:**
> - `specs/design/theme-architecture.md` — how themes are loaded/switched
> - `specs/design/accessibility.md` — principles every theme must satisfy
> - `specs/design/styleguide.md` — tokens + components (Default theme values)

---

## 1. Design intent

| Intent | How |
|---|---|
| Maximum luminance contrast | Pure black `#000000` surfaces + pure white `#FFFFFF` text (21:1) |
| Sharp edges | Strong `#555/#777/#999` borders and 2px focus/active rings where state needs reinforcement |
| Shape over hue | Status uses icon shape + text label; fills and strokes reinforce, never replace |
| Highly visible focus | 3px white focus ring + 2px offset on every focusable element |
| Color-blind safe palette | v27 uses orange for brand/action and cyan-blue for success/info; status icons/text still carry meaning |
| Reduced complexity | No subtle tints, no translucent overlays, no drop shadows as signal |

**Philosophy:** if you can tell the state of the UI with a monochrome screenshot, the design has passed.

---

## 2. Full palette

All hex values. HSL is not the source of truth in this theme — pure colors drive contrast math.

### Backgrounds

| Token | High-Contrast value | Contrast vs. `--text-primary` |
|---|---|---|
| `--bg-base` | `#000000` | 21:1 |
| `--bg-surface` | `#0A0A0A` | 19.8:1 |
| `--bg-elevated` | `#1A1A1A` | 17.4:1 |
| `--bg-hover` | `#2A2A2A` | 13.5:1 |
| `--nav-rail-bg` | `#0A0A0A` | 19.8:1 |
| `--brand-tile` | `#1A1A1A` | logo-only |
| `--brand-pill-from` | `#2E2F35` | logo-only |
| `--brand-pill-to` | `#2E2E34` | logo-only |

**Rule:** Surfaces don't use layered translucent whites in high-contrast — elevation is communicated by borders, not bg lightness.

### Foreground

| Token | Value | Contrast vs. black |
|---|---|---|
| `--text-primary` | `#FFFFFF` | 21:1 |
| `--text-secondary` | `#E5E5E5` | 16.7:1 |
| `--text-muted` | `#BFBFBF` | 11.4:1 |
| `--text-subtle` | `#999999` | 7.4:1 |
| `--text-inverse` | `#1A0E07` | for use on orange surfaces |

### Accent

| Token | Value | Contrast vs. black | Use |
|---|---|---|---|
| `--accent-primary` | `#FF6A35` | ≈6.9:1 | CTA fills, active rings, icon accents |
| `--accent-secondary` | `#FF8050` | ≈7.9:1 | CTA hover |
| `--accent-muted` | `rgba(255,106,53,.18)` | — | Subtle selected-row fills |
| `--accent-muted-strong` | `rgba(255,106,53,.30)` | — | v27 active row gradient top stop |

### Status (shape always paired)

| State | Color | Icon | Mandatory text |
|---|---|---|---|
| Success | `#5BCDFF` | `CheckCircle2` (filled) | "OK" / "Passed" / "Available" |
| Warning | `#FFD93D` | `TriangleAlert` | "Warning" / "Attention" |
| Error | `#FF3344` (red on black: ≈6:1) | `XCircle` (filled) **+ bold text** | "Error" / "Failed" |
| Info | `#5BCDFF` | `Info` | "Info" / "Note" |

**Red is intentionally duller** so it doesn't vibrate against black. Bold text and the filled X icon carry the error semantics.

### Borders

| Token | Value | Notes |
|---|---|---|
| `--border-subtle` | `#555555` | Inner dividers |
| `--border-default` | `#777777` | Input/card borders |
| `--border-strong` | `#999999` | Emphasis, card edges |
| `--border-focus` | `#FFFFFF` @ 3px | Keyboard focus |

---

## 3. Shape / size overrides

The token values `--border-width-default`, `--border-width-focus`, `--radius-lg`, `--font-size-base` from `theme-architecture.md` change in high-contrast:

| Token | Default | High-Contrast | Why |
|---|---|---|---|
| `--border-width-default` | `1px` | `2px` | More visible edges; distinguishes overlapping cards |
| `--border-width-focus` | `2px` | `3px` | Focus must be unmissable |
| `--radius-lg` | `12px` | `8px` | Sharper corners read better at small sizes + on low-vision |
| `--radius-md` | `8px` | `6px` | Same |
| `--font-size-base` | `14px` | `15px` | Slight bump; combines with optional Font Scale setting |

---

## 4. Component-level behavior

### Buttons

```
Primary:
  bg:     #FF6A35     (v27 orange)
  text:   #1A0E07
  border: 2px solid #000000 (inside button — keeps shape on non-black surfaces)
  focus:  3px solid #FFFFFF + 2px offset

Secondary / ghost:
  bg:     #000000
  text:   #FFFFFF
  border: 2px solid #FFFFFF

Destructive:
  bg:     #FF3344
  text:   #FFFFFF
  Icon:   XCircle or Trash (required)
```

### Inputs

```
Background: #000000
Text:       #FFFFFF (size ≥ 15px)
Border:     2px solid #FFFFFF
Focus:      3px solid #FFFFFF + 2px offset
Placeholder: #BFBFBF (≥ 7:1; still usable)
Disabled:    #000000 bg, #666666 text (≥ 4.5:1 kept); no hover
```

### Switches / checkboxes

```
Unchecked:
  Switch track:  2px solid #FFFFFF, #000000 fill
  Checkbox:      2px solid #FFFFFF, empty

Checked:
  Switch track:  #FF6A35 fill, #1A0E07 thumb
  Checkbox:      #FF6A35 fill, #1A0E07 check icon (2px stroke)

Focus:
  Always adds 3px #FFFFFF ring with 2px offset
```

**Rule:** the thumb of an accent switch must be a solid dark circle, not a small light dot — visibility over aesthetics.

### Selects / dropdowns

```
Trigger: same as Input
Open menu: #000000 bg, 2px solid #FFFFFF border
Items:
  default: #000000 bg, #FFFFFF text
  hover:   #FFFFFF bg, #000000 text (inverse flip)
  selected: background #FF6A35, text #1A0E07; include ✓ icon
```

### Tabs (Global / Project Overrides etc.)

```
TabsList: #000000 bg, 2px solid #FFFFFF
Trigger default: #FFFFFF text, no bg
Trigger active: #FF6A35 bg, #1A0E07 text, +2px border bottom (visible on any bg)
```

### Cards / notices (InlineNotice)

```
ok:    #000000 bg, 2px solid #5BCDFF border, CheckCircle2 icon + "OK"
warn:  #000000 bg, 2px solid #FFD93D border, TriangleAlert icon + "Warning"
info:  #000000 bg, 2px solid #5BCDFF border, Info icon + "Info"
error: #000000 bg, 2px solid #FF3344 border, XCircle icon + "Error", bold title
```

Text inside notices is always white; the border color + icon carry the tone. ❌ Never tint the background color to convey tone in high-contrast.

### Chat bubbles

```
User message:
  bg:     rgba(255,106,53,.18)
  text:   #FFFFFF
  border: 2px solid #000000 (stays visible on white/light settings surfaces)

Agent message:
  bg:     #000000
  text:   #FFFFFF
  border: 2px solid #FFFFFF
```

User bubble uses the same v27 orange family as the rest of the shell; ownership also comes from alignment and label context.

### Nav / sidebar

```
Item default: #FFFFFF text on #000000
Item hover:   #2A2A2A bg, #E5E5E5 text
Item active:  #2A2A2A bg, #FF6A35 text, inset 2px #FF6A35 ring
  + 2px left accent marker
  + aria-current="page"
```

### Kanban columns

```
Column header always shows:
  - Icon (shape-based: clock / bolt / check / warning)
  - Text label ("Backlog", "Executing", …)
  - Count (e.g. "12")
Column body background: #000000 with 2px solid #FFFFFF border
Column header bg: #FF6A35 for ACTIVE column (current focus) only
```

Column status must be readable from shape alone; color is decoration.

### Dialogs / modals

```
Overlay: rgba(0,0,0,0.9) — nearly opaque; avoid see-through popups
Dialog:  #000000 bg, 2px solid #FFFFFF border, radius 8px
Close X: 2px stroke, #FFFFFF; focus ring as above; 32×32 hit target
```

---

## 5. Motion

High-contrast respects the same `prefers-reduced-motion` rule as the Default theme. In addition:

- Spinner uses a tick/step rotation (4 frames/sec) rather than continuous rotation when reduced-motion is on — makes progress obvious without smooth motion.
- No opacity fades (≤ 50% opacity is never used to convey state — opacity is too subtle).
- Collapsible lanes expand instantly (no height transition).

---

## 6. What to avoid

| ❌ Anti-pattern | Why |
|---|---|
| `color: #ff6b35` inline | Hardcoded default-theme accent — must be `var(--accent-primary)` |
| `background: rgba(255,255,255,0.04)` | Translucent overlays disappear on pure black — use solid bg + border |
| Soft drop shadows for elevation | Invisible on black; always use 2px borders |
| Icon-only status indicators | Even well-chosen icons need a text label in this theme |
| Red/green pairing as sole signal | Protanopia/deuteranopia can't distinguish; use shape + text |
| Placeholder-as-label | Placeholder grays to `#B0B0B0` — a vanished label is unacceptable at low vision |
| Hue-only chart colors | Every chart series needs a pattern (solid / dashed / dotted) or marker shape |

---

## 7. Contrast verification table (spot-checked AAA)

| Pair | Ratio | Meets AAA? |
|---|---|---|
| `#FFFFFF` on `#000000` | 21:1 | ✅ |
| `#E5E5E5` on `#000000` | 16.7:1 | ✅ |
| `#BFBFBF` on `#000000` | 11.4:1 | ✅ |
| `#FF6A35` on `#000000` | ≈6.9:1 | ✅ for UI and large text |
| `#1A0E07` on `#FF6A35` | high contrast for button text | ✅ |
| `#5BCDFF` on `#000000` | high contrast | ✅ |
| `#FF3344` on `#000000` | ~6:1 | ✅ for large text; meets UI-component 4.5:1 AAA — still paired with XCircle + bold "Error" |

Any new color introduced must be verified against `#000000` (and `#FF6A35` if used for button text) and added here.

---

## 8. Testing checklist specific to this theme

- [ ] Every status reads correctly in a grayscale screenshot
- [ ] Keyboard sweep: focus ring visible on every interactive element
- [ ] VoiceOver sweep on Settings + Chat panel — reads roles, states, labels
- [ ] Toggle `prefers-reduced-motion`: spinners/transitions degrade gracefully
- [ ] 200% zoom: no overlap, no truncation without a tooltip
- [ ] Contrast of every new token pair documented in §7
- [ ] No use of `rgba(*)` backgrounds for elevation
- [ ] No legacy orange `#FF6B35` anywhere in the DOM when `[data-theme="high-contrast"]` is set; use v27 `#FF6A35` or tokens.

---

## 9. Open decisions

- **Icon weight:** lucide icons default to 2px stroke — possibly bump to 2.5px in high-contrast for chunkier silhouettes. Defer until user testing.
- **User bubble color:** v27 orange tint is quieter than filled accent; validate with real conversation density.
- **Syntax highlighting (code blocks):** must define a high-contrast code theme; current default is dark + pastel. TODO.

---

## 10. Reference screenshots

*(To be captured once the toggle ships. Store under `assets/public/themes/high-contrast/*.png` — framed per `.claude/rules/assets.md`.)*
