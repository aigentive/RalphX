/**
 * useAppKeyboardShortcuts - Keyboard shortcuts for view switching and shell actions
 */

import { useEffect, useRef } from "react";
import { register, unregister } from "@tauri-apps/plugin-global-shortcut";
import { ALL_NAV_ITEMS } from "@/components/layout/nav-items";
import type { AppView } from "@/types/app-view";
import type { FeatureFlags } from "@/types/feature-flags";

const ALL_ENABLED_FLAGS: FeatureFlags = {
  activityPage: true,
  extensibilityPage: true,
  automationsPage: true,
  atlassianOauth: false,
  ticketingDashboard: false,
};

interface UseAppKeyboardShortcutsProps {
  currentView: AppView;
  setCurrentView: (view: AppView) => void;
  toggleNotificationsPanel?: () => void;
  openProjectWizard?: () => void;
  hasProjects?: boolean;
  showWelcomeOverlay?: boolean;
  openWelcomeOverlay?: () => void;
  closeWelcomeOverlay?: () => void;
  welcomeOverlayReturnView?: AppView | null;
  openSettings?: () => void;
  openNewAgent?: () => void;
  featureFlags?: FeatureFlags;
}

export function useAppKeyboardShortcuts({
  currentView,
  setCurrentView,
  toggleNotificationsPanel,
  openProjectWizard,
  hasProjects,
  showWelcomeOverlay,
  openWelcomeOverlay,
  closeWelcomeOverlay,
  welcomeOverlayReturnView,
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
        const navigationItem = ALL_NAV_ITEMS.find(
          (item) => item.shortcut === `⌘${e.key}` && item.visible(featureFlags),
        );
        if (navigationItem) {
          e.preventDefault();
          setCurrentView(navigationItem.view);
          return;
        }

        switch (e.key) {
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
        }
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [setCurrentView, toggleNotificationsPanel, currentView, openProjectWizard, hasProjects, showWelcomeOverlay, openWelcomeOverlay, closeWelcomeOverlay, welcomeOverlayReturnView, openSettings, openNewAgent, featureFlags]);

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
