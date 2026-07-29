/**
 * Page chrome for one settings nav entry: header, leaf tab bar, and the
 * Integrations drill-in back affordance. Purely presentational — leaf
 * selection is routed back through the same `setActiveSection` the nav rail
 * uses, so persistence, warm-up, and deep-link behavior stay identical.
 *
 * The leaf bar is a hand-rolled segmented control rather than Radix `Tabs`
 * because the panel it switches is rendered outside this component (lazily,
 * by `SettingsSectionContent`), so there is no `TabsContent` to associate.
 */

import { ChevronLeft } from "lucide-react";

import {
  navForSection,
  sectionMeta,
  type SettingsSectionId,
} from "./settings-registry";

export interface SettingsNavPageProps {
  activeSection: SettingsSectionId;
  onSelectSection: (section: SettingsSectionId) => void;
  /** Preloads a leaf's lazy module on tab hover; deduped by the caller. */
  onWarmSection: (section: SettingsSectionId) => void;
  children: React.ReactNode;
}

export function SettingsNavPage({
  activeSection,
  onSelectSection,
  onWarmSection,
  children,
}: SettingsNavPageProps) {
  const nav = navForSection(activeSection);
  const hubLeaf = nav.hubLeaf;
  // Integrations replaces its tab bar with the hub grid; every other leaf of
  // that entry is a drill-in page reached from a hub card.
  const isDrillIn = hubLeaf !== undefined && hubLeaf !== activeSection;
  const showTabs = hubLeaf === undefined && nav.leaves.length > 1;

  return (
    <div className="settings-page">
      {/* Drill-in pages let the panel's own card header act as the page title,
          so they get the back affordance instead of a second heading. */}
      {isDrillIn && hubLeaf ? (
        <button
          type="button"
          data-testid="settings-drill-in-back"
          className="settings-page__back"
          onClick={() => onSelectSection(hubLeaf)}
        >
          <ChevronLeft aria-hidden="true" />
          <span>{nav.label}</span>
        </button>
      ) : (
        <div className="settings-page__head">
          <h1 className="settings-page__title" data-testid="settings-page-title">
            {nav.label}
          </h1>
          <p className="settings-page__sub">{nav.description}</p>
        </div>
      )}

      {showTabs ? (
        <div
          role="tablist"
          aria-label={`${nav.label} pages`}
          className="settings-tabs"
        >
          {nav.leaves.map((leafId) => {
            const isActive = leafId === activeSection;
            return (
              <button
                key={leafId}
                type="button"
                role="tab"
                aria-selected={isActive}
                tabIndex={isActive ? 0 : -1}
                data-testid={`settings-leaf-${leafId}`}
                data-state={isActive ? "active" : "inactive"}
                className="settings-tabs__trigger"
                onPointerEnter={() => onWarmSection(leafId)}
                onFocus={() => onWarmSection(leafId)}
                onClick={() => onSelectSection(leafId)}
              >
                {sectionMeta(leafId)?.label ?? leafId}
              </button>
            );
          })}
        </div>
      ) : null}

      <div className="settings-page__body">{children}</div>
    </div>
  );
}
