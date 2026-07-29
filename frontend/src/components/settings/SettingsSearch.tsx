/**
 * Dialog-scoped settings search.
 *
 * Results navigate through the same destination setter the nav rail and leaf
 * tabs use, so persistence, warm-up, and composite-tab behavior are identical
 * to clicking through the UI. The ⌘K/Ctrl-K shortcut is registered only while
 * the dialog is open and torn down on close, so it never shadows an app-level
 * shortcut.
 */

import { useCallback, useEffect, useRef, useState } from "react";
import { Search, X } from "lucide-react";

import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";

import { searchSettings } from "./settings-search-index";
import type { SettingsDestination } from "./settings-registry";

export interface SettingsSearchProps {
  /** True only while the settings dialog is open. */
  isOpen: boolean;
  onNavigate: (destination: SettingsDestination) => void;
}

export function SettingsSearch({ isOpen, onNavigate }: SettingsSearchProps) {
  const [query, setQuery] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);
  const results = searchSettings(query);

  const clear = useCallback(() => setQuery(""), []);

  useEffect(() => {
    if (!isOpen) {
      setQuery("");
      return undefined;
    }
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key.toLowerCase() === "k" && (event.metaKey || event.ctrlKey)) {
        event.preventDefault();
        inputRef.current?.focus();
        inputRef.current?.select();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [isOpen]);

  const go = useCallback(
    (destination: SettingsDestination) => {
      clear();
      inputRef.current?.blur();
      onNavigate(destination);
    },
    [clear, onNavigate],
  );

  return (
    <div className="settings-search" data-testid="settings-search">
      <Search className="settings-search__icon" aria-hidden="true" />
      <input
        ref={inputRef}
        type="text"
        role="searchbox"
        aria-label="Search settings"
        aria-expanded={results.length > 0}
        placeholder="Search settings…"
        className="settings-search__input"
        value={query}
        onChange={(event) => setQuery(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "Escape" && query) {
            // Clear the query first; a second Escape closes the dialog.
            event.stopPropagation();
            clear();
            return;
          }
          const first = results[0];
          if (event.key === "Enter" && first) {
            event.preventDefault();
            go(first.tab ? { section: first.section, tab: first.tab } : { section: first.section });
          }
        }}
      />
      {query ? (
        <TooltipProvider>
          <Tooltip>
            <TooltipTrigger asChild>
              <button
                type="button"
                className="settings-search__clear"
                aria-label="Clear settings search"
                onClick={clear}
              >
                <X aria-hidden="true" />
              </button>
            </TooltipTrigger>
            <TooltipContent>Clear search</TooltipContent>
          </Tooltip>
        </TooltipProvider>
      ) : (
        <kbd className="settings-search__kbd" aria-hidden="true">
          ⌘K
        </kbd>
      )}

      {results.length > 0 ? (
        <ul
          role="listbox"
          aria-label="Settings search results"
          className="settings-search__results"
        >
          {results.map((result) => (
            <li key={`${result.section}:${result.tab ?? ""}:${result.label}`}>
              <button
                type="button"
                role="option"
                aria-selected={false}
                className="settings-search__result"
                onClick={() =>
                  go(
                    result.tab
                      ? { section: result.section, tab: result.tab }
                      : { section: result.section },
                  )
                }
              >
                <span className="settings-search__result-label">
                  {result.label}
                </span>
                <span className="settings-search__result-hint">
                  {result.hint}
                </span>
              </button>
            </li>
          ))}
        </ul>
      ) : null}
    </div>
  );
}
