# RalphX Design System

The definitive design guide for RalphX. All UI work must follow these specifications.

## Current information architecture

The live shell has eight root views: **Agents**, **Automations**, **Ticketing**,
**GitHub**, **Granola**, **Extensibility**, **Activity**, and **Insights**.
Planning and task surfaces are artifacts inside Agents: Plan, Ideation,
Tasks, Graph, and Kanban. Legacy standalone roots (`ideation`, `graph`,
`kanban`, and `task_detail`) are compatibility inputs only and must not be
added to new UI state or navigation.

---

## 1. Design Philosophy

### macOS Tahoe Style (2025)

> **Primary Reference:** `specs/design/macos-tahoe-style-guide.md`

RalphX now follows the **macOS Tahoe** aesthetic—clean, flat, and minimal like the native Finder app.

**Core Principles:**

| Principle | Description |
|-----------|-------------|
| **Flat Surfaces** | Solid colors only. No gradients, no glass effects, no backdrop blur |
| **Blue-Gray Palette** | All grays use `hsl(220 10% xx%)` for subtle cool undertone |
| **Invisible Structure** | Separation through color difference, not borders |
| **Quiet Typography** | Small (11-13px), understated, never demanding attention |
| **Blue Selection** | Selected items use blue tint, not accent orange |

**What makes it feel native:**
- **Simplicity** - Remove everything that doesn't need to be there
- **Flatness** - No shadows, no depth simulation, no elevation
- **Consistency** - Same patterns as macOS Finder
- **Compactness** - Tight spacing, small text, dense information
- **Quietness** - UI fades into background, content is the focus

### Reference Apps
- **macOS Finder** - Section headers, selection style, sidebar layout
- **Linear** - Board layout, card interactions, keyboard-first UX
- **Things 3** - Clean lists, subtle interactions

### Detailed Pattern Reference
See **[macOS Tahoe Style Guide](design/macos-tahoe-style-guide.md)** for comprehensive code examples and implementation patterns.

---

## 2. Style Guardrails (macOS Tahoe)

**NEVER use these patterns:**
- ❌ Gradients on surfaces (no linear-gradient, no radial-gradient)
- ❌ Box shadows for depth (only for focus rings)
- ❌ Backdrop blur / glass effects
- ❌ Decorative borders (separation via color, not lines)
- ❌ Glowing accents (no box-shadow color bleeds)
- ❌ Large typography (keep it 11-13px)
- ❌ Orange/accent for selection (use blue tint instead)
- ❌ Pure black backgrounds (use blue-gray: `hsl(220 10% xx%)`)
- ❌ Inter font (use SF Pro / system fonts)
- ❌ Transform animations on hover (no translateY, no scale)

**ALWAYS use these patterns:**
- ✅ Flat, solid background colors
- ✅ Blue-gray palette: `hsl(220 10% xx%)` for all grays
- ✅ Blue selection highlight: `hsla(220 60% 50% / 0.20)`
- ✅ Small, uppercase section headers in muted gray
- ✅ Compact spacing (4px base unit)
- ✅ Simple hover = background color change only
- ✅ Warm orange accent ONLY for primary actions (buttons)
- ✅ Focus rings: `0 0 0 2px hsla(220 80% 60% / 0.5)`
- ✅ Minimal borders (only on inputs if needed)
- ✅ System fonts (SF Pro, -apple-system)

---

## 3. Color System

> **Full reference:** `specs/design/macos-tahoe-style-guide.md`

### Backgrounds (Blue-Gray Tinted)

All grays use `hsl(220 10% xx%)` for subtle cool undertone like macOS Finder.

| Token | Value | Usage |
|-------|-------|-------|
| `--bg-base` | `hsl(220 10% 8%)` | Main canvas |
| `--bg-surface` | `hsl(220 10% 12%)` | Cards, sidebar, surfaces |
| `--bg-elevated` | `hsl(220 10% 16%)` | Hover backgrounds |
| `--bg-hover` | `hsl(220 10% 20%)` | Active hover |

**No gradients. Solid colors only.**

### Text (Blue-Gray Tinted)

| Token | Value | Usage |
|-------|-------|-------|
| `--text-primary` | `hsl(220 10% 90%)` | Primary text, file names |
| `--text-secondary` | `hsl(220 10% 60%)` | Secondary info |
| `--text-muted` | `hsl(220 10% 45%)` | Section headers, hints |
| `--text-faint` | `hsl(220 10% 35%)` | Disabled, placeholder |

