/**
 * ApiKeysSection - Settings section for managing external API keys.
 *
 * Lists existing API keys and provides a "Create API Key" button
 * that opens CreateKeyDialog.
 */

import { useState } from "react";
import { KeyRound, Plus, AlertCircle } from "lucide-react";
import { Button } from "@/components/ui/button";
import { SettingsSection } from "./SettingsView.shared";
import { ApiKeyEntry } from "./ApiKeyEntry";
import { CreateKeyDialog } from "./CreateKeyDialog";
import { useApiKeys } from "@/hooks/useApiKeys";
import { withAlpha } from "@/lib/theme-colors";

// ============================================================================
// Component
// ============================================================================

export function ApiKeysSection() {
  const { data: keys = [], isLoading, error } = useApiKeys();
  const [dialogOpen, setDialogOpen] = useState(false);

  return (
    <>
      <SettingsSection>
        {/* Error state */}
        {error && (
          <div className="mb-3 px-3 py-2 rounded-md flex items-center gap-2 bg-status-error/10 border border-status-error/20">
            <AlertCircle className="w-4 h-4 text-status-error shrink-0" />
            <p className="text-sm text-status-error">{error.message}</p>
          </div>
        )}

        {/* Loading state */}
        {isLoading && (
          <div className="py-4 flex items-center justify-center">
            <div className="w-4 h-4 border-2 border-[var(--accent-primary)] border-t-transparent rounded-full animate-spin" />
          </div>
        )}

        {/* Key list */}
        {!isLoading && !error && keys.length > 0 && (
          <div className="space-y-0">
            {keys.map((key) => (
              <ApiKeyEntry
                key={key.id}
                apiKey={key}
              />
            ))}
          </div>
        )}

        {/* Empty state */}
        {!isLoading && !error && keys.length === 0 && (
          <div className="py-6 flex flex-col items-center text-center gap-2">
            <div
              className="p-3 rounded-full"
              style={{ background: withAlpha("var(--accent-primary)", 6) }}
            >
              <KeyRound className="w-5 h-5 text-[var(--text-muted)]" />
            </div>
            <p className="text-sm text-[var(--text-muted)]">
              No API keys yet
            </p>
            <p className="text-xs text-[var(--text-muted)] max-w-[240px]">
              Create a key to allow external tools or CI pipelines to access RalphX
            </p>
          </div>
        )}

        {/* Create button */}
        {!isLoading && (
          <div className="pt-3 border-t border-[var(--border-subtle)] mt-3">
            <Button
              variant="ghost"
              size="sm"
              onClick={() => setDialogOpen(true)}
              data-testid="create-api-key-button"
              className="h-8 px-3 text-xs text-[var(--text-secondary)] hover:text-[var(--text-primary)] hover:bg-[var(--bg-hover)] gap-1.5"
            >
              <Plus className="w-3.5 h-3.5" />
              Create API Key
            </Button>
          </div>
        )}
      </SettingsSection>

      <CreateKeyDialog
        open={dialogOpen}
        onClose={() => setDialogOpen(false)}
        onCreated={() => setDialogOpen(false)}
      />
    </>
  );
}
