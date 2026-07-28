/**
 * PlanEditor - Markdown editor for plan artifacts
 *
 * Features:
 * - Markdown editor with preview toggle
 * - Save and Cancel buttons
 * - Calls update_plan_artifact HTTP endpoint on save
 */

import { useAgentGate } from "@/hooks/useAgentGate";
import { useState, useCallback } from "react";
import { Save, X, Eye, Edit2 } from "lucide-react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { Card } from "@/components/ui/card";
import { backendFetch } from "@/api/backend";
import { ArtifactResponseSchema, transformArtifactResponse } from "@/api/artifact";
import type { Artifact } from "@/types/artifact";
import { PlanTemplateSelector } from "./PlanTemplateSelector";

// ============================================================================
// Types
// ============================================================================

export interface PlanEditorProps {
  /** The plan artifact being edited */
  plan: Artifact;
  /** Callback when save is successful */
  onSave: (updatedPlan: Artifact) => void;
  /** Callback when cancel is clicked */
  onCancel: () => void;
  /** Whether this is a new plan (shows template selector) */
  isNewPlan?: boolean;
}

// ============================================================================
// Markdown Components (reused from PlanDisplay)
// ============================================================================

const markdownComponents = {
  a: ({ href, children, ...props }: React.AnchorHTMLAttributes<HTMLAnchorElement>) => (
    <a
      href={href}
      target="_blank"
      rel="noopener noreferrer"
      className="underline hover:no-underline text-[var(--accent-primary)]"
      {...props}
    >
      {children}
    </a>
  ),
  code: ({ className, children, ...props }: React.HTMLAttributes<HTMLElement>) => {
    const isBlock = className?.includes("language-");
    if (isBlock) {
      return (
        <code
          className={`block p-3 rounded text-sm overflow-x-auto bg-[var(--bg-elevated)] ${className || ""}`}
          {...props}
        >
          {children}
        </code>
      );
    }
    return (
      <code className="px-1 py-0.5 rounded text-sm bg-[var(--bg-elevated)]" {...props}>
        {children}
      </code>
    );
  },
  pre: ({ children, ...props }: React.HTMLAttributes<HTMLPreElement>) => (
    <pre className="my-2 rounded overflow-hidden bg-[var(--bg-elevated)]" {...props}>
      {children}
    </pre>
  ),
  p: ({ children, ...props }: React.HTMLAttributes<HTMLParagraphElement>) => (
    <p className="mb-2 last:mb-0" {...props}>
      {children}
    </p>
  ),
  ul: ({ children, ...props }: React.HTMLAttributes<HTMLUListElement>) => (
    <ul className="list-disc list-inside mb-2 space-y-1" {...props}>
      {children}
    </ul>
  ),
  ol: ({ children, ...props }: React.HTMLAttributes<HTMLOListElement>) => (
    <ol className="list-decimal list-inside mb-2 space-y-1" {...props}>
      {children}
    </ol>
  ),
  li: ({ children, ...props }: React.LiHTMLAttributes<HTMLLIElement>) => (
    <li className="mb-1" {...props}>
      {children}
    </li>
  ),
  h1: ({ children, ...props }: React.HTMLAttributes<HTMLHeadingElement>) => (
    <h1 className="text-2xl font-semibold mb-3 mt-4 first:mt-0" {...props}>
      {children}
    </h1>
  ),
  h2: ({ children, ...props }: React.HTMLAttributes<HTMLHeadingElement>) => (
    <h2 className="text-xl font-semibold mb-2 mt-3" {...props}>
      {children}
    </h2>
  ),
  h3: ({ children, ...props }: React.HTMLAttributes<HTMLHeadingElement>) => (
    <h3 className="text-lg font-semibold mb-2 mt-3" {...props}>
      {children}
    </h3>
  ),
  // Table support (GFM)
  table: ({ children, ...props }: React.TableHTMLAttributes<HTMLTableElement>) => (
    <div className="my-3 overflow-x-auto rounded-lg border border-white/[0.06]">
      <table className="w-full text-sm border-collapse" {...props}>
        {children}
      </table>
    </div>
  ),
  thead: ({ children, ...props }: React.HTMLAttributes<HTMLTableSectionElement>) => (
    <thead className="bg-white/[0.02]" {...props}>
      {children}
    </thead>
  ),
  tbody: ({ children, ...props }: React.HTMLAttributes<HTMLTableSectionElement>) => (
    <tbody {...props}>{children}</tbody>
  ),
  tr: ({ children, ...props }: React.HTMLAttributes<HTMLTableRowElement>) => (
    <tr className="border-b border-white/[0.06] last:border-b-0" {...props}>
      {children}
    </tr>
  ),
  th: ({ children, ...props }: React.ThHTMLAttributes<HTMLTableCellElement>) => (
    <th className="px-3 py-2 text-left text-xs font-medium text-[var(--text-primary)] uppercase tracking-wider" {...props}>
      {children}
    </th>
  ),
  td: ({ children, ...props }: React.TdHTMLAttributes<HTMLTableCellElement>) => (
    <td className="px-3 py-2 text-[var(--text-secondary)]" {...props}>
      {children}
    </td>
  ),
};

// ============================================================================
// Component
// ============================================================================