### Selection & Focus

| Token | Value | Usage |
|-------|-------|-------|
| `--selection-bg` | `hsla(220 60% 50% / 0.20)` | Selected item background |
| `--focus-ring` | `hsla(220 80% 60% / 0.5)` | Focus outline |

**Selection is BLUE, not orange.**

### Accent (Use Sparingly)

Warm orange for **primary action buttons only**.

| Token | Value | Usage |
|-------|-------|-------|
| `--accent-primary` | `hsl(14 100% 60%)` | Primary buttons only |
| `--accent-hover` | `hsl(14 100% 55%)` | Button hover |

### Status Colors (Badges/Indicators Only)

| Token | Value | Usage |
|-------|-------|-------|
| `--status-success` | `hsl(145 60% 45%)` | Success badges |
| `--status-warning` | `hsl(45 90% 55%)` | Warning badges |
| `--status-error` | `hsl(0 70% 55%)` | Error badges |
| `--status-info` | `hsl(220 80% 60%)` | Info badges |

### Borders (Minimal)

| Token | Value | Usage |
|-------|-------|-------|
| `--border-subtle` | `hsl(220 10% 18%)` | Only if needed |
| `--border-default` | `hsl(220 10% 22%)` | Input borders |

**Prefer no borders. Separate with color difference.**

### Usage Guidelines
1. **No borders on containers** - Use color difference instead
2. **Selection = blue** - Never use accent orange for selection
3. **Accent for buttons only** - Primary action buttons, nothing else
4. **Status colors for badges** - Never for backgrounds

---

## 4. Typography

### Font Families
System fonts for performance and Mac-native feel.

| Token | Value | Usage |
|-------|-------|-------|
| `--font-display` | SF Pro Display, -apple-system, sans-serif | Headings, titles |
| `--font-body` | SF Pro Text, -apple-system, sans-serif | Body text, UI labels |
| `--font-mono` | JetBrains Mono, Menlo, monospace | Code, diffs, IDs |

### Type Scale (Compact Application UI)

RalphX is a **native application**, not a marketing website. Use compact sizing to maximize information density.

| Class | Size | Usage |
|-------|------|-------|
| `text-[9px]` | 9px | Priority badges, mini labels |
| `text-[10px]` | 10px | Timestamps, metadata, counts |
| `text-[11px]` | 11px | Secondary info, hints |
| `text-xs` | 12px | Descriptions, card content |
| `text-sm` | 14px | Primary UI text, titles |
| `text-base` | 16px | Section headers (use sparingly) |
| `text-lg` | 18px | Modal titles, hero text (rare) |

**Principle**: Most UI text should be 12-14px. Reserve 16px+ for key headings only.

### Letter Spacing

| Token | Value | Usage |
|-------|-------|-------|
| `--tracking-tight` | -0.02em | Headings, titles (tighter = premium) |
| `--tracking-normal` | 0 | Body text |
| `--tracking-wide` | 0.05em | Uppercase labels, badges |

### Line Heights

| Token | Value | Usage |
|-------|-------|-------|
| `--leading-tight` | 1.2 | Headings, single-line labels |
| `--leading-normal` | 1.5 | Body text, descriptions |
| `--leading-relaxed` | 1.65 | Long-form content, readability |

### Font Weights

| Weight | Value | Usage |
|--------|-------|-------|
| Normal | 400 | Body text, descriptions |
| Medium | 500 | Labels, subtle emphasis |
| Semibold | 600 | Headings, titles, buttons |

---

## 5. Spacing System

### Base Unit
4px base unit, 8pt grid for component alignment.

### Spacing Tokens (Compact Application UI)

RalphX favors compact spacing to maximize screen real estate for information-dense views.

| Token | Value | Usage |
|-------|-------|-------|
| `--space-0.5` | 2px | Micro gaps (between badge text and border) |
| `--space-1` | 4px | Icon-to-text gaps, inline elements |
| `--space-1.5` | 6px | Tight list spacing |
| `--space-2` | 8px | Card content gaps, button padding |
| `--space-2.5` | 10px | Session card padding |
| `--space-3` | 12px | Panel headers, section padding |
| `--space-4` | 16px | Larger containers (modals) |
| `--space-5` | 20px | Empty state padding |
| `--space-6` | 24px | Major section breaks |

