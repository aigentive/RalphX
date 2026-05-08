/**
 * TeamFindingsSection - Collapsible team research findings display
 *
 * Shows specialist findings from team-ideated plans in a table format.
 * macOS Tahoe glass-morphism, warm orange accent.
 */

import { useState } from "react";
import { Users, ChevronDown } from "lucide-react";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import { cn } from "@/lib/utils";

// ============================================================================
// Types
// ============================================================================

export interface TeamFinding {
  specialist: string;
  keyFinding: string;
  color?: string;
}

export interface TeamFindingsSectionProps {
  findings: TeamFinding[];
  teamMode: "research" | "debate";
  teammateCount: number;
  defaultExpanded?: boolean;
}

// ============================================================================
// Component
// ============================================================================

export function TeamFindingsSection({
  findings,
  teamMode,
  teammateCount,
  defaultExpanded = false,
}: TeamFindingsSectionProps) {
  const [isOpen, setIsOpen] = useState(defaultExpanded);

  if (findings.length === 0) return null;

  const title = teamMode === "research" ? "Team Research Summary" : "Team Debate Summary";

  return (
    <Collapsible open={isOpen} onOpenChange={setIsOpen}>
      <div
        className="rounded-lg mb-4"
        style={{
          background: "var(--overlay-faint)",
          border: "1px solid var(--overlay-faint)",
        }}
      >
        <CollapsibleTrigger asChild>
          <button
            className="flex items-center gap-2.5 w-full px-3 py-2.5 text-left"
            aria-label={title}
          >
            <Users
              className="w-3.5 h-3.5 flex-shrink-0"
              style={{ color: "var(--accent-primary)" }}
            />
            <span
              className="text-[0.75rem] font-medium flex-1"
              style={{ color: "var(--text-primary)" }}
            >
              {title}
            </span>
            <span
              className="text-[0.625rem] font-medium w-5 h-5 rounded-full flex items-center justify-center flex-shrink-0"
              style={{
                background: "var(--accent-muted)",
                color: "var(--accent-primary)",
              }}
            >
              {teammateCount}
            </span>
            <ChevronDown
              className={cn(
                "w-3.5 h-3.5 transition-transform duration-200 flex-shrink-0",
                !isOpen && "-rotate-90"
              )}
              style={{ color: "var(--text-muted)" }}
            />
          </button>
        </CollapsibleTrigger>

        <CollapsibleContent>
          <div className="px-3 pb-3">
            <div
              className="overflow-hidden rounded-md"
              style={{ border: "1px solid var(--overlay-faint)" }}
            >
              <table className="w-full text-[0.75rem]">
                <thead>
                  <tr style={{ background: "var(--overlay-faint)" }}>
                    <th
                      className="px-3 py-2 text-left text-[0.625rem] font-medium uppercase tracking-wider"
                      style={{ color: "var(--text-muted)" }}
                    >
                      Specialist
                    </th>
                    <th
                      className="px-3 py-2 text-left text-[0.625rem] font-medium uppercase tracking-wider"
                      style={{ color: "var(--text-muted)" }}
                    >
                      Key Finding
                    </th>
                  </tr>
                </thead>
                <tbody>
                  {findings.map((finding) => (
                    <tr
                      key={finding.specialist}
                      className="border-t"
                      style={{ borderColor: "var(--overlay-faint)" }}
                    >
                      <td className="px-3 py-2 whitespace-nowrap">
                        <span className="flex items-center gap-2">
                          {finding.color && (
                            <span
                              className="w-2 h-2 rounded-full flex-shrink-0"
                              style={{ background: finding.color }}
                            />
                          )}
                          <span style={{ color: "var(--text-primary)" }}>
                            {finding.specialist}
                          </span>
                        </span>
                      </td>
                      <td
                        className="px-3 py-2"
                        style={{ color: "var(--text-secondary)" }}
                      >
                        {finding.keyFinding}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </div>
        </CollapsibleContent>
      </div>
    </Collapsible>
  );
}
