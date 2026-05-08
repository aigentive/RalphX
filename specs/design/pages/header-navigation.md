# Header and Navigation

The current v27 chrome is split between `AppTopBar` and `LeftNavRail`: a fixed 48px topbar for breadcrumbs/search/status/theme/font controls, plus a 72px icon-only rail for primary view switching.

**Current non-negotiables:** topbar matches v27 spacing; primary view switching lives in the left rail; project selector does not occupy topbar chrome; unimplemented topbar tools may be static but must keep the v27 geometry.

## v27 Contract

| Element | Contract |
|---|---|
| Topbar | `height: 48px`, `pl: 88px`, `pr: 16px`, bg `--topbar-bg`, bottom border `--border-subtle` |
| Traffic lights | static 12px dots at `left: 20px`, `top: 17px` |
| Breadcrumb | `Workspace / <View> / New run` for Agents start, 12.5px, current crumb `--text-primary` |
| Command search | 380px × 32px, bg `--bg-elevated`, border `--border-default`, radius 6px, `⌘K` keycap |
| Reviews | 32px icon button with orange count badge, tooltip required |
| Theme | 32px dropdown trigger, options Dark/Light/High contrast |
| Font size | 32px dropdown trigger, `Aa 100%` default |
| Left rail | 72px wide, bg `--nav-rail-bg`, inline v27 `BrandMark` logo, 44px icon buttons, 28px divider, tooltip required |
| Rail active | bg `--bg-hover`, color `--nav-rail-active-color`, 2px orange left marker, inset outline only, no glow |
| Rail focus | 2px `--border-focus` outline with 2px offset; no Tailwind ring/offset shadow |
| Brand mark | 44px square SVG, no wordmark, literal theme fills mirror `--brand-tile`, `--brand-pill-*`, and `--brand-x` so WKWebView cannot fall back to black |
| Agents sidebar | fixed 272px v27 panel, bg `--bg-surface`, border `--border-subtle`, no backdrop blur, shadow, or resize gutter |
| Agents tree | v27 header + project/session tree only; no legacy `All projects`/`Latest`/`Archived` pill row |
| Agents footer | static v27 `Recent` block pinned above dashed `Add project` until live recent runs are wired |
| Right reviews sidebar | flush right panel, bg `--bg-surface`, left border `--border-subtle`, no floating card margin, radius, or shadow |

---

## Legacy Notes

Historical notes below describe the pre-v27 horizontal navigation shell and should not override the v27 contract above.

**Design Inspiration:**
- Linear's header (minimal chrome, keyboard shortcuts prominent, no wasted space)
- Raycast's command bar (Mac-native feel, glass effects, SF Pro typography)
- Arc's browser controls (spatial organization, bold typography, warm accents)
- Vercel Dashboard header (clean hierarchy, project switching, action groupings)

**Aesthetic Direction:** Mac-native refinement. The header should feel invisible until needed - providing critical navigation and context without demanding attention. The warm orange accent appears sparingly (active states, focused elements) while the overall composition relies on typography weight, spatial rhythm, and subtle surface treatments to establish hierarchy. Every pixel is intentional; nothing decorative without purpose.

---

## Layout Structure

### Header Container

```
┌──────────────────────────────────────────────────────────────────────────────────────┐
│  RalphX   [Kanban] [Ideation] [Extensibility] [Activity] [Settings]  │  ▾ Project  │  💬 Chat ⌘K  │  ✓ Reviews (3)  │
└──────────────────────────────────────────────────────────────────────────────────────┘
   ↑ Left Section (branding + nav)                                       ↑ Center      ↑ Right Section (toggles)
```

### Container Properties

| Property | Value | Notes |
|----------|-------|-------|
| Position | Fixed at top | Sticky within window |
| Height | 48px | Comfortable touch targets |
| Width | 100% of viewport | Full-width bar |
| Background | `--bg-surface` | Subtle elevation from content |
| Border bottom | 1px `--border-subtle` | Delicate separation |
| Box shadow | `0 1px 3px rgba(0,0,0,0.1), 0 1px 2px rgba(0,0,0,0.06)` | Subtle lift |
| Padding | 0 16px | Consistent with page margins |
| Display | Flex, align-items center, justify-content space-between |
| Z-index | 50 | Above all content, below modals |
| `-webkit-app-region` | drag | Mac-native window dragging |