### Component Sizing Guide

| Component | Height | Padding | Font |
|-----------|--------|---------|------|
| Sidebar header | auto | px-3 py-3 | text-sm |
| Panel header | h-9 (36px) | px-3 py-2 | text-xs |
| Main header | h-11 (44px) | px-4 | text-xs |
| Session card | auto | p-2.5 | text-xs |
| Proposal card | auto | p-3 | text-xs |
| Message bubble | auto | px-3 py-2 | text-[13px] |
| Button (small) | h-6-8 | px-2-3 | text-xs |
| Badge | auto | px-1.5 py-px | text-[9px] |
| Icon (inline) | w-3-3.5 | - | - |
| Avatar | w-6-7 | - | - |

### Compact UI Principles
- **Cards**: Use p-2.5 to p-3, not p-4
- **Sidebars**: 260px width, tight item spacing (space-y-1)
- **Headers**: h-9 to h-11, not h-14+
- **Icons**: 12-16px for UI, 20-24px for empty states
- **Empty states**: Keep decorative but not oversized (w-12 icons, not w-16)

---

## 6. Shadow System (Layered)

Shadows provide depth and hierarchy. **Always use multiple layers** for realistic elevation. Single shadows look flat.

### Shadow Tokens

| Token | CSS | Usage |
|-------|-----|-------|
| `--shadow-xs` | `0 1px 2px rgba(0,0,0,0.2), 0 2px 4px rgba(0,0,0,0.1)` | Subtle card lift |
| `--shadow-sm` | `0 2px 4px rgba(0,0,0,0.2), 0 4px 8px rgba(0,0,0,0.15)` | Hover states, dropdowns |
| `--shadow-md` | `0 4px 12px rgba(0,0,0,0.3), 0 8px 24px rgba(0,0,0,0.2)` | Modals, floating panels |
| `--shadow-lg` | `0 8px 24px rgba(0,0,0,0.4), 0 16px 48px rgba(0,0,0,0.3)` | Large overlays |
| `--shadow-glow` | `0 0 0 3px rgba(255,107,53,0.1), 0 0 12px rgba(255,107,53,0.15)` | Focus/active glow |

### Premium Card Shadow (Recommended)

```css
.premium-card {
  box-shadow:
    0 2px 8px rgba(0,0,0,0.2),
    0 1px 2px rgba(0,0,0,0.1),
    inset 0 1px 0 rgba(255,255,255,0.03);
}

.premium-card:hover {
  box-shadow:
    0 4px 12px rgba(0,0,0,0.3),
    0 2px 4px rgba(0,0,0,0.15),
    0 0 0 1px rgba(255,255,255,0.05);
}
```

### Accent Glow (Selected/Active States)

```css
/* Subtle pulsing glow for selected items */
@keyframes glowPulse {
  0%, 100% {
    box-shadow: 0 0 12px rgba(255,107,53,0.08), 0 0 24px rgba(255,107,53,0.04);
  }
  50% {
    box-shadow: 0 0 18px rgba(255,107,53,0.15), 0 0 36px rgba(255,107,53,0.08);
  }
}

.selected {
  animation: glowPulse 3s ease-in-out infinite;
}
```

### Shadow Usage
- **Cards at rest**: Layered shadow with inset highlight
- **Cards on hover**: Increased shadow + slight lift (`translateY(-1px)`)
- **Dropdowns/Popovers**: `--shadow-md` with backdrop blur
- **Modals**: `--shadow-lg` with dark backdrop
- **Selected states**: Subtle glow animation
- **Focus states**: `--shadow-glow` ring

---

## 7. Border & Radius System

### Radius Tokens

| Token | Value | Usage |
|-------|-------|-------|
| `--radius-sm` | 4px | Small elements (badges, chips) |
| `--radius-md` | 8px | Buttons, inputs, cards |
| `--radius-lg` | 12px | Larger cards, modals |
| `--radius-xl` | 16px | Hero sections, large containers |
| `--radius-full` | 9999px | Pills, avatars, circular buttons |

### Gradient Border Technique
For premium cards with subtle depth:

```css
.premium-card {
  border: 1px solid transparent;
  background:
    linear-gradient(var(--bg-elevated), var(--bg-elevated)) padding-box,
    linear-gradient(180deg, rgba(255,255,255,0.08) 0%, rgba(255,255,255,0.02) 100%) border-box;
  border-radius: var(--radius-md);
}
```

