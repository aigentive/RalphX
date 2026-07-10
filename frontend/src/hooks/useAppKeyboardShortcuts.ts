/**
 * useAppKeyboardShortcuts - Keyboard shortcuts for view switching and shell actions
 */

import { useEffect, useRef } from "react";
import { register, unregister } from "@tauri-apps/plugin-global-shortcut";
import type { ViewType } from "@/types/chat";
import type { FeatureFlags } from "@/types/feature-flags";

const ALL_ENABLED_FLAGS: FeatureFlags = {
  activityPage: true,
  extensibilityPage: true,
  ideationPage: true,
  automationsPage: true,
  battleMode: true,
  teamMode: false,
  atlassianOauth: false,
  ticketingDashboard: false,
};

interface UseAppKeyboardShortcutsProps {
  currentView: ViewType;
  setCurrentView: (view: ViewType) => void;
  toggleNotificationsPanel?: () => void;
  toggleGraphRightPanel?: () => void;
  openProjectWizard?: () => void;
  hasProjects?: boolean;
  showWelcomeOverlay?: boolean;
  openWelcomeOverlay?: () => void;
  closeWelcomeOverlay?: () => void;
  welcomeOverlayReturnView?: ViewType | null;
  openPlanQuickSwitcher?: () => void;
  onBattleModeToggle?: () => void;
  openSettings?: () => void;
  openNewAgent?: () => void;
  featureFlags?: FeatureFlags;
}

