/**
 * DebateAdvocateCard - Collapsible card for narrow (stacked) debate layout
 *
 * Used in the DebateSummary component when viewport is <768px.
 * Glass-morphism card with chevron toggle, warm orange winner highlight.
 */

import { useState } from "react";
import { ChevronDown } from "lucide-react";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import { cn } from "@/lib/utils";
import type { DebateAdvocate } from "./DebateSummary";

// ============================================================================
// Types
// ============================================================================

interface DebateAdvocateCardProps {
  advocate: DebateAdvocate;
  isWinner: boolean;
  defaultOpen?: boolean;
}

// ============================================================================
// Section Header
// ============================================================================

function SectionHeader({ label }: { label: string }) {
  return (
    <h4
      className="text-[0.6875rem] uppercase tracking-wide font-medium mb-1.5"
      style={{ color: "var(--text-muted)" }}
    >
      {label}
    </h4>
  );
}

// ============================================================================
// Component
// ============================================================================

export function DebateAdvocateCard({
  advocate,
  isWinner,
  defaultOpen = false,
}: DebateAdvocateCardProps) {
  const [isOpen, setIsOpen] = useState(defaultOpen);

  return (
    <div data-testid={`advocate-card-${advocate.name}`}>
      <Collapsible open={isOpen} onOpenChange={setIsOpen}>
        <div
          className="rounded-xl transition-all duration-200"
          style={{
            background: "var(--overlay-faint)",
            border: isWinner
              ? "1px solid var(--accent-primary)"
              : "1px solid var(--overlay-faint)",
          }}
        >
          <CollapsibleTrigger asChild>
            <button
              data-testid={`advocate-trigger-${advocate.name}`}
              className="flex items-center gap-3 w-full text-left px-4 py-3"
            >
              <ChevronDown
                className={cn(
                  "w-4 h-4 transition-transform duration-200 flex-shrink-0",
                  !isOpen && "-rotate-90"
                )}
                style={{
                  color: isWinner ? "var(--accent-primary)" : "var(--text-muted)",
                }}
              />

              <div className="flex-1 min-w-0">
                <div className="flex items-center gap-2">
                  <span
                    className="text-[0.8125rem] font-medium tracking-[-0.01em]"
                    style={{
                      color: isWinner
                        ? "var(--accent-primary)"
                        : "var(--text-primary)",
                    }}
                  >
                    {advocate.name}
                  </span>
                  <span
                    className="text-[0.625rem] font-medium px-1.5 py-0.5 rounded-md"
                    style={{
                      background: "var(--overlay-faint)",
                      border: "1px solid var(--overlay-faint)",
                      color: advocate.color ?? "var(--text-muted)",
                    }}
                  >
                    {advocate.role}
                  </span>
                </div>
              </div>
            </button>
          </CollapsibleTrigger>

          <CollapsibleContent>
            <div className="px-4 pb-4 space-y-4">
              {/* Strengths */}
              <div>
                <SectionHeader label="Strengths" />
                <ul className="space-y-1">
                  {advocate.strengths.map((s) => (
                    <li
                      key={s}
                      className="text-[0.75rem] leading-relaxed pl-3 relative"
                      style={{ color: "var(--text-secondary)" }}
                    >
                      <span
                        className="absolute left-0 top-[6px] w-1 h-1 rounded-full"
                        style={{ background: "var(--status-success)" }}
                      />
                      {s}
                    </li>
                  ))}
                </ul>
              </div>

              {/* Weaknesses */}
              <div>
                <SectionHeader label="Weaknesses" />
                <ul className="space-y-1">
                  {advocate.weaknesses.map((w) => (
                    <li
                      key={w}
                      className="text-[0.75rem] leading-relaxed pl-3 relative"
                      style={{ color: "var(--text-secondary)" }}
                    >
                      <span
                        className="absolute left-0 top-[6px] w-1 h-1 rounded-full"
                        style={{ background: "var(--status-error)" }}
                      />
                      {w}
                    </li>
                  ))}
                </ul>
              </div>

              {/* Evidence */}
              <div>
                <SectionHeader label="Evidence" />
                <ul className="space-y-1">
                  {advocate.evidence.map((e) => (
                    <li
                      key={e}
                      className="text-[0.75rem] leading-relaxed pl-3 relative"
                      style={{ color: "var(--text-secondary)" }}
                    >
                      <span
                        className="absolute left-0 top-[6px] w-1 h-1 rounded-full"
                        style={{ background: "var(--text-muted)" }}
                      />
                      {e}
                    </li>
                  ))}
                </ul>
              </div>

              {/* Critic Challenge */}
              <div>
                <SectionHeader label="Critic Challenge" />
                <p
                  className="text-[0.75rem] leading-relaxed italic pl-3"
                  style={{
                    color: "var(--text-secondary)",
                    borderLeft: "2px solid var(--overlay-faint)",
                  }}
                >
                  {advocate.criticChallenge}
                </p>
              </div>
            </div>
          </CollapsibleContent>
        </div>
      </Collapsible>
    </div>
  );
}