### Border Patterns
- **Subtle dividers**: `1px solid var(--border-subtle)`
- **Card outlines**: `1px solid var(--border-default)`
- **Focus/Selected**: `2px solid var(--accent-primary)` or use `--shadow-glow`
- **No border + shadow**: Often cleaner than bordered cards

---

## 8. Component Patterns

### Buttons

**Primary Button (Gradient):**
```css
.btn-primary {
  background: linear-gradient(180deg, #ff6b35 0%, #f97316 100%);
  color: white;
  font-weight: 500;
  box-shadow:
    0 2px 8px rgba(255,107,53,0.3),
    inset 0 1px 0 rgba(255,255,255,0.1);
}

.btn-primary:hover {
  background: linear-gradient(180deg, #ff7a4d 0%, #fb923c 100%);
  box-shadow: 0 4px 12px rgba(255,107,53,0.4);
  transform: translateY(-1px);
}
```

**Variants:**
| Variant | Background | Text | Usage |
|---------|------------|------|-------|
| Primary | gradient `#ff6b35` to `#f97316` | white | Main actions (1 per section) |
| Secondary | `rgba(255,255,255,0.05)` | `--text-primary` | Secondary actions |
| Ghost | transparent | `--text-secondary` | Tertiary, navigation |
| Destructive | `--status-error` | white | Delete, dangerous actions |

**States:**
- Hover: gradient shift + shadow increase + `translateY(-1px)`
- Active: `scale(0.98)`, darken bg
- Focus: `--shadow-glow` ring
- Disabled: 50% opacity, no pointer events

**Sizing:**
- Small: 28px height, text-sm, px-3
- Default: 36px height, text-sm, px-4
- Large: 44px height, text-base, px-6

### Cards (Premium Pattern)

**Recommended Card Structure:**
```css
.premium-card {
  background: linear-gradient(180deg, rgba(28,28,28,0.9) 0%, rgba(22,22,22,0.95) 100%);
  border: 1px solid rgba(255,255,255,0.06);
  border-radius: 12px;
  box-shadow: 0 2px 8px rgba(0,0,0,0.2);
  transition: all 200ms cubic-bezier(0.4, 0, 0.2, 1);
}

.premium-card:hover {
  background: linear-gradient(180deg, rgba(32,32,32,0.95) 0%, rgba(26,26,26,0.98) 100%);
  border-color: rgba(255,255,255,0.1);
  transform: translateY(-1px);
  box-shadow: 0 4px 12px rgba(0,0,0,0.3);
}
```

**Variants:**
| Variant | Background | Border | Shadow |
|---------|------------|--------|--------|
| Standard | gradient `#1c1c1c` to `#161616` | `rgba(255,255,255,0.06)` | layered |
| Glass | `rgba(26,26,26,0.95)` + blur(20px) | gradient | `--shadow-md` |
| Selected | gradient with `#ff6b35` tint | `rgba(255,107,53,0.25)` | glow pulse |

**Card Padding:** 14-16px default, 12px for compact cards

### Inputs

**States:**
| State | Border | Shadow | Background |
|-------|--------|--------|------------|
| Default | `--border-default` | none | `--bg-surface` |
| Hover | `--border-default` | none | `--bg-hover` |
| Focus | `--accent-primary` | `--shadow-glow` | `--bg-surface` |
| Error | `--status-error` | error glow | `--bg-surface` |
| Disabled | `--border-subtle` | none | `--bg-base` + 50% opacity |

**Input Sizing:** 36px height default, 12px horizontal padding

### Badges

**Variants:**
| Variant | Background | Text | Usage |
|---------|------------|------|-------|
| Default | `--bg-surface` | `--text-secondary` | Neutral labels |
| Primary | `--accent-muted` | `--accent-primary` | Highlighted |
| Success | success-muted | `--status-success` | Passed, complete |
| Warning | warning-muted | `--status-warning` | Pending, caution |
| Error | error-muted | `--status-error` | Failed, blocked |

**Badge Sizing:** text-xs, px-2 py-0.5, rounded-md

### Modals

**Structure:**
```
Backdrop: rgba(0,0,0,0.6) + backdrop-blur(8px)
Content: --bg-elevated + --shadow-lg + --radius-lg
Animation: scale(0.95)→scale(1) + opacity(0)→opacity(1), 200ms
Close: Escape key + backdrop click (unless destructive)
```

