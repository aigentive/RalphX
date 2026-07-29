/**
 * Left nav rail for the settings dialog.
 *
 * Renders the seven consolidated nav entries (icon + label + sublabel). The
 * per-leaf sections underneath each entry are selected by the in-page tab bar
 * in `SettingsNavPage`, so a nav click always lands on the entry's first leaf
 * — or on its hub leaf when it has one.
 */

import {
  SETTINGS_NAV,
  navForSection,
  type SettingsNavMeta,
  type SettingsSectionId,
} from "./settings-registry";

export interface SettingsNavRailProps {
  activeSection: SettingsSectionId;
  onSelect: (section: SettingsSectionId) => void;
  /** Preloads a leaf's lazy module; deduped by the caller. */
  onWarm: (section: SettingsSectionId) => void;
}

/** Leaf a nav click lands on: the hub when present, otherwise the first leaf. */
function navEntrySection(nav: SettingsNavMeta): SettingsSectionId {
  return nav.hubLeaf ?? (nav.leaves[0] as SettingsSectionId);
}

export function SettingsNavRail({
  activeSection,
  onSelect,
  onWarm,
}: SettingsNavRailProps) {
  const activeNavId = navForSection(activeSection).id;

  return (
    <nav
      aria-label="Settings sections"
      className="settings-nav hidden lg:flex flex-shrink-0 flex-col overflow-y-auto"
    >
      {SETTINGS_NAV.map((nav) => {
        const isActive = nav.id === activeNavId;
        const target = navEntrySection(nav);
        const Icon = nav.icon;
        // Warm every leaf behind the entry so the tab bar is instant too.
        const warmEntry = () => nav.leaves.forEach(onWarm);
        return (
          <div
            key={nav.id}
            role="button"
            tabIndex={0}
            data-section={target}
            data-nav={nav.id}
            data-testid={`settings-nav-${nav.id}`}
            aria-label={nav.label}
            aria-current={isActive ? "page" : undefined}
            onPointerEnter={warmEntry}
            onFocus={warmEntry}
            onClick={() => onSelect(target)}
            onKeyDown={(e) => {
              if (e.key === "Enter" || e.key === " ") {
                e.preventDefault();
                onSelect(target);
              }
            }}
            className="settings-nav__item"
          >
            <Icon className="settings-nav__icon" aria-hidden="true" />
            <span className="settings-nav__text">
              <span className="settings-nav__name">{nav.label}</span>
              <span className="settings-nav__sub">{nav.sublabel}</span>
            </span>
          </div>
        );
      })}
    </nav>
  );
}
