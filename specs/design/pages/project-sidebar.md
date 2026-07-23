# Project Sidebar

The Project Sidebar is the primary navigation hub of RalphX, providing project selection, workspace status, and view navigation. It lives on the left edge of the viewport and serves as the command center for multi-project workflows.

**Design Inspiration:**
- **Linear** - Project switching, keyboard-first navigation, refined list interactions
- **Raycast** - Mac-native feel, spacious yet dense, subtle depth and glows
- **Arc** - Warm accents, spatial organization, collapsible sections
- **Finder Sidebar** - Native macOS patterns, selection states, icon sizing

**Aesthetic Direction:** Refined utility with warmth. The sidebar should feel like a native Mac app extension - quiet confidence, premium materials (subtle depth, refined borders), and purposeful restraint. It's always there, always useful, never demanding attention unless something needs it.

---

## Sidebar Structure

### Container

The sidebar is a fixed-width panel anchored to the left edge of the viewport.

| Property | Value | Notes |
|----------|-------|-------|
| Width | 256px (16rem) | Comfortable for project names and badges |
| Min-width | 240px | If resizable in future |
| Max-width | 320px | If resizable in future |
| Height | 100vh | Full viewport height |
| Position | Fixed left 0, or flex child | Depends on layout approach |
| Background | `--bg-surface` (#1a1a1a) | Mid-elevation for sidebar |
| Border right | 1px `--border-subtle` | Subtle separation from main content |
| Z-index | 30 | Below panels (40), below modals (50) |

### Flex Layout

```
┌─────────────────────────────────────┐
│  Header (Projects title + close)    │ ← Fixed height
├─────────────────────────────────────┤
│  Worktree Status (conditional)      │ ← Conditional
├─────────────────────────────────────┤
│  Project List (scrollable)          │ ← Flex-1, overflow-y-auto
│    └─ ProjectItem[]                 │
│    └─ EmptyState (if no projects)   │
├─────────────────────────────────────┤
│  New Project Button                 │ ← Fixed height
├─────────────────────────────────────┤
│  Divider                            │ ← 1px
├─────────────────────────────────────┤
│  Navigation                         │ ← Fixed height
└─────────────────────────────────────┘
```

### Collapse/Expand (Future Enhancement)

For future consideration:

| State | Width | Behavior |
|-------|-------|----------|
| Expanded | 256px | Full sidebar with labels |
| Collapsed | 56px | Icons only, tooltips on hover |
| Hidden | 0px | Completely hidden, toggle to show |

Animation: 200ms ease-out for width transitions.

---

## Header Section

### Layout

```
┌─────────────────────────────────────┐
│  PROJECTS                      [✕]  │
└─────────────────────────────────────┘
```

### Structure

| Element | Styling | Notes |
|---------|---------|-------|
| Container | `px-4 py-3` | 16px horizontal, 12px vertical |
| Border bottom | 1px `--border-subtle` | Separates from content |
| Background | transparent (inherits surface) | Clean |
| Height | 48px | Consistent header height |
| Flex | `flex items-center justify-between` | Title left, close right |

### Section Title

| Property | Value |
|----------|-------|
| Text | "PROJECTS" |
| Font size | `text-xs` (12px) |
| Font weight | `font-semibold` (600) |
| Color | `--text-muted` |
| Letter spacing | `--tracking-wide` (0.05em) |
| Text transform | uppercase |

The uppercase, tracked-out label is a native macOS pattern (see Finder sidebar section headers).

### Close Button

| Property | Value |
|----------|-------|
| Position | Right edge of header |
| Icon | Lucide `X` (16px) |
| Size | 28px × 28px |
| Background | transparent |
| Hover background | `--bg-hover` |
| Border radius | `--radius-md` (8px) |
| Color | `--text-muted` |
| Hover color | `--text-primary` |
| Focus | `--shadow-glow` |
| Transition | 150ms ease |

```tsx
<button
  className="p-1.5 rounded-lg transition-colors hover:bg-[var(--bg-hover)]"
  style={{ color: 'var(--text-muted)' }}
  onClick={onClose}
  aria-label="Close sidebar"
>
  <X className="w-4 h-4" />
</button>
```

---

## Worktree Status Indicator

Displayed when the active project uses git worktree mode. Shows the current worktree branch and its base branch.

### Layout

```
┌─────────────────────────────────────┐
│  ⎇  feature/sidebar-redesign        │
│     from main                       │
└─────────────────────────────────────┘
```

### Container

| Property | Value |
|----------|-------|
| Container padding | `px-3 py-2` |
| Background | `--bg-base` (#0f0f0f) |
| Border radius | `--radius-md` (8px) |
| Margin | 8px horizontal, 8px vertical |
| Flex | `flex items-start gap-2` |

### Icon

| Property | Value |
|----------|-------|
| Icon | Lucide `GitBranch` (14px) |
| Color | `--text-muted` |
| Margin top | 2px (align with first line of text) |
| Flex shrink | 0 |

### Branch Text

| Property | Value |
|----------|-------|
| Branch name | `text-xs`, `font-medium`, `--text-primary` |
| Truncation | Single line ellipsis |
| "from {base}" | `text-xs`, `--text-muted` |
| Line height | `--leading-tight` (1.2) |

```tsx
<div
  className="flex items-start gap-2 mx-3 my-2 px-3 py-2 rounded-lg"
  style={{ backgroundColor: 'var(--bg-base)' }}
>
  <GitBranch className="w-3.5 h-3.5 mt-0.5 flex-shrink-0" style={{ color: 'var(--text-muted)' }} />
  <div className="flex-1 min-w-0">
    <div className="text-xs font-medium truncate" style={{ color: 'var(--text-primary)' }}>
      {worktreeBranch}
    </div>
    <div className="text-xs" style={{ color: 'var(--text-muted)' }}>
      from {baseBranch}
    </div>
  </div>
</div>
```

### Visibility Conditions

- **Show when:** Active project exists AND `gitMode === "worktree"` AND `worktreeBranch` is set
- **Hide when:** No active project OR `gitMode === "local"`

---

## Project List

### Scroll Container

| Property | Value |
|----------|-------|
| Container | `flex-1 overflow-y-auto` |
| Padding | `px-2 py-2` |
| Scrollbar | Styled thin scrollbar matching Mac |
| Scroll behavior | smooth |

### Scrollbar Styling

```css
.project-list::-webkit-scrollbar {
  width: 6px;
}

.project-list::-webkit-scrollbar-track {
  background: transparent;
}

.project-list::-webkit-scrollbar-thumb {
  background: var(--border-subtle);
  border-radius: 3px;
}

.project-list::-webkit-scrollbar-thumb:hover {
  background: var(--border-default);
}
```

### Project Item

Each project in the list is an interactive button.

#### Layout

```
┌─────────────────────────────────────┐
│  📁  Project Name Here              │
│      [Local] or [⎇ Worktree] branch │
└─────────────────────────────────────┘
```

#### Container Styling

| Property | Value |
|----------|-------|
| Element | `<button>` |
| Width | 100% |
| Padding | 12px horizontal, 8px vertical |
| Border radius | `--radius-md` (8px) |
| Flex | `flex items-start gap-3` |
| Text align | left |
| Cursor | pointer |
| Transition | 150ms ease (background, color) |

#### States

| State | Background | Text Color | Icon Color |
|-------|------------|------------|------------|
| Default | transparent | `--text-secondary` | `--text-muted` |
| Hover | `--bg-hover` | `--text-primary` | `--text-secondary` |
| Active (selected) | `--bg-elevated` | `--text-primary` | `--accent-primary` |
| Focus | transparent | `--text-primary` | `--shadow-glow` |

```tsx
<button
  className="w-full flex items-start gap-3 px-3 py-2 rounded-lg transition-colors text-left"
  style={{
    backgroundColor: isActive ? 'var(--bg-elevated)' : 'transparent',
    color: isActive ? 'var(--text-primary)' : 'var(--text-secondary)',
  }}
  onClick={() => selectProject(project.id)}
>
  {/* content */}
</button>
```

#### Hover Animation

```css
.project-item {
  transition: background-color 150ms ease, transform 150ms ease;
}

.project-item:hover {
  transform: translateX(2px);
}

.project-item:active {
  transform: translateX(0);
}
```

Subtle 2px rightward shift on hover creates a "drawer peek" effect.

#### Project Icon

| Property | Value |
|----------|-------|
| Icon | Lucide `Folder` or `FolderOpen` (16px) |
| Alternative | Lucide `FolderGit2` for worktree projects |
| Color | `--text-muted` (default), `--accent-primary` (active) |
| Margin top | 2px (align with project name baseline) |
| Flex shrink | 0 |

#### Project Name

| Property | Value |
|----------|-------|
| Font size | `text-sm` (14px) |
| Font weight | `font-medium` (500) |
| Color | inherit from container |
| Truncation | Single line ellipsis |
| Max width | 100% of available space |

#### Git Mode Badge

Below the project name, show the git mode indicator.

##### Local Mode

| Property | Value |
|----------|-------|
| Content | "Local" |
| Background | `--bg-base` |
| Text color | `--text-muted` |
| Font size | `text-xs` (12px) |
| Padding | 2px 6px |
| Border radius | `--radius-sm` (4px) |

##### Worktree Mode

| Property | Value |
|----------|-------|
| Content | GitBranch icon (10px) + "Worktree" |
| Background | `--bg-base` |
| Text color | `--text-muted` |
| Font size | `text-xs` (12px) |
| Padding | 2px 6px |
| Border radius | `--radius-sm` (4px) |
| Gap | 4px between icon and text |

##### Branch Name (Worktree)

| Property | Value |
|----------|-------|
| Content | Branch name |
| Position | After Worktree badge, inline |
| Font size | `text-xs` (12px) |
| Color | `--text-muted` |
| Truncation | Single line ellipsis |

```tsx
<div className="flex items-center gap-1.5 mt-1">
  {project.gitMode === 'worktree' ? (
    <>
      <span
        className="inline-flex items-center gap-1 text-xs px-1.5 py-0.5 rounded"
        style={{ backgroundColor: 'var(--bg-base)', color: 'var(--text-muted)' }}
      >
        <GitBranch className="w-2.5 h-2.5" />
        Worktree
      </span>
      <span className="text-xs truncate" style={{ color: 'var(--text-muted)' }}>
        {project.worktreeBranch}
      </span>
    </>
  ) : (
    <span
      className="text-xs px-1.5 py-0.5 rounded"
      style={{ backgroundColor: 'var(--bg-base)', color: 'var(--text-muted)' }}
    >
      Local
    </span>
  )}
</div>
```

### Active Project Indicator

The active project has an additional visual indicator.

| Property | Value |
|----------|-------|
| Left accent bar | 3px width, `--accent-primary` |
| Position | Absolute left edge of item |
| Height | 60% of item height, vertically centered |
| Border radius | 0 2px 2px 0 (right side rounded) |
| Animation | Fade in 150ms on selection |

```css
.project-item[data-active="true"]::before {
  content: '';
  position: absolute;
  left: 0;
  top: 20%;
  bottom: 20%;
  width: 3px;
  background: var(--accent-primary);
  border-radius: 0 2px 2px 0;
}
```

### Dirty Indicator (Future)

When a project has uncommitted changes:

| Property | Value |
|----------|-------|
| Position | After project name, inline |
| Icon | Dot (6px circle) |
| Color | `--status-warning` (#f59e0b) |
| Animation | Subtle pulse (optional) |

---

## Empty State

Displayed when no projects exist.

### Layout

```
┌─────────────────────────────────────┐
│                                     │
│           ┌─────────┐               │
│           │  (📁)   │               │
│           └─────────┘               │
│          No projects yet            │
│     Create a project to get started │
│                                     │
└─────────────────────────────────────┘
```

### Styling

| Element | Styling |
|---------|---------|
| Container | `flex flex-col items-center justify-center p-6 text-center` |
| Icon container | 48px × 48px circle, `--bg-base` background |
| Icon | Lucide `FolderPlus` or `Folder` (dashed), 24px |
| Icon color | `--text-muted`, 50% opacity |
| Title | "No projects yet", `text-sm`, `font-medium`, `--text-secondary` |
| Subtitle | "Create a project to get started", `text-xs`, `--text-muted` |
| Gap | 12px between icon and title, 4px between title and subtitle |

```tsx
<div className="flex flex-col items-center justify-center p-6 text-center">
  <div
    className="w-12 h-12 rounded-full flex items-center justify-center mb-3"
    style={{ backgroundColor: 'var(--bg-base)' }}
  >
    <Folder className="w-6 h-6" style={{ color: 'var(--text-muted)', opacity: 0.5 }} />
  </div>
  <p className="text-sm font-medium" style={{ color: 'var(--text-secondary)' }}>
    No projects yet
  </p>
  <p className="text-xs mt-1" style={{ color: 'var(--text-muted)' }}>
    Create a project to get started
  </p>
</div>
```

---

## New Project Button

### Container

| Property | Value |
|----------|-------|
| Container padding | `px-3 py-2` |
| Border top | 1px `--border-subtle` |
| Background | transparent |

### Button Styling

Using **shadcn Button** as base.

| Property | Value |
|----------|-------|
| Width | 100% |
| Height | 36px |
| Background | `--bg-elevated` (secondary variant) |
| Hover background | `--bg-hover` |
| Text color | `--text-primary` |
| Font size | `text-sm` (14px) |
| Font weight | `font-medium` (500) |
| Border radius | `--radius-md` (8px) |
| Border | none |
| Transition | 150ms ease |
| Flex | `flex items-center justify-center gap-2` |

### Icon

| Property | Value |
|----------|-------|
| Icon | Lucide `Plus` (16px) |
| Color | inherit (`--text-primary`) |
| Stroke width | 2px |

### Hover State

| Property | Value |
|----------|-------|
| Background | `--bg-hover` |
| Transform | none (no lift for contained buttons) |

### Focus State

| Property | Value |
|----------|-------|
| Outline | none |
| Box shadow | `--shadow-glow` |

### Alternative: Primary Button Variant

For more prominent "New Project" action:

| Property | Value |
|----------|-------|
| Background | `--accent-primary` (#ff6b35) |
| Text color | white |
| Hover | Lighten 10% |
| Active | `scale(0.98)` |

Note: Use sparingly - only one primary button per sidebar. The secondary variant is recommended.

```tsx
<div className="px-3 py-2 border-t" style={{ borderColor: 'var(--border-subtle)' }}>
  <Button
    variant="secondary"
    className="w-full justify-center"
    onClick={onNewProject}
  >
    <Plus className="w-4 h-4" />
    New Project
  </Button>
</div>
```

---

## Divider

### Styling

| Property | Value |
|----------|-------|
| Container padding | `px-3 py-2` |
| Line height | 1px |
| Background | `--border-subtle` |
| Width | 100% |

```tsx
<div className="px-3 py-2">
  <div className="h-px w-full" style={{ backgroundColor: 'var(--border-subtle)' }} />
</div>
```

Alternative: Use **shadcn Separator** component.

---

## Navigation Section

### Container

| Property | Value |
|----------|-------|
| Container padding | `px-2 pb-4` |
| Background | transparent |
| Flex | `flex flex-col gap-1` |

### Navigation Items

The primary rail exposes the live root views. Task and planning views are
artifacts inside Agents and do not receive standalone rail entries.

| Key | Label | Icon | Shortcut |
|-----|-------|------|----------|
| `agents` | Agents | `Briefcase` | Cmd+1 |
| `automations` | Automations | `Workflow` | Cmd+2 |
| `ticketing` | Ticketing | `Ticket` | — |
| `github` | GitHub | `GitHubMarkIcon` | — |
| `granola` | Granola | `GranolaIcon` | — |
| `insights` | Insights | `TrendingUp` | Cmd+3 |
| `extensibility` | Extensibility | `Puzzle` | — |
| `activity` | Activity | `Activity` | — |

### Nav Item Styling

| Property | Value |
|----------|-------|
| Element | `<button>` |
| Width | 100% |
| Height | 36px |
| Padding | 12px horizontal |
| Border radius | `--radius-md` (8px) |
| Flex | `flex items-center gap-3` |
| Text align | left |
| Transition | 150ms ease |

### Nav Item States

| State | Background | Text/Icon Color |
|-------|------------|-----------------|
| Default | transparent | `--text-secondary` |
| Hover | `--bg-hover` | `--text-primary` |
| Active (current view) | `--bg-elevated` | `--accent-primary` |
| Focus | transparent | `--shadow-glow` |

### Nav Item Icon

| Property | Value |
|----------|-------|
| Size | 18px |
| Stroke width | 1.5px |
| Color | inherit from state |

### Nav Item Label

| Property | Value |
|----------|-------|
| Font size | `text-sm` (14px) |
| Font weight | `font-medium` (500) |
| Color | inherit from state |

### Active Indicator

Similar to project item, the active nav item has a left accent bar:

| Property | Value |
|----------|-------|
| Left accent bar | 3px width, `--accent-primary` |
| Position | Absolute left edge |
| Height | 60% of item height |
| Border radius | 0 2px 2px 0 |

```tsx
<nav className="flex flex-col gap-1 px-2 pb-4">
  {NAV_ITEMS.map((item) => {
    const isActive = currentView === item.key;
    const Icon = item.icon;
    return (
      <button
        key={item.key}
        className="relative flex items-center gap-3 px-3 py-2 rounded-lg transition-colors"
        style={{
          backgroundColor: isActive ? 'var(--bg-elevated)' : 'transparent',
          color: isActive ? 'var(--accent-primary)' : 'var(--text-secondary)',
        }}
        onClick={() => setCurrentView(item.key)}
      >
        {isActive && (
          <span
            className="absolute left-0 top-[20%] bottom-[20%] w-[3px] rounded-r"
            style={{ backgroundColor: 'var(--accent-primary)' }}
          />
        )}
        <Icon className="w-[18px] h-[18px]" />
        <span className="text-sm font-medium">{item.label}</span>
      </button>
    );
  })}
</nav>
```

---

## Keyboard Navigation

| Key | Action |
|-----|--------|
| `Cmd + 1` | Switch to Agents |
| `Cmd + 2` | Switch to Automations |
| `Cmd + 3` | Switch to Insights |
| `Cmd + Shift + A` | Switch to Agents |
| `Cmd + N` | Open New Project dialog |
| `Cmd + \` | Toggle sidebar visibility |
| `Arrow Up/Down` | Navigate project list |
| `Enter` | Select focused project |
| `Tab` | Move focus through interactive elements |

---

## Lucide Icons Used

| Icon | Usage | Size |
|------|-------|------|
| `X` | Close sidebar button | 16px |
| `GitBranch` | Worktree indicator, git mode badge | 10-14px |
| `Folder` | Project icon (default) | 16px |
| `FolderOpen` | Project icon (active) - optional | 16px |
| `FolderGit2` | Project icon (worktree) - optional | 16px |
| `FolderPlus` | Empty state icon | 24px |
| `Plus` | New Project button | 16px |
| `Briefcase` | Agents nav item | 18px |
| `Workflow` | Automations nav item | 18px |
| `TrendingUp` | Insights nav item | 18px |
| `Activity` | Activity nav item | 18px |
| `Settings` | Settings nav item | 18px |

---

## Component Hierarchy

```
ProjectSidebar
├── SidebarContainer (aside, fixed left)
│   │
│   ├── Header
│   │   ├── SectionTitle ("PROJECTS")
│   │   └── CloseButton (Lucide X)
│   │
│   ├── WorktreeStatus (conditional - only for worktree projects)
│   │   ├── GitBranchIcon
│   │   ├── BranchName (truncated)
│   │   └── BaseBranchText ("from {base}")
│   │
│   ├── ProjectListContainer (flex-1, overflow-y-auto)
│   │   ├── ProjectList (mapped from projects)
│   │   │   └── ProjectItem[] (sorted by updatedAt desc)
│   │   │       ├── ProjectIcon (Folder)
│   │   │       ├── ProjectDetails
│   │   │       │   ├── ProjectName (truncated)
│   │   │       │   └── GitModeBadge
│   │   │       │       ├── LocalBadge (if local)
│   │   │       │       └── WorktreeBadge + BranchName (if worktree)
│   │   │       └── ActiveIndicator (left bar, conditional)
│   │   │
│   │   └── EmptyState (conditional - when no projects)
│   │       ├── IconContainer
│   │       │   └── FolderIcon (muted)
│   │       ├── Title ("No projects yet")
│   │       └── Subtitle ("Create a project to get started")
│   │
│   ├── NewProjectButton
│   │   └── Button (shadcn, secondary variant)
│   │       ├── PlusIcon
│   │       └── Label ("New Project")
│   │
│   ├── Divider (Separator)
│   │
│   └── Navigation
│       └── NavItem[] (live AppView roots; planning/task modes stay in Agents)
│           ├── ActiveIndicator (left bar, conditional)
│           ├── NavIcon (view-specific)
│           └── NavLabel
│
└── (No backdrop - sidebar is persistent)
```

---

## Acceptance Criteria

### Functional Requirements

1. [ ] Sidebar displays at fixed 256px width on left edge
2. [ ] Close button closes sidebar (updates uiStore)
3. [ ] Header shows "PROJECTS" title with uppercase styling
4. [ ] Worktree status displays when active project uses worktree mode
5. [ ] Worktree status shows branch name and base branch
6. [ ] Worktree status is hidden for local-mode projects
7. [ ] Project list displays all projects sorted by updatedAt (newest first)
8. [ ] Clicking a project selects it as active (updates projectStore)
9. [ ] Active project has visual highlighting (elevated bg, accent icon)
10. [ ] Active project has left accent bar indicator
11. [ ] Project items show git mode badge (Local or Worktree)
12. [ ] Worktree projects show branch name in badge area
13. [ ] Long project names truncate with ellipsis
14. [ ] Long branch names truncate with ellipsis
15. [ ] Empty state displays when no projects exist
16. [ ] Empty state shows folder icon and helpful text
17. [ ] New Project button triggers onNewProject callback
18. [ ] New Project button spans full width of sidebar
19. [ ] Navigation items display only live AppView roots; no standalone planning/task entries
20. [ ] Clicking nav item updates current view (uiStore)
21. [ ] Active nav item has visual highlighting
22. [ ] Active nav item has left accent bar indicator
23. [ ] Keyboard shortcuts Cmd+1/2/3/4 switch views (if implemented)
24. [ ] Project list is scrollable when projects exceed viewport
25. [ ] Scrollbar is styled to match Mac aesthetic

---

## Design Quality Checklist

### Colors & Theming

1. [ ] NO purple or blue gradients anywhere
2. [ ] Sidebar background uses `--bg-surface` (#1a1a1a)
3. [ ] Active items use `--bg-elevated` (#242424)
4. [ ] Hover states use `--bg-hover` (#2d2d2d)
5. [ ] Git mode badges use `--bg-base` (#0f0f0f)
6. [ ] Accent color (`#ff6b35`) used only for active indicators and icons
7. [ ] Section title uses `--text-muted` (#666666)
8. [ ] Default text uses `--text-secondary` (#a0a0a0)
9. [ ] Active/hover text uses `--text-primary` (#f0f0f0)

### Typography

10. [ ] Header title: `text-xs`, `font-semibold`, `tracking-wide`, uppercase
11. [ ] Project names: `text-sm`, `font-medium`
12. [ ] Git mode badges: `text-xs`
13. [ ] Branch names: `text-xs`
14. [ ] Nav labels: `text-sm`, `font-medium`
15. [ ] Empty state title: `text-sm`, `font-medium`
16. [ ] Empty state subtitle: `text-xs`
17. [ ] All text uses SF Pro (system font stack)

### Spacing & Layout

18. [ ] Sidebar width: 256px (16rem)
19. [ ] Header padding: 16px horizontal, 12px vertical
20. [ ] Project list padding: 8px horizontal, 8px vertical
21. [ ] Project item padding: 12px horizontal, 8px vertical
22. [ ] Nav item height: 36px
23. [ ] Gap between nav items: 4px
24. [ ] Gap between project items: 4px (via flex gap)
25. [ ] Icon-to-text gap: 12px for items, 4px for badges
26. [ ] 8pt grid alignment maintained throughout

### Shadows & Depth

27. [ ] No shadows on sidebar container (border provides separation)
28. [ ] No shadows on list items (background change is sufficient)
29. [ ] Focus rings use `--shadow-glow`
30. [ ] Subtle depth through background color hierarchy

### Borders & Radius

31. [ ] Sidebar right border: 1px `--border-subtle`
32. [ ] Header bottom border: 1px `--border-subtle`
33. [ ] Button bottom border (above nav): 1px `--border-subtle`
34. [ ] Item border radius: `--radius-md` (8px)
35. [ ] Badge border radius: `--radius-sm` (4px)
36. [ ] Active indicator bar: 3px width, right-side rounded

### Motion & Interactions

37. [ ] Item hover: 150ms background transition
38. [ ] Item hover: subtle 2px rightward shift
39. [ ] Button hover: 150ms background transition
40. [ ] Button active: scale(0.98)
41. [ ] No jarring transitions
42. [ ] Scrollbar fades on inactivity (native behavior)

### Icons

43. [ ] All icons from Lucide library
44. [ ] Close button icon: 16px
45. [ ] Project folder icon: 16px
46. [ ] GitBranch icon (badges): 10px
47. [ ] GitBranch icon (status): 14px
48. [ ] Nav icons: 18px
49. [ ] Empty state icon: 24px
50. [ ] New Project plus icon: 16px
51. [ ] Icons use stroke-width 1.5-2px
52. [ ] Icons inherit appropriate colors

### Accessibility

53. [ ] Color contrast meets WCAG AA (4.5:1)
54. [ ] Focus states visible on all interactive elements
55. [ ] Sidebar has appropriate ARIA role (navigation or complementary)
56. [ ] Close button has aria-label
57. [ ] Nav items are keyboard navigable
58. [ ] Project list is keyboard navigable
59. [ ] Tab order follows visual order

---

## Implementation Notes

### shadcn Components to Use

- `Button` (New Project button)
- `ScrollArea` (project list scrollable container)
- `Separator` (divider between sections)
- `Tooltip` (keyboard shortcut hints on nav items)

### CSS Custom Properties

```css
/* Sidebar-specific */
--sidebar-width: 256px;
--sidebar-min-width: 240px;
--sidebar-max-width: 320px;

/* From DESIGN.md */
--bg-surface: #1a1a1a;
--bg-elevated: #242424;
--bg-hover: #2d2d2d;
--bg-base: #0f0f0f;
--text-primary: #f0f0f0;
--text-secondary: #a0a0a0;
--text-muted: #666666;
--accent-primary: #ff6b35;
--border-subtle: rgba(255, 255, 255, 0.06);
--radius-sm: 4px;
--radius-md: 8px;
--shadow-glow: 0 0 0 2px var(--bg-base), 0 0 0 4px var(--accent-primary);
```

### State Management

The sidebar interacts with two Zustand stores:

```tsx
// projectStore
const projects = useProjectStore((s) => s.projects);
const activeProjectId = useProjectStore((s) => s.activeProjectId);
const selectProject = useProjectStore((s) => s.selectProject);
const activeProject = useProjectStore(selectActiveProject);

// uiStore
const currentView = useUiStore((s) => s.currentView);
const setCurrentView = useUiStore((s) => s.setCurrentView);
const setSidebarOpen = useUiStore((s) => s.setSidebarOpen);
```

### Project Sorting

Projects are sorted by `updatedAt` descending (most recently updated first):

```tsx
const projectList = useMemo(() => {
  return Object.values(projects).sort((a, b) =>
    new Date(b.updatedAt).getTime() - new Date(a.updatedAt).getTime()
  );
}, [projects]);
```

---

## References

- [DESIGN.md](../DESIGN.md) - Master design system
- [specs/DESIGN_OVERHAUL_PLAN.md](../../DESIGN_OVERHAUL_PLAN.md) - Design overhaul strategy
- [header-navigation.md](./header-navigation.md) - Header navigation (when complete)
- Finder Sidebar - Native macOS sidebar patterns
- Linear Sidebar - Project switching, keyboard navigation
- Raycast - Mac-native aesthetic, spacing discipline
- shadcn/ui Button - https://ui.shadcn.com/docs/components/button
- shadcn/ui ScrollArea - https://ui.shadcn.com/docs/components/scroll-area
- Lucide icons - https://lucide.dev/icons/