**Sizes:**
- Small (max-w-sm): Simple confirmations
- Medium (max-w-md): Forms, details
- Large (max-w-lg): Complex content
- Full (max-w-2xl): Task detail, wizards

**Spacing:** 24px padding (--space-6)

---

## 9. Motion & Micro-interactions

### Timing Functions

| Name | Value | Usage |
|------|-------|-------|
| `ease-smooth` | `cubic-bezier(0.4, 0, 0.2, 1)` | Default for most transitions |
| `ease-out` | `cubic-bezier(0, 0, 0.2, 1)` | Enter animations |
| `ease-in` | `cubic-bezier(0.4, 0, 1, 1)` | Exit animations |
| `ease-spring` | `cubic-bezier(0.34, 1.56, 0.64, 1)` | Playful interactions |

### Durations

| Token | Value | Usage |
|-------|-------|-------|
| `--transition-fast` | 150ms | Hover states, micro-feedback |
| `--transition-normal` | 200ms | Most transitions, card interactions |
| `--transition-slow` | 300ms | Complex animations, modals, entry |

### Premium Hover Pattern

```css
.premium-hoverable {
  transition: all 200ms cubic-bezier(0.4, 0, 0.2, 1);
}

.premium-hoverable:hover {
  transform: translateY(-1px);
  background: /* slightly lighter gradient */;
  border-color: rgba(255,255,255,0.1);
  box-shadow: 0 4px 12px rgba(0,0,0,0.3);
}
```

### Entry Animations

**Fade Slide In (Lists, Cards):**
```css
@keyframes fadeSlideIn {
  from {
    opacity: 0;
    transform: translateY(8px);
  }
  to {
    opacity: 1;
    transform: translateY(0);
  }
}

.animate-in {
  animation: fadeSlideIn 0.3s ease-out forwards;
}

/* Staggered children for lists */
.stagger > *:nth-child(1) { animation-delay: 0.05s; }
.stagger > *:nth-child(2) { animation-delay: 0.1s; }
.stagger > *:nth-child(3) { animation-delay: 0.15s; }
```

### Glow Pulse (Selected States)

```css
@keyframes glowPulse {
  0%, 100% {
    box-shadow: 0 0 12px rgba(255,107,53,0.08), 0 0 24px rgba(255,107,53,0.04);
  }
  50% {
    box-shadow: 0 0 18px rgba(255,107,53,0.15), 0 0 36px rgba(255,107,53,0.08);
  }
}

.selected-glow {
  animation: glowPulse 3s ease-in-out infinite;
}
```

### Shimmer Loading

```css
@keyframes shimmer {
  0% { background-position: -200% 0; }
  100% { background-position: 200% 0; }
}

.shimmer-loading {
  background: linear-gradient(
    90deg,
    rgba(255,255,255,0) 0%,
    rgba(255,255,255,0.05) 50%,
    rgba(255,255,255,0) 100%
  );
  background-size: 200% 100%;
  animation: shimmer 2s infinite;
}
```

### Standard Interactions

**Active Press:**
```css
.pressable:active {
  transform: scale(0.98);
}
```

**Focus Ring:**
```css
.focusable:focus-visible {
  outline: none;
  box-shadow: 0 0 0 3px rgba(255,107,53,0.1), 0 0 12px rgba(255,107,53,0.15);
}
```

**Drag State:**
```css
.dragging {
  transform: scale(1.02) rotate(2deg);
  box-shadow: var(--shadow-md);
  opacity: 0.9;
}
```

---

## 10. Icon Usage (Lucide)

### Size Guidelines

| Context | Size | Stroke | Usage |
|---------|------|--------|-------|
| Inline text | 16px | 1.5px | Badge icons, inline labels |
| Buttons | 18-20px | 2px | Button icons |
| Navigation | 20-24px | 2px | Nav items, prominent actions |
| Empty states | 48-64px | 1.5px | Illustration-style usage |

### Color
Icons inherit text color by default. Use `currentColor`.

### Common Icons by Category

**Status:**
- CheckCircle (success), XCircle (error), AlertCircle (warning)
- Clock (pending), Loader2 (loading - animated)

**Navigation:**
- LayoutGrid (kanban), Lightbulb (ideation), Settings
- Activity (activity stream), Puzzle (extensibility)

