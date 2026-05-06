import {
  useCallback,
  useEffect,
  useRef,
  useState,
} from "react";
import { X } from "lucide-react";

import { ScrollArea } from "@/components/ui/scroll-area";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogTitle,
} from "@/components/ui/dialog";
import { useUiStore } from "@/stores/uiStore";
import type { ProjectSettings } from "@/types/settings";

import {
  DEFAULT_SETTINGS_SECTION,
  SETTINGS_GROUPS,
  SETTINGS_SECTIONS,
  isSettingsSectionId,
  type SettingsSectionId,
} from "./settings-registry";
import { loadActiveSection, saveActiveSection } from "./settings-ui-state";
import {
  cancelScheduledJob,
  scheduleAfterPaint,
  sectionModuleLoaders,
  useDeferredDialogFrame,
  useDeferredHydratedSection,
  type ScheduledJob,
} from "./SettingsDialog.performance";
import { SettingsSectionContent } from "./SettingsSectionContent";

export interface SettingsDialogProps {
  executionSettings: ProjectSettings | null;
  isLoadingSettings: boolean;
  isSavingSettings: boolean;
  settingsError: string | null;
  onSettingsChange: (settings: ProjectSettings) => void;
}

export default function SettingsDialog({
  executionSettings,
  isLoadingSettings,
  isSavingSettings,
  settingsError,
  onSettingsChange,
}: SettingsDialogProps) {
  const activeModal = useUiStore((s) => s.activeModal);
  const modalContext = useUiStore((s) => s.modalContext);
  const closeModal = useUiStore((s) => s.closeModal);

  const isOpen = activeModal === "settings";

  const [activeSection, setActiveSectionState] = useState<SettingsSectionId>(
    () => loadActiveSection() ?? DEFAULT_SETTINGS_SECTION,
  );
  const shouldRenderFrame = useDeferredDialogFrame(isOpen);
  const isSectionHydrated = useDeferredHydratedSection(isOpen, activeSection);
  const persistJobRef = useRef<ScheduledJob | null>(null);
  const closeJobRef = useRef<ScheduledJob | null>(null);
  const warmedSectionsRef = useRef<Partial<Record<SettingsSectionId, true>>>({});
  const [isClosing, setIsClosing] = useState(false);

  const persistActiveSection = useCallback((section: SettingsSectionId) => {
    cancelScheduledJob(persistJobRef.current);
    persistJobRef.current = scheduleAfterPaint(() => {
      persistJobRef.current = null;
      saveActiveSection(section);
    });
  }, []);

  const setActiveSection = useCallback(
    (section: SettingsSectionId) => {
      setActiveSectionState(section);
      persistActiveSection(section);
    },
    [persistActiveSection],
  );

  const warmSection = useCallback((section: SettingsSectionId) => {
    if (warmedSectionsRef.current[section]) {
      return;
    }
    warmedSectionsRef.current[section] = true;
    void sectionModuleLoaders[section]();
  }, []);

  const requestClose = useCallback(() => {
    if (closeJobRef.current) {
      return;
    }
    setIsClosing(true);
    closeJobRef.current = scheduleAfterPaint(() => {
      closeJobRef.current = null;
      closeModal();
    });
  }, [closeModal]);

  useEffect(
    () => () => {
      cancelScheduledJob(persistJobRef.current);
      cancelScheduledJob(closeJobRef.current);
    },
    [],
  );

  useEffect(() => {
    if (!isOpen) {
      cancelScheduledJob(closeJobRef.current);
      closeJobRef.current = null;
    }
    setIsClosing(false);
  }, [isOpen]);

  useEffect(() => {
    if (isOpen) {
      const section = modalContext?.["section"];
      if (isSettingsSectionId(section)) {
        setActiveSection(section);
      }
    }
  }, [isOpen, modalContext, setActiveSection]);

  const activeSectionMeta = SETTINGS_SECTIONS.find((s) => s.id === activeSection);

  const disabled = isLoadingSettings || isSavingSettings;

  return (
    <Dialog open={isOpen} onOpenChange={(open) => !open && requestClose()}>
      {shouldRenderFrame && (
        <DialogContent
          forceMount
          data-testid="settings-dialog"
          overlayClassName="settings-layer__scrim"
          className={`settings-modal p-0 gap-0 overflow-hidden flex flex-col max-w-[95vw] w-[95vw] h-[95vh] bg-[var(--dialog-bg)] border border-[var(--dialog-border-color)] duration-0 data-[state=open]:animate-none data-[state=closed]:animate-none ${
            isClosing ? "pointer-events-none opacity-0 scale-[0.98]" : ""
          }`}
          style={{
            backgroundColor: "var(--dialog-bg)",
            borderColor: "var(--dialog-border-color)",
            boxShadow: "var(--dialog-shadow)",
          }}
          hideCloseButton={true}
        >
        <DialogTitle className="sr-only">Settings</DialogTitle>
        <DialogDescription className="sr-only">
          Configure execution, ideation, workspace, and access settings.
        </DialogDescription>
        {/* Header */}
        <div className="settings-modal__head shrink-0">
          <div className="settings-modal__crumbs">
            <span className="lbl">
              Settings
            </span>
            {activeSectionMeta && (
              <>
                <span className="sep">/</span>
                <span className="cur">
                  {activeSectionMeta.label}
                </span>
              </>
            )}
          </div>
          <button
            type="button"
            onClick={requestClose}
            className="settings-modal__close focus:outline-none focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-[var(--accent-primary)]"
            aria-label="Close settings"
          >
            <X className="h-4 w-4" />
          </button>
        </div>

        {/* Body */}
        <div className="settings-modal__body flex-1 overflow-hidden">
          {/* Left rail — hidden below lg breakpoint */}
          <nav className="settings-nav hidden lg:flex flex-shrink-0 flex-col overflow-y-auto">
            {SETTINGS_GROUPS.map((group) => {
              const groupSections = SETTINGS_SECTIONS.filter(
                (s) => s.groupId === group.id
              );
              return (
                <div key={group.id} className="settings-nav__group">
                  <p className="settings-nav__label">
                    {group.label}
                  </p>
                  {groupSections.map((section) => {
                    const isActive = section.id === activeSection;
                    return (
                      <div
                        key={section.id}
                        role="button"
                        tabIndex={0}
                        data-section={section.id}
                        data-testid={`settings-section-${section.id}`}
                        aria-label={section.label}
                        onPointerEnter={() => warmSection(section.id)}
                        onFocus={() => warmSection(section.id)}
                        onClick={() => setActiveSection(section.id)}
                        onKeyDown={(e) => {
                          if (e.key === "Enter" || e.key === " ") {
                            e.preventDefault();
                            setActiveSection(section.id);
                          }
                        }}
                        aria-current={isActive ? "page" : undefined}
                        className="settings-nav__item"
                      >
                        <span className="block truncate">{section.label}</span>
                      </div>
                    );
                  })}
                </div>
              );
            })}
          </nav>

          {/* Mobile section selector — visible below lg breakpoint */}
          <div className="block lg:hidden w-full px-4 py-2 border-b border-[var(--border-subtle)] shrink-0">
            <select
              value={activeSection}
              onChange={(e) => setActiveSection(e.target.value as SettingsSectionId)}
              className="settings-input w-full focus:outline-none"
            >
              {SETTINGS_GROUPS.map((group) => {
                const groupSections = SETTINGS_SECTIONS.filter(
                  (s) => s.groupId === group.id
                );
                return (
                  <optgroup key={group.id} label={group.label}>
                    {groupSections.map((section) => (
                      <option key={section.id} value={section.id}>
                        {section.label}
                      </option>
                    ))}
                  </optgroup>
                );
              })}
            </select>
          </div>

          {/* Right pane */}
          <div className="settings-pane min-w-0 flex-1 overflow-hidden flex flex-col">
            <ScrollArea className="flex-1">
              <div className="settings-pane__inner">
                <SettingsSectionContent
                  section={activeSection}
                  executionSettings={executionSettings}
                  disabled={disabled}
                  isHydrated={isSectionHydrated}
                  onSettingsChange={onSettingsChange}
                />
              </div>
            </ScrollArea>
          </div>
        </div>

        {settingsError && (
          <div className="px-4 py-2 text-sm shrink-0 border-t border-[var(--border-subtle)] text-[var(--status-error)]">
            {settingsError}
          </div>
        )}
        </DialogContent>
      )}
    </Dialog>
  );
}