export function useAppKeyboardShortcuts({
  currentView,
  setCurrentView,
  toggleNotificationsPanel,
  toggleGraphRightPanel,
  openProjectWizard,
  hasProjects,
  showWelcomeOverlay,
  openWelcomeOverlay,
  closeWelcomeOverlay,
  welcomeOverlayReturnView,
  openPlanQuickSwitcher,
  onBattleModeToggle,
  openSettings,
  openNewAgent,
  featureFlags = ALL_ENABLED_FLAGS,
}: UseAppKeyboardShortcutsProps) {
  // Keyboard shortcuts for view switching and shell actions
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      // Escape to close welcome overlay (no modifier required)
      if (e.key === "Escape" && showWelcomeOverlay && closeWelcomeOverlay) {
        e.preventDefault();
        if (welcomeOverlayReturnView) {
          setCurrentView(welcomeOverlayReturnView);
        }
        closeWelcomeOverlay();
        return;
      }

      if (e.metaKey || e.ctrlKey) {
        // Main navigation order: Agents → Ideation → Graph → Kanban → Insights
        switch (e.key) {
          case "1":
            e.preventDefault();
            setCurrentView("agents");
            break;
          case "2":
            e.preventDefault();
            if (featureFlags.ideationPage) {
              setCurrentView("ideation");
            }
            break;
          case "3":
            e.preventDefault();
            setCurrentView("graph");
            break;
          case "4":
            e.preventDefault();
            setCurrentView("kanban");
            break;
          case "5":
            e.preventDefault();
            setCurrentView("insights");
            break;
          case "6":
          case ".":
          case ",":
            // Cmd+6, Cmd+. or Cmd+, for settings (Cmd+, may not work in dev mode)
            e.preventDefault();
            openSettings?.();
            break;
          case "n":
          case "N": {
            // Cmd+Shift+N: Always open project wizard (global)
            // Cmd+N: Open new agent in Agents view, otherwise project wizard only on welcome screen
            if (!openProjectWizard) {
              return;
            }
            const activeEl = document.activeElement;
            if (
              activeEl instanceof HTMLInputElement ||
              activeEl instanceof HTMLTextAreaElement
            ) {
              return;
            }
            if (e.shiftKey) {
              // Cmd+Shift+N: Always available
              e.preventDefault();
              openProjectWizard();
            } else if (!hasProjects) {
              // Cmd+N: Only on welcome screen (no projects)
              e.preventDefault();
              openProjectWizard();
            } else if (currentView === "agents" && openNewAgent) {
              e.preventDefault();
              openNewAgent();
            }
            break;
          }
          case "a":
          case "A": {
            if (!e.shiftKey) {
              return;
            }
            const activeEl = document.activeElement;
            if (
              activeEl instanceof HTMLInputElement ||
              activeEl instanceof HTMLTextAreaElement
            ) {
              return;
            }
            e.preventDefault();
            setCurrentView("agents");
            break;
          }
          case "w":
          case "W": {
            // Cmd+Shift+W: Toggle welcome screen overlay
            if (!e.shiftKey || !openWelcomeOverlay || !hasProjects) {
              return;
            }
            const activeEl = document.activeElement;
            if (
              activeEl instanceof HTMLInputElement ||
              activeEl instanceof HTMLTextAreaElement
            ) {
              return;
            }
            e.preventDefault();
            if (showWelcomeOverlay && closeWelcomeOverlay) {
              // Already showing - close it
              if (welcomeOverlayReturnView) {
                setCurrentView(welcomeOverlayReturnView);
              }
              closeWelcomeOverlay();
            } else {
              // Open welcome overlay
              openWelcomeOverlay();
            }
            break;
          }
          case "r":
          case "R": {
            // Cmd+Shift+R: Toggle reviews panel
            if (!e.shiftKey || !toggleNotificationsPanel) {
              return;
            }
            const activeEl = document.activeElement;
            if (
              activeEl instanceof HTMLInputElement ||
              activeEl instanceof HTMLTextAreaElement
            ) {
              return;
            }
            e.preventDefault();
            toggleNotificationsPanel();
            break;
          }
          case "l":
          case "L": {
            if (!toggleGraphRightPanel || currentView !== "graph") {
              return;
            }
            const activeEl = document.activeElement;
            if (
              activeEl instanceof HTMLInputElement ||
              activeEl instanceof HTMLTextAreaElement
            ) {
              return;
            }
            e.preventDefault();
            toggleGraphRightPanel();
            break;
          }
          case "p":
          case "P": {
            // Cmd+Shift+P: Open plan quick switcher
            if (!e.shiftKey || !openPlanQuickSwitcher) {
              return;
            }
            const activeEl = document.activeElement;
            if (
              activeEl instanceof HTMLInputElement ||
              activeEl instanceof HTMLTextAreaElement
            ) {
              return;
            }
            e.preventDefault();
            openPlanQuickSwitcher();
            break;
          }
          case "b":
          case "B": {
            // Cmd+Shift+B: Toggle Battle Mode (graph view only, feature flag gated)
            if (!e.shiftKey || !onBattleModeToggle) {
              return;
            }
            if (currentView !== "graph" || !featureFlags.battleMode) {
              return;
            }
            const activeEl = document.activeElement;
            if (
              activeEl instanceof HTMLInputElement ||
              activeEl instanceof HTMLTextAreaElement
            ) {
              return;
            }
            e.preventDefault();
            onBattleModeToggle();
            break;
          }
        }
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [setCurrentView, toggleNotificationsPanel, toggleGraphRightPanel, currentView, openProjectWizard, hasProjects, showWelcomeOverlay, openWelcomeOverlay, closeWelcomeOverlay, welcomeOverlayReturnView, openPlanQuickSwitcher, onBattleModeToggle, openSettings, openNewAgent, featureFlags]);

  // Global shortcut for Cmd+, (registered at OS level to bypass DevTools interception)
  const setCurrentViewRef = useRef(setCurrentView);
  const openSettingsRef = useRef(openSettings);

  useEffect(() => {
    setCurrentViewRef.current = setCurrentView;
  }, [setCurrentView]);

  useEffect(() => {
    openSettingsRef.current = openSettings;
  }, [openSettings]);

  useEffect(() => {
    const shortcut = "CommandOrControl+,";

    register(shortcut, () => {
      openSettingsRef.current?.();
    }).catch(() => {
      // Ignore registration errors
    });

    return () => {
      unregister(shortcut).catch(() => {
        // Ignore unregister errors on cleanup
      });
    };
  }, []);

}