**Actions:**
- Plus (add), Trash2 (delete), Edit (edit), MoreHorizontal (menu)
- ChevronRight, ChevronDown (expandable)

**Task/Git:**
- GitBranch, GitCommit, GitMerge, GitPullRequest
- FolderOpen, File, FileCode

### Stroke Width Consistency
Always use stroke-width 1.5-2. Never use filled icons in Lucide.

---

## 11. Page-Specific Patterns

Each page has its own detailed design specification. See the individual files for complete requirements.

| Page | File | Status |
|------|------|--------|
| Agents Tasks artifact — Kanban | [kanban-board.md](design/pages/kanban-board.md) | Complete |
| Agents Plan/Ideation artifacts | [ideation-view.md](design/pages/ideation-view.md) | Complete |
| Settings View | [settings-view.md](design/pages/settings-view.md) | Complete |
| Activity Stream | [activity-stream.md](design/pages/activity-stream.md) | Complete |
| Extensibility View | [extensibility-view.md](design/pages/extensibility-view.md) | Complete |
| Task Detail | [task-detail.md](design/pages/task-detail.md) | Complete |
| Reviews Panel | [reviews-panel.md](design/pages/reviews-panel.md) | Complete |
| Chat Panel (Global) | [chat-panel.md](design/pages/chat-panel.md) | Complete |
| QA Components | [qa-components.md](design/pages/qa-components.md) | Complete |
| Project Sidebar | [project-sidebar.md](design/pages/project-sidebar.md) | Complete |
| Project Dialogs | [project-dialogs.md](design/pages/project-dialogs.md) | Complete |
| Diff Viewer | [diff-viewer.md](design/pages/diff-viewer.md) | Complete |
| Execution Control Bar | [execution-control-bar.md](design/pages/execution-control-bar.md) | Complete |
| Header and Navigation | [header-navigation.md](design/pages/header-navigation.md) | Complete |
| Modal Standards | [modal-standards.md](design/pages/modal-standards.md) | Complete |

---

## 12. shadcn/ui Integration

RalphX uses shadcn/ui as the component foundation. All shadcn components are customized to use RalphX design tokens.

### Tailwind CSS v4 Configuration

RalphX uses **Tailwind CSS v4** with the Vite plugin. This is a different configuration pattern than v3:

**Critical v4 Rules:**
- ❌ NO `tailwind.config.js` file - v4 ignores it
- ❌ NO `tailwindcss-animate` package - deprecated in v4
- ✅ Use `@tailwindcss/vite` plugin in `vite.config.ts`
- ✅ Use `@theme inline` in CSS for theme configuration
- ✅ Use `@import "tailwindcss"` at the top of `globals.css`

**Four-Step Architecture (Required):**

1. **Define CSS Variables at Root Level** (NOT inside `@layer base`):
```css
:root {
  --bg-base: hsl(0 0% 6%);        /* hsl() wrapper required */
  --accent-primary: hsl(14 100% 60%);
}
.dark {
  /* Same structure for dark mode */
}
```

2. **Map Variables to Tailwind Utilities** via `@theme inline`:
```css
@theme inline {
  --color-bg-base: var(--bg-base);
  --color-accent-primary: var(--accent-primary);
}
```

3. **Apply Base Styles** in `@layer base`:
```css
@layer base {
  body {
    background-color: var(--bg-base);  /* NO hsl() here */
  }
}
```

4. **Use Utilities** - classes like `bg-bg-base`, `text-accent-primary` now work automatically.

### Installed Components
- Button, Card, Dialog, Dropdown Menu
- Input, Label, Tabs, Tooltip, Popover
- Select, Checkbox, Switch, Badge
- Scroll Area, Separator, Skeleton

### CSS Variable Mapping
shadcn variables are mapped to RalphX tokens in `globals.css`:
- `--primary` → `--accent-primary`
- `--background` → `--bg-base`
- `--card` → `--bg-elevated`
- `--foreground` → `--text-primary`
- `--ring` → `--accent-primary`

### Component Location
All shadcn components live in `src/components/ui/`.

### Usage Pattern
```tsx
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardHeader } from '@/components/ui/card';
import { Dialog, DialogContent, DialogTrigger } from '@/components/ui/dialog';
```

---

## 13. Accessibility