export function PlanEditor({ plan, onSave, onCancel, isNewPlan = false }: PlanEditorProps) {
  const agentGate = useAgentGate("artifactEdit");
  // Get initial content
  const initialContent = plan.content.type === "inline" ? plan.content.text : "";

  const [content, setContent] = useState(initialContent);
  const [isPreview, setIsPreview] = useState(false);
  const [isSaving, setIsSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Check if content has changed
  const hasChanges = content !== initialContent;

  // Handle template selection - replace content with template
  const handleTemplateSelect = useCallback((templateContent: string) => {
    setContent(templateContent);
  }, []);

  // Handle save - call HTTP endpoint
  const handleSave = useCallback(async () => {
    if (!hasChanges) {
      onCancel();
      return;
    }

    setIsSaving(true);
    setError(null);

    try {
      // Call the HTTP endpoint to update plan artifact
      const response = await backendFetch("update_plan_artifact", {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
        },
        body: JSON.stringify({
          artifact_id: plan.id,
          content,
        }),
      });

      if (!response.ok) {
        if (response.status === 409) {
          toast.error("Plan is frozen — verification agent is actively working. Wait for the current round to complete.");
          return;
        }
        throw new Error(`Failed to update plan: ${response.statusText}`);
      }

      const data = ArtifactResponseSchema.parse(await response.json());
      const updatedPlan: Artifact = transformArtifactResponse(data);

      onSave(updatedPlan);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to save plan");
    } finally {
      setIsSaving(false);
    }
  }, [plan.id, content, hasChanges, onSave, onCancel]);

  // Handle cancel
  const handleCancel = useCallback(() => {
    if (hasChanges) {
      const confirmed = window.confirm(
        "You have unsaved changes. Are you sure you want to cancel?"
      );
      if (!confirmed) return;
    }
    onCancel();
  }, [hasChanges, onCancel]);

  return (
    <Card className="overflow-hidden border-[var(--border-primary)] bg-[var(--bg-elevated)]">
      {/* Header */}
      <div className="flex items-center justify-between p-4 border-b border-[var(--border-primary)]">
        <div className="flex items-center gap-3">
          <Edit2 className="h-5 w-5 text-[var(--accent-primary)]" />
          <h3 className="font-semibold text-base text-[var(--text-primary)]">
            Edit Plan: {plan.name}
          </h3>
        </div>

        {/* Actions */}
        <div className="flex items-center gap-2">
          <Button
            variant="ghost"
            size="sm"
            onClick={() => setIsPreview(!isPreview)}
            className="hover:bg-[var(--bg-base)]"
          >
            {isPreview ? (
              <>
                <Edit2 className="h-4 w-4 mr-1.5" />
                Edit
              </>
            ) : (
              <>
                <Eye className="h-4 w-4 mr-1.5" />
                Preview
              </>
            )}
          </Button>

          <Button
            variant="ghost"
            size="sm"
            onClick={handleCancel}
            disabled={isSaving}
            className="hover:bg-[var(--bg-base)]"
          >
            <X className="h-4 w-4 mr-1.5" />
            Cancel
          </Button>

          <Button
            variant="default"
            size="sm"
            onClick={handleSave}
            disabled={isSaving || !hasChanges || agentGate.gated}
            title={agentGate.reason ?? undefined}
            className="bg-[var(--accent-primary)] hover:bg-[var(--accent-hover)] text-white"
          >
            <Save className="h-4 w-4 mr-1.5" />
            {isSaving ? "Saving..." : "Save"}
          </Button>
        </div>
      </div>

      {/* Error message */}
      {error && (
        <div className="p-4 bg-status-error/10 border-b border-status-error/20">
          <p className="text-sm text-status-error">{error}</p>
        </div>
      )}

      {/* Content area */}
      <div className="p-4 space-y-4">
        {/* Template selector - only show for new plans and when not in preview mode */}
        {isNewPlan && !isPreview && (
          <PlanTemplateSelector
            onTemplateSelect={handleTemplateSelect}
            disabled={isSaving}
          />
        )}

        {isPreview ? (
          // Preview mode
          <div className="prose prose-sm max-w-none text-[var(--text-primary)] min-h-[400px]">
            {content ? (
              <ReactMarkdown remarkPlugins={[remarkGfm]} components={markdownComponents}>{content}</ReactMarkdown>
            ) : (
              <p className="text-[var(--text-tertiary)] italic">No content to preview</p>
            )}
          </div>
        ) : (
          // Edit mode
          <Textarea
            value={content}
            onChange={(e) => setContent(e.target.value)}
            placeholder="Write your plan in markdown..."
            className="min-h-[400px] font-mono text-sm resize-none border-[var(--border-primary)] bg-[var(--bg-base)] text-[var(--text-primary)] focus:ring-[var(--accent-primary)]"
            disabled={isSaving}
          />
        )}
      </div>

      {/* Footer with save indicator */}
      {hasChanges && !isSaving && (
        <div className="px-4 py-2 border-t border-[var(--border-primary)] bg-[var(--bg-base)]">
          <p className="text-sm text-[var(--text-tertiary)]">
            You have unsaved changes
          </p>
        </div>
      )}
    </Card>
  );
}
