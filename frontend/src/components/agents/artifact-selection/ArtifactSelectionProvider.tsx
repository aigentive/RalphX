import {
  type PropsWithChildren,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { MessageSquarePlus } from "lucide-react";
import { toast } from "sonner";

import { Button } from "@/components/ui/button";

import { ArtifactSelectionRegistrationContext } from "./ArtifactSelectionContext";
import {
  MAX_COMPOSER_EXCERPT_BYTES,
  type ArtifactExcerptSource,
  type ComposerExcerptReference,
} from "./artifactSelection.types";

interface PendingSelection {
  reference: ComposerExcerptReference;
  left: number;
  top: number;
}

interface ArtifactSelectionProviderProps extends PropsWithChildren {
  enabled: boolean;
  onAddExcerpt: (reference: ComposerExcerptReference) => void;
}

const INTERACTIVE_SELECTOR =
  "button, input, textarea, select, [role='button'], [contenteditable='true'], [data-artifact-selection-exclude='true']";

function depth(element: HTMLElement): number {
  let value = 0;
  let current: HTMLElement | null = element;
  while (current) {
    value += 1;
    current = current.parentElement;
  }
  return value;
}

function nearestRegion(
  node: Node | null,
  regions: ReadonlyMap<HTMLElement, ArtifactExcerptSource>,
): [HTMLElement, ArtifactExcerptSource] | null {
  if (!node) return null;
  const matches = [...regions.entries()].filter(([element]) =>
    element.contains(node),
  );
  matches.sort(([left], [right]) => depth(right) - depth(left));
  return matches[0] ?? null;
}

function rangeContainsInteractiveContent(range: Range): boolean {
  const fragment = range.cloneContents();
  return fragment.querySelector?.(INTERACTIVE_SELECTOR) !== null;
}

function endpointHasInteractiveAncestor(
  node: Node | null,
  region: HTMLElement,
): boolean {
  const element = node instanceof Element ? node : node?.parentElement;
  const interactiveAncestor = element?.closest(INTERACTIVE_SELECTOR);
  return Boolean(interactiveAncestor && region.contains(interactiveAncestor));
}

export function ArtifactSelectionProvider({
  enabled,
  onAddExcerpt,
  children,
}: ArtifactSelectionProviderProps) {
  const regionsRef = useRef(new Map<HTMLElement, ArtifactExcerptSource>());
  const popoverRef = useRef<HTMLDivElement>(null);
  const [pending, setPending] = useState<PendingSelection | null>(null);

  const dismiss = useCallback(() => setPending(null), []);
  const registration = useMemo(
    () => ({
      register: (element: HTMLElement, source: ArtifactExcerptSource) => {
        regionsRef.current.set(element, source);
        return () => {
          regionsRef.current.delete(element);
          setPending(null);
        };
      },
    }),
    [],
  );

  const inspectSelection = useCallback(() => {
    if (!enabled) {
      dismiss();
      return;
    }
    const selection = window.getSelection();
    if (!selection || selection.isCollapsed || selection.rangeCount !== 1) {
      dismiss();
      return;
    }
    const anchorRegion = nearestRegion(selection.anchorNode, regionsRef.current);
    const focusRegion = nearestRegion(selection.focusNode, regionsRef.current);
    if (!anchorRegion || !focusRegion || anchorRegion[0] !== focusRegion[0]) {
      dismiss();
      return;
    }
    if (
      endpointHasInteractiveAncestor(selection.anchorNode, anchorRegion[0]) ||
      endpointHasInteractiveAncestor(selection.focusNode, focusRegion[0])
    ) {
      dismiss();
      return;
    }
    const range = selection.getRangeAt(0);
    if (rangeContainsInteractiveContent(range)) {
      dismiss();
      return;
    }
    const excerpt = selection.toString().trim();
    if (!excerpt) {
      dismiss();
      return;
    }
    if (new TextEncoder().encode(excerpt).byteLength > MAX_COMPOSER_EXCERPT_BYTES) {
      dismiss();
      toast.error("Selection is too large to add as conversation context");
      return;
    }
    const rect = range.getBoundingClientRect();
    if (!Number.isFinite(rect.left) || !Number.isFinite(rect.bottom)) {
      dismiss();
      return;
    }
    const source = anchorRegion[1];
    setPending({
      reference: { ...source, excerpt },
      left: Math.min(
        Math.max(rect.left + rect.width / 2, 104),
        Math.max(window.innerWidth - 104, 104),
      ),
      top: Math.min(rect.bottom + 8, Math.max(window.innerHeight - 44, 8)),
    });
  }, [dismiss, enabled]);

  useEffect(() => {
    if (!pending) return;
    const handleEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") dismiss();
    };
    const handleOutsidePointer = (event: PointerEvent) => {
      if (!popoverRef.current?.contains(event.target as Node)) dismiss();
    };
    const handleViewportChange = () => dismiss();
    document.addEventListener("keydown", handleEscape);
    document.addEventListener("pointerdown", handleOutsidePointer, true);
    window.addEventListener("resize", handleViewportChange);
    window.addEventListener("scroll", handleViewportChange, true);
    return () => {
      document.removeEventListener("keydown", handleEscape);
      document.removeEventListener("pointerdown", handleOutsidePointer, true);
      window.removeEventListener("resize", handleViewportChange);
      window.removeEventListener("scroll", handleViewportChange, true);
    };
  }, [dismiss, pending]);

  const handleAdd = useCallback(() => {
    if (!pending) return;
    onAddExcerpt(pending.reference);
    window.getSelection()?.removeAllRanges();
    dismiss();
  }, [dismiss, onAddExcerpt, pending]);

  return (
    <ArtifactSelectionRegistrationContext.Provider value={registration}>
      <div className="contents" onPointerUp={inspectSelection} onKeyUp={inspectSelection}>
        {children}
      </div>
      {pending ? (
        <div
          ref={popoverRef}
          data-testid="artifact-selection-action"
          className="fixed z-[100] -translate-x-1/2 rounded-lg p-1 shadow-xl"
          style={{
            left: pending.left,
            top: pending.top,
            backgroundColor: "var(--bg-elevated)",
            borderColor: "var(--border-subtle)",
            borderStyle: "solid",
            borderWidth: "1px",
          }}
        >
          <Button
            type="button"
            size="sm"
            className="h-8 gap-1.5 px-2.5 text-xs"
            aria-label="Add selection to conversation"
            onPointerDown={(event) => event.preventDefault()}
            onClick={handleAdd}
          >
            <MessageSquarePlus className="h-3.5 w-3.5" />
            Add to conversation
          </Button>
        </div>
      ) : null}
    </ArtifactSelectionRegistrationContext.Provider>
  );
}