### Color Contrast
- Text on backgrounds: minimum 4.5:1 ratio
- Large text (18px+): minimum 3:1 ratio
- All colors tested against WCAG AA

### Focus States
- All interactive elements have visible focus
- Focus ring: 2px offset, accent color
- Never remove outline without replacement

### Keyboard Navigation
- Tab order follows visual order
- Escape closes modals/popovers
- Arrow keys for lists and menus
- Enter/Space activates buttons

### Screen Readers
- Semantic HTML elements
- ARIA labels where needed
- Icon-only buttons have aria-label

---

---

## 14. Phase 14 Implementation Checklist

This checklist tracks the implementation of designs from Phase 13 using shadcn/ui and Lucide icons.

### Foundation Components
- [ ] Configure CSS custom properties in `globals.css` (color tokens, typography, spacing, shadows)
- [ ] Set up Tailwind theme extensions for design tokens
- [ ] Customize shadcn component variants to match design system
- [ ] Create base layout components (PageContainer, Panel, SplitPane)

### Core UI Components
- [ ] **Header & Navigation** - Implement header with glass effect, navigation items, project switcher
- [ ] **Project Sidebar** - Implement collapsible sidebar with project list, status indicators
- [ ] **Execution Control Bar** - Implement control bar with agent status, progress, action buttons

### Kanban Board
- [ ] **TaskBoard** - Horizontal scroll container with fade edges, warm radial gradient
- [ ] **Column** - Fixed-width columns with glass header, drop zone, empty state
- [ ] **TaskCard** - Cards with priority stripe, hover lift, drag state, selection state
- [ ] Drag-and-drop integration with visual feedback

### Task Detail & Modals
- [ ] **Task Detail Modal** - Full modal with tabs, metadata panel, glass effects
- [ ] **Modal Standards** - Implement consistent modal patterns (sizes, animations, accessibility)
- [ ] **Project Dialogs** - Create/edit project dialogs with form validation

### Agents Planning Artifacts
- [ ] **Plan/Ideation artifacts** - Conversation-scoped planning surfaces inside Agents
- [ ] **Agents conversation** - Message timeline, typing indicator, and composer
- [ ] **Proposals artifact** - Proposal cards, selection, ordering, and apply actions

### Reviews & QA
- [ ] **Reviews Panel** - Review cards, status indicators, diff integration
- [ ] **QA Components** - Test step lists, status badges, visual verification areas
- [ ] **Diff Viewer** - Syntax-highlighted diffs with line numbers, expand/collapse

### Settings & Activity
- [ ] **Settings View** - Tabbed interface, form sections, toggle controls
- [ ] **Activity Stream** - Timeline with event icons, filtering, search
- [ ] **Extensibility View** - Plugin cards, methodology toggles, configuration panels

### Chat Panel (Global)
- [ ] **Chat Panel** - Slide-out panel, message list, input area
- [ ] Integration with main layout (overlay mode vs embedded mode)

### Polish & Accessibility
- [ ] All focus states use `--shadow-glow` pattern
- [ ] Keyboard navigation for all interactive elements
- [ ] Proper ARIA labels and roles
- [ ] Color contrast meets WCAG AA standards
- [ ] Micro-interactions (hover lift, active press) implemented consistently
- [ ] Loading states with skeleton components

### Design Quality Verification
- [ ] No purple or blue gradients anywhere
- [ ] Warm orange accent (`#ff6b35`) used sparingly (5% rule)
- [ ] SF Pro typography throughout (not Inter)
- [ ] Layered shadows for depth (not flat surfaces)
- [ ] Glass effects on headers/overlays (backdrop-blur)
- [ ] All spacing on 4px/8px grid
- [ ] Lucide icons used consistently (correct sizes, stroke widths)

---

## References

- **Design Overhaul Plan**: `specs/DESIGN_OVERHAUL_PLAN.md`
- **Global CSS**: `src/styles/globals.css` (contains all design tokens and `@theme inline` config)
- **Vite Config**: `vite.config.ts` (includes `@tailwindcss/vite` plugin)
- **shadcn Config**: `components.json` (config field must be empty for v4)
- **shadcn Components**: `src/components/ui/`
- **Lucide Icons**: https://lucide.dev/icons/
- **Tailwind v4 Docs**: https://tailwindcss.com/docs
- **shadcn/ui v4 Guide**: https://ui.shadcn.com/docs/tailwind-v4
