import { useEffect, useRef, useState } from "react";
import { toast } from "sonner";
import { useEventBus } from "@/providers/EventProvider";
import { api } from "@/lib/tauri";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  DialogFooter,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { AlertTriangle, Shield, Terminal } from "lucide-react";
import { useTaskStore } from "@/stores/taskStore";
import type { PermissionRequest, PermissionExpiredEvent } from "@/types/permission";

/**
 * Global permission dialog for approving agent tool usage.
 *
 * Listens to `permission:request` events from the backend and displays
 * a modal dialog asking the user to approve or deny the tool call.
 *
 * Features:
 * - Queues multiple permission requests (shows first, counts remaining)
 * - Formats tool input preview based on tool type (Bash, Write, Edit, Read)
 * - Calls `resolve_permission_request` Tauri command on decision
 * - Closing dialog is treated as "deny"
 * - Shows agent identity (agent type, context type, task name) when available
 * - Prevents double-submit with resolvingId state
 * - Hydrates queue on mount from backend in-memory state (D7)
 * - Removes expired requests via `permission:expired` event (D9)
 * - Smart error handling: "not found" removes from queue, transport errors retry (D4)
 * - Manual dismiss button for stale/stuck requests (D6)
 */

const AGENT_BADGE_CONFIG: Record<string, { label: string; colorVar: string }> = {
  "ralphx-execution-worker": { label: "Worker", colorVar: "--status-info" },
  "ralphx-execution-coder": { label: "Coder", colorVar: "--status-info" },
  "ralphx-execution-merger": { label: "Merger", colorVar: "--status-warning" },
  "ralphx-ideation": { label: "Ideation", colorVar: "--accent-primary" },
};

const CONTEXT_LABEL_MAP: Record<string, string> = {
  task_execution: "Executing",
  review: "Reviewing",
  merge: "Merging",
  ideation: "Ideation",
  task: "Task Chat",
  project: "Project Chat",
};

type BufferedEvent =
  | { type: "permission:request"; payload: PermissionRequest }
  | { type: "permission:expired"; payload: PermissionExpiredEvent };

function getStringField(
  input: Record<string, unknown>,
  keys: readonly string[]
): string | undefined {
  for (const key of keys) {
    const value = input[key];
    if (typeof value === "string" && value.length > 0) {
      return value;
    }
  }
  return undefined;
}

function getPathField(input: Record<string, unknown>): string | undefined {
  return getStringField(input, ["file_path", "filePath", "path"]);
}