### Window Drag Region

The header serves as the primary drag region for the Tauri window on macOS:

```css
header {
  -webkit-app-region: drag;
}

/* Clickable elements must opt out */
header button,
header a,
header [role="button"],
header [data-clickable] {
  -webkit-app-region: no-drag;
}
```

---

## Left Section (Branding + Navigation)

### Section Container

| Property | Value |
|----------|-------|
| Display | Flex, align-items center |
| Gap | 24px between branding and nav |

### App Branding

The RalphX wordmark - distinctive but not dominant.

| Property | Value |
|----------|-------|
| Font | `--font-display` (SF Pro Display) |
| Size | `text-xl` (20px) |
| Weight | 700 (bold) |
| Color | `--accent-primary` (#ff6b35) |
| Letter spacing | `--tracking-tight` (-0.02em) |
| Cursor | default (part of drag region) |

```tsx
<h1
  className="text-xl font-bold tracking-tight"
  style={{ color: 'var(--accent-primary)' }}
>
  RalphX
</h1>
```

### View Navigation

Horizontal nav bar with five view buttons. Using **shadcn Button** ghost variant as base.

| Property | Value |
|----------|-------|
| Display | Flex, align-items center |
| Gap | 4px between nav items |
| Role | navigation |
| Aria-label | "Main views" |

### Navigation Item (Inactive)

| Property | Value |
|----------|-------|
| Component | shadcn Button (ghost variant) |
| Padding | 8px 12px (px-3 py-2) |
| Height | 32px |
| Border radius | `--radius-md` (8px) |
| Background | transparent |
| Color | `--text-secondary` |
| Font | `text-sm` (14px), `font-medium` (500) |
| Transition | background-color 150ms ease, color 150ms ease |
| Display | Flex, align-items center, gap 8px |

### Navigation Item (Active)

| Property | Value |
|----------|-------|
| Background | `--bg-elevated` |
| Color | `--accent-primary` |
| Icon color | `--accent-primary` |

### Navigation Item (Hover - Inactive)

| Property | Value |
|----------|-------|
| Background | `--bg-hover` |
| Color | `--text-primary` |

### Navigation Icons

Each view has a dedicated Lucide icon:

| View | Icon | Size |
|------|------|------|
| Kanban | `LayoutGrid` | 18px |
| Ideation | `Lightbulb` | 18px |
| Extensibility | `Puzzle` | 18px |
| Activity | `Activity` | 18px |
| Settings | `SlidersHorizontal` | 18px |

```tsx
<Button
  variant="ghost"
  size="sm"
  onClick={() => setCurrentView('kanban')}
  className={cn(
    "gap-2 px-3 h-8",
    currentView === 'kanban'
      ? "bg-[var(--bg-elevated)] text-[var(--accent-primary)]"
      : "text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]"
  )}
  aria-current={currentView === 'kanban' ? 'page' : undefined}
  title="Kanban (⌘1)"
>
  <LayoutGrid className="w-[18px] h-[18px]" />
  <span className="text-sm font-medium">Kanban</span>
</Button>
```

### Keyboard Shortcuts

Each view has an associated keyboard shortcut shown in tooltip:

| View | Shortcut | Tooltip |
|------|----------|---------|
| Kanban | ⌘1 | "Kanban (⌘1)" |
| Ideation | ⌘2 | "Ideation (⌘2)" |
| Extensibility | ⌘3 | "Extensibility (⌘3)" |
| Activity | ⌘4 | "Activity (⌘4)" |
| Settings | ⌘5 | "Settings (⌘5)" |

### Optional: Tooltip Enhancement

For enhanced discoverability, wrap each nav item in **shadcn Tooltip**:

```tsx
<TooltipProvider>
  <Tooltip>
    <TooltipTrigger asChild>
      <Button variant="ghost" ... />
    </TooltipTrigger>
    <TooltipContent side="bottom" className="text-xs">
      Kanban <kbd className="ml-1 text-[var(--text-muted)]">⌘1</kbd>
    </TooltipContent>
  </Tooltip>
</TooltipProvider>
```

---

## Center Section (Project Selector)

### Project Selector Container

| Property | Value |
|----------|-------|
| Display | Flex, align-items center |
| Position | Absolute center of header (or flex-grow center section) |

### Project Selector Dropdown

Using **shadcn DropdownMenu** with custom trigger styling.

### Trigger Button

| Property | Value |
|----------|-------|
| Component | shadcn Button (ghost variant) |
| Padding | 8px 12px |
| Height | 32px |
| Border radius | `--radius-md` (8px) |
| Background | transparent |
| Border | 1px `--border-default` |
| Color | `--text-primary` |
| Font | `text-sm` (14px), `font-medium` (500) |
| Display | Flex, align-items center, gap 8px |
| Max width | 200px (truncate long names) |

### Trigger Content

```
┌─────────────────────────────────┐
│  📁 Project Name  ▾  │
└─────────────────────────────────┘
```

| Element | Styling |
|---------|---------|
| Folder icon | Lucide `FolderOpen` (16px, `--text-secondary`) |
| Project name | Truncated, `--text-primary` |
| Git status indicator | Small colored dot (optional, inline) |
| Chevron | Lucide `ChevronDown` (14px, `--text-muted`) |

```tsx
<DropdownMenuTrigger asChild>
  <Button
    variant="ghost"
    className="gap-2 px-3 h-8 border border-[var(--border-default)] max-w-[200px]"
  >
    <FolderOpen className="w-4 h-4 text-[var(--text-secondary)] flex-shrink-0" />
    <span className="text-sm font-medium truncate">{projectName}</span>
    {gitStatus === 'dirty' && (
      <span className="w-2 h-2 rounded-full bg-[var(--status-warning)] flex-shrink-0" />
    )}
    <ChevronDown className="w-3.5 h-3.5 text-[var(--text-muted)] flex-shrink-0" />
  </Button>
</DropdownMenuTrigger>
```

### Dropdown Menu Content

| Property | Value |
|----------|-------|
| Component | shadcn DropdownMenuContent |
| Width | 240px (w-60) |
| Background | `--bg-elevated` |
| Border | 1px `--border-default` |
| Border radius | `--radius-lg` (12px) |
| Shadow | `--shadow-md` |
| Animation | scale 0.95→1, opacity 0→1, 150ms ease-out |

### Menu Sections

```
┌─────────────────────────────────────┐
│  RECENT PROJECTS                     │
├─────────────────────────────────────┤
│  ● RalphX           main            │
│  ○ Other Project    feature/xyz     │
│  ○ Third Project    develop         │
├─────────────────────────────────────┤
│  + New Project...                    │
└─────────────────────────────────────┘
```

### Project List Item

| Property | Value |
|----------|-------|
| Padding | 8px 12px |
| Height | 36px |
| Border radius | `--radius-sm` (4px) |
| Display | Flex, align-items center, justify-content space-between |
| Hover | Background `--bg-hover` |
| Active indicator | 4px left border `--accent-primary` |

### Project Item Content

| Element | Position | Styling |
|---------|----------|---------|
| Active dot | Left | 6px circle, `--accent-primary` (active) or transparent |
| Project name | Left | `text-sm`, `font-medium`, `--text-primary` |
| Branch name | Right | `text-xs`, `font-mono`, `--text-muted` |
| Dirty indicator | Right (before branch) | 6px circle, `--status-warning` |

```tsx
<DropdownMenuItem
  className={cn(
    "flex items-center justify-between gap-2 px-3 py-2 cursor-pointer",
    isActive && "border-l-2 border-[var(--accent-primary)] bg-[var(--accent-muted)]"
  )}
>
  <div className="flex items-center gap-2">
    <span
      className={cn(
        "w-1.5 h-1.5 rounded-full",
        isActive ? "bg-[var(--accent-primary)]" : "bg-transparent"
      )}
    />
    <span className="text-sm font-medium">{project.name}</span>
  </div>
  <div className="flex items-center gap-1.5">
    {project.isDirty && (
      <span className="w-1.5 h-1.5 rounded-full bg-[var(--status-warning)]" />
    )}
    <span className="text-xs font-mono text-[var(--text-muted)]">
      {project.branch}
    </span>
  </div>
</DropdownMenuItem>
```

### New Project Item

| Property | Value |
|----------|-------|
| Separator above | shadcn DropdownMenuSeparator |
| Icon | Lucide `Plus` (16px) |
| Text | "New Project..." |
| Color | `--text-secondary` |
| Hover | `--accent-primary` for icon, `--text-primary` for text |

```tsx
<DropdownMenuSeparator />
<DropdownMenuItem
  className="flex items-center gap-2 px-3 py-2 cursor-pointer text-[var(--text-secondary)] hover:text-[var(--text-primary)]"
  onClick={onNewProject}
>
  <Plus className="w-4 h-4" />
  <span className="text-sm">New Project...</span>
</DropdownMenuItem>
```

---

## Right Section (Panel Toggles)

### Section Container

| Property | Value |
|----------|-------|
| Display | Flex, align-items center |
| Gap | 8px between buttons |

### Chat Toggle Button

Toggle for the global Chat panel with keyboard shortcut indicator.

#### Default State (Panel Closed)

| Property | Value |
|----------|-------|
| Component | shadcn Button (ghost variant) |
| Padding | 8px 12px |
| Height | 32px |
| Border radius | `--radius-md` (8px) |
| Background | transparent |
| Color | `--text-secondary` |
| Display | Flex, align-items center, gap 8px |

#### Active State (Panel Open)

| Property | Value |
|----------|-------|
| Background | `--bg-elevated` |
| Color | `--accent-primary` |

#### Button Content

```
┌─────────────────────────┐
│  💬 Chat  ⌘K           │
└─────────────────────────┘
```

| Element | Styling |
|---------|---------|
| Icon | Lucide `MessageSquare` (18px) |
| Label | "Chat", `text-sm`, `font-medium` |
| Shortcut kbd | `⌘K`, `text-xs`, `--text-muted`, background `--bg-elevated` |

```tsx
<Button
  variant="ghost"
  size="sm"
  onClick={toggleChatPanel}
  className={cn(
    "gap-2 px-3 h-8",
    chatIsOpen
      ? "bg-[var(--bg-elevated)] text-[var(--accent-primary)]"
      : "text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]"
  )}
  data-testid="chat-toggle"
  title="Toggle Chat (⌘K)"
>
  <MessageSquare className="w-[18px] h-[18px]" />
  <span className="text-sm font-medium">Chat</span>
  <kbd
    className="ml-1 px-1.5 py-0.5 text-xs rounded bg-[var(--bg-elevated)] text-[var(--text-muted)]"
  >
    ⌘K
  </kbd>
</Button>
```

### Reviews Toggle Button

Toggle for the Reviews panel with pending count badge.

#### Default State (Panel Closed, No Pending)

| Property | Value |
|----------|-------|
| Component | shadcn Button (ghost variant) |
| Same styling as Chat toggle |

#### Active State (Panel Open)

| Property | Value |
|----------|-------|
| Background | `--bg-elevated` |
| Color | `--accent-primary` |

#### With Pending Badge

```
┌────────────────────┐
│  ✓ Reviews   (3)  │  ← Badge overlaps button
└────────────────────┘
```

### Review Badge

| Property | Value |
|----------|-------|
| Position | absolute, -top-1, -right-1 |
| Size | 18px × 18px (min) |
| Border radius | `--radius-full` |
| Background | `--status-warning` (#f59e0b) |
| Color | white |
| Font | `text-xs` (12px), `font-bold` |
| Display | Flex, align-items center, justify-center |
| Content | Count (1-9) or "9+" for larger |
| Animation | scale 0→1 on count change (150ms ease-spring) |

```tsx
<Button
  variant="ghost"
  size="sm"
  onClick={toggleReviewsPanel}
  className={cn(
    "relative gap-2 px-3 h-8",
    reviewsPanelOpen
      ? "bg-[var(--bg-elevated)] text-[var(--accent-primary)]"
      : "text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]"
  )}
  data-testid="reviews-toggle"
>
  <CheckCircle className="w-[18px] h-[18px]" />
  <span className="text-sm font-medium">Reviews</span>
  {pendingReviewCount > 0 && (
    <span
      className="absolute -top-1 -right-1 flex items-center justify-center min-w-[18px] h-[18px] px-1 text-xs font-bold rounded-full bg-[var(--status-warning)] text-white"
      data-testid="reviews-badge"
    >
      {pendingReviewCount > 9 ? '9+' : pendingReviewCount}
    </span>
  )}
</Button>
```

---

## State Variations

### Default State

```
┌──────────────────────────────────────────────────────────────────────────────────────┐
│  RalphX   [Kanban] [Ideation] [Extensibility] [Activity] [Settings]  │  ▾ RalphX  │  💬 Chat ⌘K  │  ✓ Reviews  │
└──────────────────────────────────────────────────────────────────────────────────────┘
```

| Element | State |
|---------|-------|
| Kanban nav item | Active (orange text, elevated background) |
| All other nav items | Inactive (secondary text) |
| Chat toggle | Closed (secondary text) |
| Reviews toggle | Closed, no badge |

### Chat Panel Open

| Element | State |
|---------|-------|
| Chat toggle | Active (orange text, elevated background) |

### Reviews Panel Open with Pending

| Element | State |
|---------|-------|
| Reviews toggle | Active styling + warning badge showing count |

### Multiple Panels Open

Both Chat and Reviews can be open simultaneously - both buttons show active state.

---

## Responsive Behavior

### Compact Mode (< 768px)

| Change | Value |
|--------|-------|
| Nav labels | Hide, show icons only |
| Project selector | Show icon only, full dropdown on click |
| Chat/Reviews labels | Hide, icons only |
| Kbd shortcut | Hide |
| Gap reductions | Nav gap 2px, section gap 8px |

```tsx
<Button variant="ghost" size="icon" className="w-8 h-8">
  <LayoutGrid className="w-4 h-4" />
</Button>
```

### Standard Mode (>= 768px)

Full layout as specified above.

### Large Mode (>= 1280px)

| Enhancement | Value |
|-------------|-------|
| Section gaps | Increase to 32px |
| Nav item padding | 12px 16px |

---

## Keyboard Navigation

### Global Shortcuts

| Key | Action |
|-----|--------|
| `⌘1` | Switch to Kanban view |
| `⌘2` | Switch to Ideation view |
| `⌘3` | Switch to Extensibility view |
| `⌘4` | Switch to Activity view |
| `⌘5` | Switch to Settings view |
| `⌘K` | Toggle Chat panel |
| `⌘R` | Toggle Reviews panel (optional) |

### Focus Management

| Behavior | Implementation |
|----------|----------------|
| Tab order | Branding (skip) → Nav items → Project selector → Chat → Reviews |
| Focus visible | Use `--shadow-glow` ring |
| Escape | Close any open dropdown |
| Arrow keys | Navigate within dropdown menus |

---

## Micro-interactions

### Nav Item Hover

```css
.nav-item {
  transition: background-color 150ms ease, color 150ms ease;
}

.nav-item:hover {
  background: var(--bg-hover);
  color: var(--text-primary);
}
```

### Nav Item Active Press

```css
.nav-item:active {
  transform: scale(0.98);
}
```

### Project Selector Dropdown Open

```css
@keyframes dropdown-open {
  from {
    opacity: 0;
    transform: scale(0.95) translateY(-4px);
  }
  to {
    opacity: 1;
    transform: scale(1) translateY(0);
  }
}

.dropdown-content {
  animation: dropdown-open 150ms ease-out;
}
```

### Badge Count Change

```css
@keyframes badge-pop {
  0% { transform: scale(0.5); }
  50% { transform: scale(1.1); }
  100% { transform: scale(1); }
}

.badge-count {
  animation: badge-pop 200ms ease-spring;
}
```

### Tooltip Appear

```css
@keyframes tooltip-appear {
  from {
    opacity: 0;
    transform: translateY(4px);
  }
  to {
    opacity: 1;
    transform: translateY(0);
  }
}

.tooltip {
  animation: tooltip-appear 150ms ease-out;
}
```

---

## Lucide Icons Used

| Icon | Component | Usage | Size |
|------|-----------|-------|------|
| `LayoutGrid` | Navigation | Kanban view | 18px |
| `Lightbulb` | Navigation | Ideation view | 18px |
| `Puzzle` | Navigation | Extensibility view | 18px |
| `Activity` | Navigation | Activity view | 18px |
| `SlidersHorizontal` | Navigation | Settings view | 18px |
| `FolderOpen` | Project Selector | Project icon | 16px |
| `ChevronDown` | Project Selector | Dropdown indicator | 14px |
| `Plus` | Project Selector | New project action | 16px |
| `MessageSquare` | Toggle | Chat panel | 18px |
| `CheckCircle` | Toggle | Reviews panel | 18px |
| `GitBranch` | Project Item | Branch indicator (optional) | 14px |

---

## Component Hierarchy

```
Header
├── HeaderContainer (fixed positioning, shadow, drag region)
│   │
│   ├── LeftSection
│   │   ├── AppBranding
│   │   │   └── "RalphX" (h1)
│   │   │
│   │   └── ViewNavigation (nav role)
│   │       ├── NavItem (Kanban)
│   │       │   ├── LayoutGrid icon
│   │       │   └── "Kanban" label
│   │       ├── NavItem (Ideation)
│   │       │   ├── Lightbulb icon
│   │       │   └── "Ideation" label
│   │       ├── NavItem (Extensibility)
│   │       │   ├── Puzzle icon
│   │       │   └── "Extensibility" label
│   │       ├── NavItem (Activity)
│   │       │   ├── Activity icon
│   │       │   └── "Activity" label
│   │       └── NavItem (Settings)
│   │           ├── SlidersHorizontal icon
│   │           └── "Settings" label
│   │
│   ├── CenterSection
│   │   └── ProjectSelector (shadcn DropdownMenu)
│   │       ├── Trigger
│   │       │   ├── FolderOpen icon
│   │       │   ├── Project name (truncated)
│   │       │   ├── Git status dot (optional)
│   │       │   └── ChevronDown icon
│   │       │
│   │       └── Content (dropdown)
│   │           ├── Label ("RECENT PROJECTS")
│   │           ├── ProjectItem[] (for each project)
│   │           │   ├── Active indicator
│   │           │   ├── Project name
│   │           │   ├── Dirty indicator
│   │           │   └── Branch name
│   │           ├── Separator
│   │           └── NewProjectItem
│   │               ├── Plus icon
│   │               └── "New Project..." label
│   │
│   └── RightSection
│       ├── ChatToggle (shadcn Button)
│       │   ├── MessageSquare icon
│       │   ├── "Chat" label
│       │   └── Kbd ("⌘K")
│       │
│       └── ReviewsToggle (shadcn Button)
│           ├── CheckCircle icon
│           ├── "Reviews" label
│           └── Badge (conditional)
│               └── Count number
```

---

## Acceptance Criteria

### Functional Requirements

1. [ ] Header is fixed at top of viewport with z-index above content
2. [ ] Header height is 48px with proper padding
3. [ ] Window can be dragged by the header (macOS Tauri)
4. [ ] Clickable elements (buttons, dropdowns) do not trigger window drag
5. [ ] App branding "RalphX" displays in warm orange accent
6. [ ] All five view navigation items are visible and clickable
7. [ ] Active view is visually distinguished (elevated background, accent color)
8. [ ] Keyboard shortcuts ⌘1-5 switch views correctly
9. [ ] Project selector dropdown opens on click
10. [ ] Project list shows all available projects with branch names
11. [ ] Active project is highlighted in dropdown
12. [ ] "New Project..." item opens project creation wizard
13. [ ] Chat toggle shows ⌘K shortcut
14. [ ] Chat toggle reflects panel open/closed state
15. [ ] ⌘K keyboard shortcut toggles chat panel
16. [ ] Reviews toggle shows pending count badge when count > 0
17. [ ] Reviews toggle reflects panel open/closed state
18. [ ] Badge shows "9+" when count exceeds 9
19. [ ] All buttons have visible focus states
20. [ ] Tab order follows left-to-right flow

---

## Design Quality Checklist

### Colors & Theming

1. [ ] NO purple or blue gradients anywhere
2. [ ] Background uses `--bg-surface` (subtle elevation)
3. [ ] Border uses `--border-subtle` (delicate, not harsh)
4. [ ] Warm orange accent (`--accent-primary`) only for:
   - App branding
   - Active nav items
   - Active panel toggles
   - Active project indicator
5. [ ] Text colors follow hierarchy (primary for active, secondary for inactive)
6. [ ] Badge uses `--status-warning` for review count

### Typography

7. [ ] App branding uses SF Pro Display (`--font-display`)
8. [ ] All other text uses SF Pro Text (`--font-body`)
9. [ ] Branding: `text-xl`, `font-bold`, `tracking-tight`
10. [ ] Nav labels: `text-sm`, `font-medium`
11. [ ] Project name: `text-sm`, `font-medium`
12. [ ] Branch name: `text-xs`, `font-mono`
13. [ ] Kbd shortcut: `text-xs`
14. [ ] Badge count: `text-xs`, `font-bold`

### Spacing & Layout

15. [ ] Header height: 48px
16. [ ] Horizontal padding: 16px
17. [ ] Gap between branding and nav: 24px
18. [ ] Gap between nav items: 4px
19. [ ] Gap between right section buttons: 8px
20. [ ] Nav item padding: 8px 12px
21. [ ] Button height: 32px
22. [ ] 8pt grid alignment maintained throughout

### Shadows & Depth

23. [ ] Header has subtle bottom shadow
24. [ ] Dropdown has `--shadow-md`
25. [ ] No excessive shadows on buttons
26. [ ] Focus states use `--shadow-glow`

### Borders & Radius

27. [ ] Header has 1px bottom border
28. [ ] Button border radius: `--radius-md` (8px)
29. [ ] Dropdown border radius: `--radius-lg` (12px)
30. [ ] Badge border radius: `--radius-full`
31. [ ] Project selector has 1px border

### Motion & Interactions

32. [ ] Nav hover transitions: 150ms ease
33. [ ] Nav press: scale(0.98)
34. [ ] Dropdown open animation: 150ms ease-out, scale + translate
35. [ ] Badge pop animation: 200ms ease-spring
36. [ ] All state changes have smooth transitions
37. [ ] No jarring or instant state changes

### Icons

38. [ ] All icons from Lucide library
39. [ ] Nav icons: 18px
40. [ ] Project icon: 16px
41. [ ] Dropdown chevron: 14px
42. [ ] Toggle icons: 18px
43. [ ] Icons inherit color from parent text
44. [ ] Consistent stroke width (default Lucide)

### Accessibility

45. [ ] Color contrast meets WCAG AA (4.5:1)
46. [ ] Focus states visible on all interactive elements
47. [ ] Navigation has `role="navigation"` and `aria-label`
48. [ ] Active nav item has `aria-current="page"`
49. [ ] Buttons have accessible names (visible text or aria-label)
50. [ ] Keyboard navigation works throughout
51. [ ] Dropdown is keyboard accessible
52. [ ] Badge count announced to screen readers

---

## Implementation Notes

### shadcn Components to Use

- `Button` (nav items, toggles, project selector trigger)
- `DropdownMenu`, `DropdownMenuTrigger`, `DropdownMenuContent`, `DropdownMenuItem`, `DropdownMenuSeparator`, `DropdownMenuLabel`
- `Tooltip`, `TooltipTrigger`, `TooltipContent`, `TooltipProvider` (optional, for shortcuts)

### CSS Custom Properties

```css
/* Header specific */
--header-height: 48px;
--header-z-index: 50;

/* Reused from DESIGN.md */
--bg-surface: #1a1a1a;
--bg-elevated: #242424;
--bg-hover: #2d2d2d;
--text-primary: #f0f0f0;
--text-secondary: #a0a0a0;
--text-muted: #666666;
--accent-primary: #ff6b35;
--accent-muted: rgba(255, 107, 53, 0.15);
--status-warning: #f59e0b;
--border-subtle: rgba(255, 255, 255, 0.06);
--border-default: rgba(255, 255, 255, 0.1);
--radius-sm: 4px;
--radius-md: 8px;
--radius-lg: 12px;
--radius-full: 9999px;
--shadow-md: 0 4px 6px rgba(0,0,0,0.3), 0 8px 16px rgba(0,0,0,0.2);
--shadow-glow: 0 0 0 2px var(--bg-base), 0 0 0 4px var(--accent-primary);
--font-display: SF Pro Display, -apple-system, sans-serif;
--font-body: SF Pro Text, -apple-system, sans-serif;
--font-mono: JetBrains Mono, Menlo, monospace;
--tracking-tight: -0.02em;
```

### Animation Keyframes

```css
@keyframes dropdown-open {
  from {
    opacity: 0;
    transform: scale(0.95) translateY(-4px);
  }
  to {
    opacity: 1;
    transform: scale(1) translateY(0);
  }
}

@keyframes badge-pop {
  0% { transform: scale(0.5); }
  50% { transform: scale(1.1); }
  100% { transform: scale(1); }
}

@keyframes tooltip-appear {
  from {
    opacity: 0;
    transform: translateY(4px);
  }
  to {
    opacity: 1;
    transform: translateY(0);
  }
}
```

### Keyboard Shortcuts Implementation

```tsx
useEffect(() => {
  const handleKeyDown = (e: KeyboardEvent) => {
    if (e.metaKey || e.ctrlKey) {
      switch (e.key) {
        case '1':
          e.preventDefault();
          setCurrentView('kanban');
          break;
        case '2':
          e.preventDefault();
          setCurrentView('ideation');
          break;
        case '3':
          e.preventDefault();
          setCurrentView('extensibility');
          break;
        case '4':
          e.preventDefault();
          setCurrentView('activity');
          break;
        case '5':
          e.preventDefault();
          setCurrentView('settings');
          break;
        case 'k':
          e.preventDefault();
          toggleChatPanel();
          break;
      }
    }
  };

  window.addEventListener('keydown', handleKeyDown);
  return () => window.removeEventListener('keydown', handleKeyDown);
}, [setCurrentView, toggleChatPanel]);
```

### Window Drag Region (Tauri)

```tsx
<header
  className="..."
  style={{
    WebkitAppRegion: 'drag',
  } as React.CSSProperties}
>
  <Button
    style={{ WebkitAppRegion: 'no-drag' } as React.CSSProperties}
    ...
  >
    ...
  </Button>
</header>
```

Note: TypeScript may require casting for `-webkit-app-region` as it's not in the standard CSSProperties type.

---

## References

- [DESIGN.md](../DESIGN.md) - Master design system
- [specs/DESIGN_OVERHAUL_PLAN.md](../../DESIGN_OVERHAUL_PLAN.md) - Design overhaul strategy
- [project-sidebar.md](./project-sidebar.md) - Related project navigation
- [chat-panel.md](./chat-panel.md) - Chat panel that header toggles
- [reviews-panel.md](./reviews-panel.md) - Reviews panel that header toggles
- Linear app header - Navigation patterns
- Raycast - Mac-native chrome, SF Pro usage
- Arc browser - Spatial organization
- shadcn/ui Button - https://ui.shadcn.com/docs/components/button
- shadcn/ui DropdownMenu - https://ui.shadcn.com/docs/components/dropdown-menu
- shadcn/ui Tooltip - https://ui.shadcn.com/docs/components/tooltip
- Lucide icons - https://lucide.dev/icons/
- Tauri window decoration - https://tauri.app/v1/guides/features/window-customization/
