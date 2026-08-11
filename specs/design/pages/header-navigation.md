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