export function PermissionDialog() {
  const [requests, setRequests] = useState<PermissionRequest[]>([]);
  // D8: track WHICH request is being resolved, not just a boolean
  const [resolvingId, setResolvingId] = useState<string | null>(null);
  const eventBus = useEventBus();
  const currentRequest = requests[0];

  // Code quality #3: reactive task selector at component top level
  const tasks = useTaskStore((state) => state.tasks);

  // D7: hydration race guard refs
  const hydratingRef = useRef(false);
  const pendingEventsRef = useRef<BufferedEvent[]>([]);

  // D7: Hydration on mount — seed queue from backend in-memory state
  useEffect(() => {
    hydratingRef.current = true;

    api.permission.getPendingPermissions().then((pending) => {
      // Snapshot IDs from hydration response
      const snapshotIds = new Set(pending.map((r) => r.request_id));

      setRequests((prev) => {
        const existingIds = new Set(prev.map((r) => r.request_id));
        const newRequests = pending.filter((r) => !existingIds.has(r.request_id));
        return [...prev, ...newRequests];
      });

      // Replay buffered events in order
      const buffered = pendingEventsRef.current;
      pendingEventsRef.current = [];

      for (const event of buffered) {
        if (event.type === "permission:request") {
          setRequests((prev) => {
            if (prev.some((r) => r.request_id === event.payload.request_id)) return prev;
            return [...prev, event.payload];
          });
        } else if (event.type === "permission:expired") {
          const requestId = event.payload.request_id;
          // Buffer replay: skip toast if request was never in the hydration snapshot
          if (snapshotIds.has(requestId)) {
            toast.info("Permission request timed out");
          }
          setRequests((prev) => prev.filter((r) => r.request_id !== requestId));
        }
      }

      hydratingRef.current = false;
    }).catch((err) => {
      console.error("Failed to hydrate pending permissions:", err);
      hydratingRef.current = false;
      pendingEventsRef.current = [];
    });
  }, []);

  // Listen to permission:request events from backend
  useEffect(() => {
    const unsubscribe = eventBus.subscribe<PermissionRequest>("permission:request", (payload) => {
      if (hydratingRef.current) {
        pendingEventsRef.current.push({ type: "permission:request", payload });
        return;
      }
      setRequests((prev) => {
        // Dedupe by request_id
        if (prev.some((r) => r.request_id === payload.request_id)) return prev;
        return [...prev, payload];
      });
    });

    return unsubscribe;
  }, [eventBus]);

  // D9: permission:expired event listener with D8 race guard
  useEffect(() => {
    const unsubscribe = eventBus.subscribe<PermissionExpiredEvent>("permission:expired", (payload) => {
      if (hydratingRef.current) {
        pendingEventsRef.current.push({ type: "permission:expired", payload });
        return;
      }

      const expiredRequestId = payload.request_id;

      // D8: if this request is currently being resolved, skip toast (resolve catch will handle it)
      // but still schedule removal
      setResolvingId((currentResolvingId) => {
        if (currentResolvingId !== expiredRequestId) {
          // Not the active request — show toast
          toast.info("Permission request timed out");
        }
        return currentResolvingId;
      });

      // D9: defer queue removal via setTimeout to ensure toast renders before modal closes
      setTimeout(() => {
        setRequests((prev) => prev.filter((r) => r.request_id !== expiredRequestId));
      }, 0);
    });

    return unsubscribe;
  }, [eventBus]);

  // The notification center is a second entry point to this same queue. Reload
  // authoritative pending state and move the selected request to the front.
  useEffect(() => {
    const reopen = (event: Event) => {
      const requestId = event instanceof CustomEvent && typeof event.detail?.requestId === "string"
        ? event.detail.requestId
        : undefined;
      void api.permission.getPendingPermissions().then((pending) => {
        setRequests((previous) => {
          const merged = [...previous, ...pending.filter((request) => !previous.some((known) => known.request_id === request.request_id))];
          if (!requestId) return merged;
          const selected = merged.find((request) => request.request_id === requestId);
          return selected ? [selected, ...merged.filter((request) => request.request_id !== requestId)] : merged;
        });
      }).catch((error: unknown) => {
        console.error("Failed to re-open pending permission:", error);
      });
    };
    window.addEventListener("ralphx:open-permission-dialog", reopen);
    return () => window.removeEventListener("ralphx:open-permission-dialog", reopen);
  }, []);

  const handleDecision = async (decision: "allow" | "deny") => {
    if (!currentRequest) return;

    // D8: set resolvingId to current request's ID
    setResolvingId(currentRequest.request_id);
    try {
      await api.permission.resolveRequest({
        requestId: currentRequest.request_id,
        decision,
        ...(decision === "deny" && { message: "User denied permission" }),
      });
      // Remove from queue only on success
      setRequests((prev) => prev.slice(1));
    } catch (error) {
      console.error("Failed to resolve permission:", error);
      // D4: normalize error and split on "not found"
      const message = error instanceof Error ? error.message : String(error);
      if (message.includes("not found")) {
        // Request was already expired/removed — remove from queue, show info
        setRequests((prev) => prev.slice(1));
        toast.info("Permission request expired");
      } else {
        // Transport or unexpected error — keep in queue for retry
        toast.error("Failed to resolve permission request, please retry");
      }
    } finally {
      // D8: clear resolvingId on completion or error
      setResolvingId(null);
    }
  };

  // D6: hide removes from frontend queue only — no backend call
  const handleDismiss = () => {
    if (!currentRequest) return;
    setRequests((prev) => prev.filter((r) => r.request_id !== currentRequest.request_id));
    toast.info("Permission request hidden");
  };

  // Dialog not visible when no requests
  if (!currentRequest) return null;

  const toolInputPreview = formatToolInput(
    currentRequest.tool_name,
    currentRequest.tool_input
  );

  const hasIdentity =
    Boolean(currentRequest.agent_type) ||
    Boolean(currentRequest.context_type) ||
    Boolean(currentRequest.task_id);

  // Code quality #3: use reactive tasks selector from component top level
  const taskTitle = currentRequest.task_id
    ? (tasks[currentRequest.task_id]?.title ?? currentRequest.task_id.slice(0, 8))
    : null;

  return (
    <Dialog
      open
      onOpenChange={(open) => {
        // D8: guard uses resolvingId !== null
        if (resolvingId !== null) return;
        if (!open) void handleDecision("deny");
      }}
    >
      <DialogContent className="sm:max-w-[500px] max-h-[85vh] flex flex-col">
        <DialogHeader className="shrink-0">
          <div className="flex items-center gap-2">
            <div
              className="p-2 rounded-full"
              style={{
                backgroundColor: "var(--status-warning-muted)",
              }}
            >
              <AlertTriangle className="h-5 w-5" style={{ color: "var(--status-warning)" }} />
            </div>
            <DialogTitle>Permission Required</DialogTitle>
          </div>
          <DialogDescription>
            An agent is requesting permission to use a tool
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4 py-4 px-6 overflow-y-auto min-h-0">
          {/* Agent identity row */}
          {hasIdentity && (
            <div
              className="rounded-md p-3 space-y-1"
              style={{
                backgroundColor: "var(--bg-surface)",
                border: "1px solid var(--border-subtle)",
              }}
            >
              {/* Badges row */}
              <div className="flex items-center gap-2 flex-wrap">
                {currentRequest.agent_type && (() => {
                  const badge =
                    AGENT_BADGE_CONFIG[currentRequest.agent_type] ??
                    { label: currentRequest.agent_type, colorVar: "--text-secondary" };
                  return (
                    <span
                      className="text-xs font-medium px-2 py-0.5 rounded"
                      style={{
                        backgroundColor: `color-mix(in srgb, var(${badge.colorVar}) 15%, transparent)`,
                        color: `var(${badge.colorVar})`,
                      }}
                    >
                      {badge.label}
                    </span>
                  );
                })()}
                {currentRequest.context_type && (
                  <span className="text-xs" style={{ color: "var(--text-secondary)" }}>
                    {CONTEXT_LABEL_MAP[currentRequest.context_type] ?? currentRequest.context_type}
                  </span>
                )}
              </div>
              {/* Task name row */}
              {taskTitle && (
                <p className="text-xs" style={{ color: "var(--text-muted)" }}>
                  Task: {taskTitle}
                </p>
              )}
            </div>
          )}

          {/* Tool name */}
          <div className="flex items-center gap-2 text-sm">
            <Terminal className="h-4 w-4 shrink-0" style={{ color: "var(--text-muted)" }} />
            <span
              className="font-medium"
              style={{ color: "var(--text-primary)" }}
              data-testid="permission-tool-name"
            >
              {currentRequest.tool_name}
            </span>
          </div>

          {/* Tool input preview */}
          <div
            className="rounded-md p-3 font-mono text-sm overflow-x-auto max-h-[50vh]"
            style={{
              backgroundColor: "var(--bg-surface)",
              border: "1px solid var(--border-subtle)",
            }}
          >
            <pre
              className="whitespace-pre-wrap break-all"
              style={{ color: "var(--text-secondary)" }}
              data-testid="permission-input-preview"
            >
              {toolInputPreview}
            </pre>
          </div>

          {/* Context if provided */}
          {currentRequest.context && (
            <p
              className="text-sm"
              style={{ color: "var(--text-secondary)" }}
              data-testid="permission-context"
            >
              {currentRequest.context}
            </p>
          )}

          <p
            className="text-xs"
            style={{ color: "var(--text-muted)" }}
            data-testid="permission-decision-hint"
          >
            Allow approves this exact request and lets the agent continue. Hide only closes this dialog locally.
          </p>

          {/* Queue indicator */}
          {requests.length > 1 && (
            <p
              className="text-xs"
              style={{ color: "var(--text-muted)" }}
              data-testid="permission-queue-count"
            >
              +{requests.length - 1} more permission request(s) waiting
            </p>
          )}
        </div>

        {/* D6: Dismiss left-aligned, Deny+Allow right-aligned */}
        <DialogFooter className="shrink-0 flex items-center justify-between sm:justify-between">
          <Button
            variant="ghost"
            className="text-sm"
            style={{ color: "var(--text-muted)" }}
            onClick={handleDismiss}
            disabled={resolvingId !== null}
          >
            Hide
          </Button>
          <div className="flex gap-2">
            <Button
              variant="outline"
              onClick={() => void handleDecision("deny")}
              disabled={resolvingId !== null}
            >
              Deny
            </Button>
            <Button onClick={() => void handleDecision("allow")} disabled={resolvingId !== null}>
              <Shield className="h-4 w-4 mr-2" />
              Allow
            </Button>
          </div>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

/**
 * Format tool input for display based on tool type.
 *
 * - Bash: show command
 * - Write: show file path + content preview (first 200 chars)
 * - Edit: show file path + old/new strings
 * - Read: show file path
 * - Default: JSON.stringify
 */
function formatToolInput(
  toolName: string,
  input: Record<string, unknown>
): string {
  switch (toolName) {
    case "Bash":
      return (input.command as string) || JSON.stringify(input, null, 2);
    case "Glob": {
      const pattern = getStringField(input, ["pattern"]);
      return pattern ? `Glob: ${pattern}` : JSON.stringify(input, null, 2);
    }
    case "Write": {
      const targetPath = getPathField(input) ?? "(path unavailable)";
      const content = getStringField(input, ["content"]) ?? "";
      const preview = content?.slice(0, 200) || "";
      const truncated = content?.length > 200 ? "..." : "";
      return `Write to: ${targetPath}\n\n${preview}${truncated}`;
    }
    case "Edit": {
      const targetPath = getPathField(input) ?? "(path unavailable)";
      const oldString = getStringField(input, ["old_string", "oldString"]) ?? "";
      const newString = getStringField(input, ["new_string", "newString"]) ?? "";
      return `Edit: ${targetPath}\n- "${oldString}"\n+ "${newString}"`;
    }
    case "Read": {
      const targetPath = getPathField(input) ?? "(path unavailable)";
      return `Read: ${targetPath}`;
    }
    default:
      return JSON.stringify(input, null, 2);
  }
}
