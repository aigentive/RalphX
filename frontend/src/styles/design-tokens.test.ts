/**
 * Tests for design system CSS variables
 *
 * Verifies that all required design tokens are defined
 * in the global styles.
 */

import { describe, it, expect, beforeAll } from "vitest";
import fs from "fs";
import path from "path";

describe("design-tokens", () => {
  let cssContent: string;

  beforeAll(() => {
    // 3-tier token architecture — concatenate all token sources so assertions
    // don't depend on which file a given token currently lives in.
    // See specs/design/styleguide.md.
    const files = [
      "./globals.css",
      "./tokens/primitives.css",
      "./tokens/semantic.css",
      "./tokens/components.css",
      "./themes/light.css",
      "./themes/dark.css",
      "./themes/high-contrast.css",
    ];
    cssContent = files
      .map((f) => fs.readFileSync(path.resolve(__dirname, f), "utf-8"))
      .join("\n");
  });

  describe("color palette", () => {
    it("should define background colors", () => {
      expect(cssContent).toContain("--bg-base:");
      expect(cssContent).toContain("--bg-surface:");
      expect(cssContent).toContain("--bg-elevated:");
      expect(cssContent).toContain("--bg-hover:");
    });

    it("should define text colors", () => {
      expect(cssContent).toContain("--text-primary:");
      expect(cssContent).toContain("--text-secondary:");
      expect(cssContent).toContain("--text-muted:");
    });

    it("should define accent colors (warm, NOT purple)", () => {
      expect(cssContent).toContain("--accent-primary:");
      expect(cssContent).toContain("--accent-secondary:");
      expect(cssContent).toContain("--accent-muted-strong:");
      // v27 flattens the brand accent into literal values so WKWebView does
      // not drop chained custom properties on inherited chrome.
      expect(cssContent).toMatch(/--accent-primary:\s*#FF6A35/);
      expect(cssContent).toMatch(/--accent-muted-strong:\s*rgba\(255,106,53,.18\)/);
    });

    it("should define status colors", () => {
      expect(cssContent).toContain("--status-success:");
      expect(cssContent).toContain("--status-warning:");
      expect(cssContent).toContain("--status-error:");
      expect(cssContent).toContain("--status-info:");
    });

    it("should define border colors", () => {
      expect(cssContent).toContain("--border-subtle:");
      expect(cssContent).toContain("--border-default:");
    });

    it("should define v27 theme-aware brand mark tokens", () => {
      expect(cssContent).toMatch(/--brand-tile:\s*#232329/);
      expect(cssContent).toMatch(/--brand-tile:\s*#DEDEE2/);
      expect(cssContent).toMatch(/--brand-tile:\s*#1A1A1A/);
      expect(cssContent).toMatch(/--brand-x:\s*#FA4F19/);
      expect(cssContent).toContain("--nav-rail-active-color:");
      expect(cssContent).toContain("--nav-rail-inactive-color:");
    });

    it("should mirror v27 light chrome surface tokens", () => {
      expect(cssContent).toMatch(/--bg-base:\s*#FFFFFF/);
      expect(cssContent).toMatch(/--bg-surface:\s*#F4F4F6/);
      expect(cssContent).toMatch(/--bg-elevated:\s*#FFFFFF/);
      expect(cssContent).toMatch(/--bg-hover:\s*#F1F1F4/);
      expect(cssContent).toMatch(/--topbar-bg:\s*#F4F4F6/);
      expect(cssContent).toMatch(/--nav-rail-bg:\s*#F4F4F6/);
      expect(cssContent).toMatch(/--app-navbar-bg:\s*#F4F4F6/);
      expect(cssContent).toMatch(/--app-rail-bg:\s*#F4F4F6/);
      expect(cssContent).toMatch(/--app-sidebar-bg:\s*#F8F8FA/);
      expect(cssContent).toMatch(/--app-content-bg:\s*#FFFFFF/);
      expect(cssContent).toMatch(/--border-subtle:\s*#E5E5E8/);
      expect(cssContent).toMatch(/--border-default:\s*#D9D9DD/);
      expect(cssContent).toMatch(/--border-strong:\s*#C8C8CD/);
      expect(cssContent).toMatch(/--app-navbar-border:\s*#E5E5E8/);
      expect(cssContent).toMatch(/--app-rail-border:\s*#E5E5E8/);
      expect(cssContent).toMatch(/--app-sidebar-border:\s*#E5E5E8/);
      expect(cssContent).toMatch(/--app-content-border:\s*#E5E5E8/);
      expect(cssContent).toMatch(/--nav-rail-inactive-color:\s*#6A6A72/);
      expect(cssContent).toMatch(/--text-primary:\s*#18181D/);
      expect(cssContent).toMatch(/--text-secondary:\s*#404048/);
      expect(cssContent).toMatch(/--text-muted:\s*#6A6A72/);
    });

    it("should pin v27 app chrome surfaces with literal theme selectors for WKWebView", () => {
      expect(cssContent).toMatch(
        /\[data-theme="light"\]\s+\[data-testid="app-header"\]\s*\{[^}]*background-color:\s*#F4F4F6\s*!important;[^}]*border-bottom-color:\s*#E5E5E8\s*!important;/s
      );
      expect(cssContent).toMatch(
        /\[data-theme="light"\]\s+\[data-testid="left-nav-rail"\]\s*\{[^}]*background-color:\s*#F4F4F6\s*!important;[^}]*border-right-color:\s*#E5E5E8\s*!important;/s
      );
      expect(cssContent).toMatch(
        /\[data-theme="light"\]\s+\.left-nav-rail__active-border\s*\{[^}]*background-color:\s*#FF6A35\s*!important;/s
      );
      expect(cssContent).toMatch(
        /\[data-theme="light"\]\s+\[data-testid="agents-sidebar"\]\s*\{[^}]*background-color:\s*#F8F8FA\s*!important;[^}]*border-right-color:\s*#E5E5E8\s*!important;/s
      );
      expect(cssContent).toMatch(
        /\[data-theme="light"\]\s+\.agents-project-row\[aria-current="true"\]\s*\{[^}]*background-color:\s*rgba\(255,106,53,\.14\)\s*!important;[^}]*background-image:\s*linear-gradient\(180deg,\s*rgba\(255,106,53,\.18\),\s*rgba\(255,106,53,\.10\)\)\s*!important;/s
      );
      expect(cssContent).toMatch(
        /\[data-theme="light"\]\s+\.agents-session-row\[aria-current="true"\]\s*\{[^}]*background-color:\s*rgba\(255,106,53,\.10\)\s*!important;/s
      );
      expect(cssContent).toMatch(
        /\[data-theme="light"\]\s+\.agents-project-row\s*\{[^}]*color:\s*#6A6A72\s*!important;/s
      );
      expect(cssContent).toMatch(
        /\[data-theme="light"\]\s+\.agents-project-row\[aria-current="true"\]\s*\{[^}]*color:\s*#18181D\s*!important;/s
      );
      expect(cssContent).toMatch(
        /\[data-theme="light"\]\s+\.agents-session-meta\s*\{[^}]*color:\s*#6A6A72\s*!important;/s
      );
      expect(cssContent).toMatch(
        /\[data-theme="light"\]\s+\[data-testid="reviews-panel-shell"\]\s*\{[^}]*background-color:\s*#F4F4F6\s*!important;[^}]*border-left-color:\s*#E5E5E8\s*!important;/s
      );
      expect(cssContent).toMatch(
        /\[data-theme="light"\]\s+\[data-testid="kanban-split-layout"\][^{]*\{[^}]*background-color:\s*#FFFFFF\s*!important;/s
      );
    });

    it("should pin light navbar control surfaces to white for WKWebView", () => {
      expect(cssContent).toMatch(
        /\[data-theme="light"\]\s+\[data-testid="topbar-command-search"\],[^{]*\[data-testid="theme-selector-trigger"\],[^{]*\[data-testid="theme-selector-menu"\],[^{]*\[data-testid="font-scale-selector-trigger"\],[^{]*\[data-testid="font-scale-selector-menu"\]\s*\{[^}]*background-color:\s*#FFFFFF\s*!important;[^}]*border-color:\s*#D9D9DD\s*!important;[^}]*color:\s*#6A6A72\s*!important;/s
      );
      expect(cssContent).toMatch(
        /\[data-theme="light"\]\s+\[data-testid="topbar-command-search"\]\s+kbd,[^{]*\[data-testid="theme-selector-trigger"\]\s+svg,[^{]*\[data-testid="font-scale-selector-trigger"\]\s+svg\s*\{[^}]*color:\s*#6A6A72\s*!important;/s
      );
      expect(cssContent).toMatch(
        /\[data-theme="light"\]\s+\[data-testid\^="theme-option-"\],[^{]*\[data-testid\^="font-scale-option-"\]\s*\{[^}]*color:\s*#404048\s*!important;/s
      );
      expect(cssContent).toMatch(
        /\[data-theme="light"\]\s+\[data-testid\^="theme-option-"\]\[aria-checked="true"\],[^{]*\[data-testid\^="font-scale-option-"\]\[aria-checked="true"\]\s*\{[^}]*color:\s*#18181D\s*!important;/s
      );
    });

    it("should pin light settings shell panes with literal v30 colors for WKWebView", () => {
      expect(cssContent).toMatch(
        /\[data-theme="light"\]\s+\[data-testid="settings-dialog"\]\s+\.settings-nav\s*\{[^}]*background-color:\s*#F8F8FA\s*!important;/s
      );
      expect(cssContent).toMatch(
        /\[data-theme="light"\]\s+\[data-testid="settings-dialog"\]\s+\.settings-pane\s*\{[^}]*background-color:\s*#FAFAFB\s*!important;/s
      );
      expect(cssContent).toMatch(
        /\[data-theme="light"\]\s+\[data-testid="settings-dialog"\]\s+\.settings-modal__crumbs\s+\.cur,[^{]*\.settings-pane-head__title,[^{]*\.settings-row__label,[^{]*\.settings-diag-card__title,[^{]*\.settings-nav__item\[aria-current="true"\]\s*\{[^}]*color:\s*#18181D\s*!important;/s
      );
      expect(cssContent).toMatch(
        /\[data-theme="light"\]\s+\[data-testid="settings-dialog"\]\s+\.settings-nav__item,[^{]*\.settings-readonly-value,[^{]*\.settings-diag-card__body,[^{]*\.settings-btn-ghost\s*\{[^}]*color:\s*#404048\s*!important;/s
      );
      expect(cssContent).toMatch(/--notice-info-bg:\s*#FFFFFF/);
      expect(cssContent).toMatch(/--notice-info-border:\s*#D9D9DD/);
      expect(cssContent).toMatch(/--notice-info-text:\s*#6A6A72/);
      expect(cssContent).toMatch(/--notice-ok-bg:\s*#FFFFFF/);
      expect(cssContent).toMatch(/--notice-ok-border:\s*#D9D9DD/);
    });

    it("should override Sonner light toasts with v27 light ink and action colors", () => {
      expect(cssContent).toMatch(
        /\[data-theme="light"\]\s+\[data-sonner-toaster\]\s*\{[^}]*--normal-bg:\s*#FFFFFF;[^}]*--normal-border:\s*#D9D9DD;[^}]*--normal-text:\s*#18181D;/s
      );
      expect(cssContent).toMatch(
        /\[data-theme="light"\]\s+\[data-sonner-toast\]\[data-styled="true"\]\s*\{[^}]*background-color:\s*#FFFFFF\s*!important;[^}]*border-color:\s*#D9D9DD\s*!important;[^}]*color:\s*#18181D\s*!important;/s
      );
      expect(cssContent).toMatch(
        /\[data-theme="light"\]\s+\[data-sonner-toast\]\[data-styled="true"\]\s+\[data-description\]\s*\{[^}]*color:\s*#404048\s*!important;/s
      );
      expect(cssContent).toMatch(
        /\[data-theme="light"\]\s+\[data-sonner-toast\]\[data-styled="true"\]\s+\[data-button\]\s*\{[^}]*background-color:\s*#FF6A35\s*!important;[^}]*border:\s*1px\s+solid\s+#E0521E\s*!important;[^}]*color:\s*#1A0E07\s*!important;/s
      );
    });

    it("should define literal tooltip chrome tokens across themes", () => {
      expect(cssContent).toMatch(/--tooltip-bg:\s*#232329/);
      expect(cssContent).toMatch(/--tooltip-border:\s*#393940/);
      expect(cssContent).toMatch(/--tooltip-text:\s*#F2F2F4/);
      expect(cssContent).toMatch(/--tooltip-bg:\s*#FFFFFF/);
      expect(cssContent).toMatch(/--tooltip-border:\s*#D9D9DD/);
      expect(cssContent).toMatch(/--tooltip-text:\s*#18181D/);
      expect(cssContent).toMatch(/--tooltip-bg:\s*#000000/);
      expect(cssContent).toMatch(/--tooltip-border:\s*#FFFFFF/);
      expect(cssContent).toMatch(/--tooltip-text:\s*#FFFFFF/);
    });

    it("should define subdued dark settings notice tokens", () => {
      expect(cssContent).toMatch(/--notice-title-text:\s*#C7C7CC/);
      expect(cssContent).toMatch(/--notice-info-bg:\s*rgba\(255,255,255,\.025\)/);
      expect(cssContent).toMatch(/--notice-info-border:\s*#393940/);
      expect(cssContent).toMatch(/--notice-info-text:\s*#A9A9B0/);
      expect(cssContent).toMatch(/--notice-ok-bg:\s*rgba\(63,191,127,\.055\)/);
      expect(cssContent).toMatch(/--notice-ok-border:\s*rgba\(63,191,127,\.22\)/);
    });

    it("should mirror v27 dark and high-contrast app chrome tokens", () => {
      expect(cssContent).toMatch(/--app-navbar-bg:\s*#1E1E23/);
      expect(cssContent).toMatch(/--app-rail-bg:\s*#1B1B20/);
      expect(cssContent).toMatch(/--app-sidebar-bg:\s*#1E1E23/);
      expect(cssContent).toMatch(/--app-content-bg:\s*#18181D/);
      expect(cssContent).toMatch(/--app-navbar-border:\s*#2E2E36/);
      expect(cssContent).toMatch(/--app-rail-border:\s*#2E2E36/);
      expect(cssContent).toMatch(/--app-sidebar-border:\s*#2E2E36/);
      expect(cssContent).toMatch(/--app-content-border:\s*#2E2E36/);
      expect(cssContent).toMatch(/--app-navbar-bg:\s*#0A0A0A/);
      expect(cssContent).toMatch(/--app-rail-bg:\s*#0A0A0A/);
      expect(cssContent).toMatch(/--app-sidebar-bg:\s*#0A0A0A/);
      expect(cssContent).toMatch(/--app-content-bg:\s*#000000/);
      expect(cssContent).toMatch(/--app-navbar-border:\s*#555555/);
      expect(cssContent).toMatch(/--app-rail-border:\s*#555555/);
      expect(cssContent).toMatch(/--app-sidebar-border:\s*#555555/);
      expect(cssContent).toMatch(/--app-content-border:\s*#555555/);
    });

    it("should define v29a Kanban component tokens across themes", () => {
      expect(cssContent).toMatch(/--kanban-card-bg:\s*#232329/);
      expect(cssContent).toMatch(/--kanban-card-bg:\s*#FFFFFF/);
      expect(cssContent).toMatch(/--kanban-card-bg:\s*#1A1A1A/);
      expect(cssContent).toMatch(/--kanban-card-border:\s*#34343C/);
      expect(cssContent).toMatch(/--kanban-card-border:\s*#E0E0E4/);
      expect(cssContent).toMatch(/--kanban-card-border:\s*#777777/);
      expect(cssContent).toMatch(/--kanban-board-divider:\s*#2E2E36/);
      expect(cssContent).toMatch(/--kanban-column-bg:\s*#18181D/);
      expect(cssContent).toMatch(/--kanban-tray-bg:\s*#2A2A31/);
      expect(cssContent).toMatch(/--kanban-tray-bg:\s*#EBEBED/);
      expect(cssContent).toMatch(/--kanban-tray-bg:\s*#1A1A1A/);
      expect(cssContent).toMatch(/--kanban-empty-ink:\s*#6A6A72/);
      expect(cssContent).toMatch(/--kanban-empty-ink:\s*#93939B/);
      expect(cssContent).toMatch(/--kanban-empty-ink:\s*#999999/);
      expect(cssContent).toMatch(/--kanban-column-bg:\s*rgba\(255,255,255,\.02\)/);
      expect(cssContent).toMatch(/--kanban-card-warning-bg:\s*rgba\(224,179,65,\.10\)/);
      expect(cssContent).toMatch(/--kanban-card-warning-border:\s*rgba\(224,179,65,\.30\)/);
      expect(cssContent).toMatch(/--kanban-card-success-bg:\s*rgba\(63,191,127,\.08\)/);
      expect(cssContent).toMatch(/--kanban-card-success-border:\s*rgba\(63,191,127,\.22\)/);
      expect(cssContent).toMatch(/--kanban-card-selected-border:\s*rgba\(111,179,255,\.24\)/);
      expect(cssContent).toMatch(/--kanban-progress-track:\s*rgba\(255,255,255,\.08\)/);
      expect(cssContent).toMatch(/--kanban-progress-track:\s*rgba\(255,255,255,\.06\)/);
      expect(cssContent).toContain("--kanban-card-warning-bg:");
      expect(cssContent).toContain("--kanban-card-success-bg:");
    });

    it("should pin dark Kanban column and empty-state chrome with literal v29a selectors", () => {
      expect(cssContent).toMatch(
        /\[data-theme="dark"\]\s+\[data-testid="task-board"\],[^{]*\[data-testid="task-board-skeleton"\]\s*\{[^}]*background-color:\s*#2E2E36\s*!important;/s
      );
      expect(cssContent).toMatch(
        /\[data-theme="dark"\]\s+\[data-testid\^="column-"\]:not\(\[data-testid="column-header"\]\),[^{]*\[data-testid\^="skeleton-column-"\]\s*\{[^}]*background-color:\s*#18181D\s*!important;/s
      );
      expect(cssContent).toMatch(
        /\[data-theme="dark"\]\s+\[data-testid="empty-state-tray"\],[^{]*\[data-testid="collapsed-empty-state-tray"\]\s*\{[^}]*background-color:\s*#2A2A31\s*!important;[^}]*color:\s*#6A6A72\s*!important;/s
      );
      expect(cssContent).toMatch(
        /\[data-theme="dark"\]\s+\[data-testid="empty-state-label"\],[^{]*\[data-testid="collapsed-empty-state-label"\]\s*\{[^}]*color:\s*#6A6A72\s*!important;/s
      );
    });
  });

  describe("typography", () => {
    it("should define font families (NOT Inter)", () => {
      expect(cssContent).toContain("--font-display:");
      expect(cssContent).toContain("--font-body:");
      expect(cssContent).toContain("--font-mono:");
      // Verify NOT using Inter
      expect(cssContent).not.toMatch(/--font-display:.*Inter/);
      expect(cssContent).not.toMatch(/--font-body:.*Inter/);
    });

    it("should define font sizes", () => {
      expect(cssContent).toContain("--text-xs:");
      expect(cssContent).toContain("--text-sm:");
      expect(cssContent).toContain("--text-base:");
      expect(cssContent).toContain("--text-lg:");
      expect(cssContent).toContain("--text-xl:");
    });
  });

  describe("spacing (8pt grid)", () => {
    it("should define spacing scale", () => {
      // --space-* primitives are the direct-CSS scale (1-8). Tailwind's wider
      // 1-16 scale resolves via --spacing-* in the @theme inline block.
      expect(cssContent).toContain("--space-1:");
      expect(cssContent).toContain("--space-2:");
      expect(cssContent).toContain("--space-3:");
      expect(cssContent).toContain("--space-4:");
      expect(cssContent).toContain("--space-6:");
      expect(cssContent).toContain("--space-8:");
      // Tailwind-scale entries available as --spacing-* for utility classes
      expect(cssContent).toContain("--spacing-12:");
    });

    it("should use 8pt grid values", () => {
      // space-1 = 4px, space-2 = 8px, etc.
      expect(cssContent).toMatch(/--space-1:\s*4px/);
      expect(cssContent).toMatch(/--space-2:\s*8px/);
      expect(cssContent).toMatch(/--space-4:\s*16px/);
      expect(cssContent).toMatch(/--space-8:\s*32px/);
    });
  });

  describe("other tokens", () => {
    it("should define border radius", () => {
      expect(cssContent).toContain("--radius-sm:");
      expect(cssContent).toContain("--radius-md:");
      expect(cssContent).toContain("--radius-lg:");
    });

    it("should define shadows", () => {
      expect(cssContent).toContain("--shadow-sm:");
      expect(cssContent).toContain("--shadow-md:");
      expect(cssContent).toContain("--shadow-lg:");
    });

    it("should define transitions", () => {
      expect(cssContent).toContain("--transition-fast:");
      expect(cssContent).toContain("--transition-normal:");
    });

    it("should keep settings card icon tiles legible in high contrast", () => {
      // HC icon tile pattern: transparent fill + accent outline + white glyph.
      // Avoids the accent-on-accent collision that happens when tinted bg
      // meets accent-colored glyphs. See themes/high-contrast.md §3.
      expect(cssContent).toMatch(/--card-icon-bg:\s*transparent/);
      expect(cssContent).toMatch(/--card-icon-border:\s*var\(--accent-primary\)/);
      expect(cssContent).toMatch(/--card-icon-color:\s*var\(--color-white\)/);
    });
  });

  describe("anti-AI-slop guardrails", () => {
    it("should NOT use purple gradients", () => {
      // Check no purple hex codes in accents
      expect(cssContent).not.toMatch(/--accent.*#[0-9a-f]*[8-9a-f][0-9a-f][0-9a-f][0-9a-f]ff/i);
    });

    it("should use v27 dark canvas, NOT pure black", () => {
      expect(cssContent).toMatch(/--bg-base:\s*#18181D/);
    });

    it("should use off-white, NOT pure white", () => {
      expect(cssContent).toMatch(/--text-primary:\s*#F2F2F4/);
    });
  });
});
