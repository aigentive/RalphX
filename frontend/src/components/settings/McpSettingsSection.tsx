import { useEffect, useMemo, useRef, useState } from "react";
import { ChevronDown, LockKeyhole, Network, Plus, RefreshCw, ServerCog } from "lucide-react";
import { toast } from "sonner";

import type {
  McpOverrideState,
  McpServer,
} from "@/api/mcp-policy";
import type { Harness } from "@/api/ideation-harness";
import { Button } from "@/components/ui/button";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { useHarnessProviders } from "@/hooks/useHarnessProviders";
import { useMcpPolicy } from "@/hooks/useMcpPolicy";
import { selectActiveProject, useProjectStore } from "@/stores/projectStore";
import { useUiStore } from "@/stores/uiStore";

import { ErrorBanner, SectionCard } from "./SettingsView.shared";
import { cancelScheduledJob, scheduleAfterPaint } from "./SettingsDialog.performance";

type Scope = "global" | "project";

const PROVIDER_LABELS: Record<string, string> = {
  claude: "Claude",
  codex: "Codex",
};

const STATE_LABELS: Record<McpOverrideState, string> = {
  follow: "Follow provider",
  enabled: "Enable",
  disabled: "Disable",
};

function providerLabel(provider: string) {
  return PROVIDER_LABELS[provider] ?? provider;
}

function PolicySelect({
  value,
  disabled,
  canEnable = true,
  label,
  onChange,
}: {
  value: McpOverrideState;
  disabled: boolean;
  canEnable?: boolean;
  label: string;
  onChange: (state: McpOverrideState) => void;
}) {
  return (
    <Select
      value={value}
      disabled={disabled}
      onValueChange={(next) => onChange(next as McpOverrideState)}
    >
      <SelectTrigger className="w-[150px]" aria-label={label}>
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        {(Object.keys(STATE_LABELS) as McpOverrideState[]).map((state) => (
          <SelectItem
            key={state}
            value={state}
            disabled={state === "enabled" && !canEnable}
          >
            {STATE_LABELS[state]}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}

function ServerPolicyCard({
  server,
  disabled,
  focused,
  onServerChange,
  onToolChange,
  onRepair,
}: {
  server: McpServer;
  disabled: boolean;
  focused: boolean;
  onServerChange: (state: McpOverrideState) => Promise<void>;
  onToolChange: (toolName: string, state: McpOverrideState) => Promise<void>;
  onRepair: () => Promise<void>;
}) {
  const cardRef = useRef<HTMLElement>(null);
  const [toolsOpen, setToolsOpen] = useState(focused);
  useEffect(() => {
    if (!focused) return;
    setToolsOpen(true);
    cardRef.current?.focus();
    cardRef.current?.scrollIntoView({ block: "center" });
  }, [focused]);
  const status = server.conflictKind
    ? "Reserved ID conflict"
    : server.locked
      ? "Required"
    : server.effectiveEnabled
      ? "Enabled"
      : "Disabled";
  return (
    <article
      ref={cardRef}
      tabIndex={-1}
      data-mcp-server-id={server.serverId}
      className="rounded-lg bg-[var(--bg-surface)] p-4 outline-none"
      style={{
        backgroundColor: "var(--bg-surface)",
        borderColor: focused ? "var(--accent-primary)" : "var(--border-subtle)",
        borderStyle: "solid",
        borderWidth: focused ? 2 : 1,
      }}
    >
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0">
          <div className="flex items-center gap-2">
            <h4 className="truncate text-sm font-semibold text-[var(--text-primary)]">
              {server.serverId}
            </h4>
            {server.locked && <LockKeyhole className="h-3.5 w-3.5 text-[var(--text-muted)]" />}
            <span className="rounded border border-[var(--border-subtle)] px-1.5 py-0.5 text-[10px] text-[var(--text-muted)]">
              {status}
            </span>
          </div>
          <p className="mt-1 text-xs text-[var(--text-muted)]">
            {server.lockedReason ??
              `${server.nativeScope ?? "provider"} · ${server.nativeState.replace(/_/g, " ")} · source ${server.effectiveSource.replace(/_/g, " ")}`}
          </p>
          {!server.locked && server.nativeState !== "enabled" && (
            <p className="mt-1 text-xs text-[var(--status-warning)]">
              Enable or approve this server in {providerLabel(server.provider)} before RalphX can use it.
            </p>
          )}
        </div>
        {!server.locked && (
          <PolicySelect
            value={server.configuredState}
            disabled={disabled}
            canEnable={server.nativeState === "enabled"}
            label={`${server.serverId} policy`}
            onChange={(state) => void onServerChange(state)}
          />
        )}
      </div>
      {server.diagnostic && (
        <p className="mt-3 rounded-md border border-[var(--status-warning-border)] bg-[var(--bg-elevated)] px-3 py-2 text-xs text-[var(--status-warning)]">
          {server.diagnostic}
        </p>
      )}
      {server.repairStatus === "repairable" && (
        <div className="mt-3 flex flex-wrap items-center gap-2">
          <Button
            type="button"
            size="sm"
            variant="outline"
            disabled={disabled}
            onClick={() => void onRepair()}
          >
            Retry cleanup
          </Button>
        </div>
      )}
      {server.knownTools.length > 0 && (
        <Collapsible open={toolsOpen} onOpenChange={setToolsOpen} className="mt-4 border-t border-[var(--border-subtle)]">
          <CollapsibleTrigger asChild>
            <button
              type="button"
              className="flex w-full items-center justify-between py-2.5 text-xs font-medium text-[var(--text-secondary)] focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-[var(--accent-primary)]"
              aria-label={`${toolsOpen ? "Collapse" : "Expand"} ${server.serverId} tools`}
            >
              <span>Tools ({server.knownTools.length})</span>
              <ChevronDown className={`h-4 w-4 transition-transform ${toolsOpen ? "rotate-180" : ""}`} />
            </button>
          </CollapsibleTrigger>
          <CollapsibleContent>
            <div className="max-h-64 overflow-y-auto overscroll-contain divide-y divide-[var(--border-subtle)]" tabIndex={0}>
              {!server.effectiveEnabled && (
                <p className="py-2.5 text-xs text-[var(--text-muted)]">
                  Tool controls are unavailable while this server is disabled.
                </p>
              )}
              {server.knownTools.map((tool) => (
                <div key={tool.toolName} className="flex flex-wrap items-center justify-between gap-3 py-2.5">
                  <div>
                    <code className="text-xs text-[var(--text-secondary)]">{tool.toolName}</code>
                    <p className="text-[10px] text-[var(--text-muted)]">
                      Effective: {tool.effectiveState} · {tool.effectiveSource.replace(/_/g, " ")}
                    </p>
                  </div>
                  <PolicySelect value={tool.configuredState} disabled={disabled || server.locked || !server.effectiveEnabled} label={`${server.serverId} ${tool.toolName} policy`} onChange={(state) => void onToolChange(tool.toolName, state)} />
                </div>
              ))}
            </div>
          </CollapsibleContent>
        </Collapsible>
      )}
    </article>
  );
}

export function McpSettingsSection() {
  const activeProject = useProjectStore(selectActiveProject);
  const openModal = useUiStore((state) => state.openModal);
  const modalContext = useUiStore((state) => state.modalContext);
  const [scope, setScope] = useState<Scope>("global");
  const [ready, setReady] = useState(false);
  const [provider, setProvider] = useState<Harness | null>(null);
  const [exactServer, setExactServer] = useState("");
  const [exactTool, setExactTool] = useState("");
  const focusedServerId =
    typeof modalContext?.["serverId"] === "string"
      ? modalContext["serverId"]
      : null;

  useEffect(() => {
    const job = scheduleAfterPaint(() => setReady(true));
    return () => cancelScheduledJob(job);
  }, []);

  const providerState = useHarnessProviders({ refreshRuntime: true, enabled: ready });
  const { refetchProviders } = providerState;
  const bootstrapEligibleProviders = useMemo(
    () => providerState.providers.filter((row) => row.enabled && row.available),
    [providerState.providers],
  );
  useEffect(() => {
    if (bootstrapEligibleProviders.length === 0) {
      setProvider(null);
      return;
    }
    if (provider && bootstrapEligibleProviders.some((row) => row.provider === provider)) return;
    const preferred = bootstrapEligibleProviders.find(
      (row) => row.provider === providerState.settings.defaultProvider,
    );
    const fallback = preferred ?? bootstrapEligibleProviders[0];
    if (fallback) setProvider(fallback.provider);
  }, [bootstrapEligibleProviders, provider, providerState.settings.defaultProvider]);
  useEffect(() => {
    if (!ready || modalContext?.["section"] !== "mcp") return;
    const requestedProvider = modalContext["provider"];
    if (requestedProvider === "claude" || requestedProvider === "codex") {
      setProvider(requestedProvider);
    }
    const requestedScope = modalContext["scope"];
    setScope(requestedScope === "project" || requestedScope === "local" ? "project" : "global");
  }, [modalContext, ready]);

  const projectId = scope === "project" ? activeProject?.id ?? null : null;
  const policy = useMcpPolicy(
    projectId,
    provider,
    ready && bootstrapEligibleProviders.length > 0,
  );
  const eligibleProviders = policy.catalog?.eligibleProviders ??
    bootstrapEligibleProviders.map((row) => row.provider);
  useEffect(() => {
    if (!policy.catalog) return;
    if (provider && eligibleProviders.includes(provider)) return;
    const preferred = policy.catalog.eligibleDefaultProvider;
    setProvider(
      preferred && eligibleProviders.includes(preferred)
        ? preferred
        : eligibleProviders[0] ?? null,
    );
  }, [eligibleProviders, policy.catalog, provider]);
  useEffect(() => {
    if (policy.error) {
      void refetchProviders();
    }
  }, [policy.error, refetchProviders]);
  const servers = useMemo(
    () => policy.catalog?.servers.filter((server) => server.provider === provider) ?? [],
    [policy.catalog?.servers, provider],
  );
  const run = async (action: () => Promise<unknown>) => {
    try {
      await action();
    } catch (error) {
      toast.error(error instanceof Error ? error.message : "MCP policy update failed");
    }
  };

  return (
    <div className="space-y-8">
      <SectionCard
        icon={<Network className="h-5 w-5" />}
        title="MCP"
        description="Inherit provider-native MCP servers and apply RalphX server or tool restrictions. Definitions, authentication, approvals, and trust remain provider-owned."
      >
        <div className="space-y-5">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <Tabs value={scope} onValueChange={(value) => setScope(value as Scope)}>
              <TabsList aria-label="MCP policy scope">
                <TabsTrigger value="global">Global Defaults</TabsTrigger>
                <TabsTrigger value="project" disabled={!activeProject}>
                  Project Overrides
                </TabsTrigger>
              </TabsList>
            </Tabs>
            {eligibleProviders.length > 0 && provider && (
              <div className="flex items-center gap-2">
                <Select value={provider} onValueChange={setProvider}>
                  <SelectTrigger className="w-[160px]" aria-label="MCP provider">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {eligibleProviders.map((providerName) => (
                      <SelectItem key={providerName} value={providerName}>
                        {providerLabel(providerName)}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  aria-label={`Refresh ${providerLabel(provider)} MCP catalog`}
                  disabled={policy.isFetching}
                  onClick={() => void run(() => policy.refreshProvider(provider))}
                >
                  <RefreshCw className={`mr-1.5 h-4 w-4 ${policy.isFetching ? "animate-spin" : ""}`} />
                  Refresh
                </Button>
              </div>
            )}
          </div>

          {scope === "project" && activeProject && (
            <p className="text-xs text-[var(--text-muted)]">
              Overrides for {activeProject.name}. Follow provider removes the UI override and reveals project YAML, global UI/YAML, then provider-native behavior.
            </p>
          )}
          {providerState.isError && (
            <ErrorBanner error="Provider readiness could not be loaded." onDismiss={() => undefined} />
          )}
          {policy.catalog?.probeStale && (
            <p role="status" className="text-xs text-[var(--status-warning)]">
              Provider readiness result is stale. Refresh before relying on this catalog.
            </p>
          )}
          {Object.entries(policy.catalog?.providerDiagnostics ?? {}).map(([providerName, diagnostic]) => (
            <p key={providerName} role="status" className="rounded-md border border-[var(--status-warning-border)] bg-[var(--bg-elevated)] px-3 py-2 text-xs text-[var(--status-warning)]">
              {diagnostic}
            </p>
          ))}
          {(policy.catalog?.policyDiagnostics ?? []).map((diagnostic) => (
            <p key={diagnostic} role="alert" className="rounded-md border border-[var(--status-warning-border)] bg-[var(--bg-elevated)] px-3 py-2 text-xs text-[var(--status-warning)]">
              {diagnostic}
            </p>
          ))}
          {ready && !providerState.isLoading && eligibleProviders.length === 0 && (
            <div className="rounded-lg border border-[var(--border-subtle)] bg-[var(--bg-surface)] p-5 text-center">
              <ServerCog className="mx-auto h-6 w-6 text-[var(--text-muted)]" />
              <h4 className="mt-2 text-sm font-semibold text-[var(--text-primary)]">No validated provider is enabled</h4>
              <p className="mx-auto mt-1 max-w-md text-xs text-[var(--text-muted)]">
                Enable a provider and complete its CLI validation before managing its MCP catalog.
              </p>
              <Button
                type="button"
                className="mt-3"
                onClick={() => openModal("settings", { section: "providers" })}
              >
                Manage providers
              </Button>
            </div>
          )}
          {policy.error && (
            <ErrorBanner
              error={policy.error instanceof Error ? policy.error.message : "MCP catalog failed to load."}
              onDismiss={() => undefined}
            />
          )}
          {eligibleProviders.length > 0 && policy.isLoading && (
            <div data-testid="mcp-catalog-loading" className="h-28 rounded-lg border border-[var(--border-subtle)] bg-[var(--bg-surface)]" />
          )}
          {ready &&
            !policy.isLoading &&
            !policy.error &&
            eligibleProviders.length > 0 &&
            servers.length === 0 && (
              <div className="rounded-lg border border-[var(--border-subtle)] bg-[var(--bg-surface)] p-5 text-center">
                <ServerCog className="mx-auto h-6 w-6 text-[var(--text-muted)]" />
                <h4 className="mt-2 text-sm font-semibold text-[var(--text-primary)]">
                  No MCP servers found
                </h4>
                <p className="mx-auto mt-1 max-w-md text-xs text-[var(--text-muted)]">
                  {provider
                    ? `${providerLabel(provider)} has no MCP servers configured. Servers added to the provider's native configuration will appear here after a refresh.`
                    : "Servers added to the provider's native configuration will appear here after a refresh."}
                </p>
              </div>
            )}
          {!policy.isLoading && servers.map((server) => (
            <ServerPolicyCard
              key={`${server.provider}:${server.serverId}`}
              server={server}
              disabled={policy.isUpdating}
              focused={server.serverId === focusedServerId}
              onServerChange={(state) =>
                run(() => policy.updateServer({ projectId, provider: server.provider, serverId: server.serverId, state }))
              }
              onToolChange={(toolName, state) =>
                run(() => policy.updateTool({ projectId, provider: server.provider, serverId: server.serverId, toolName, state }))
              }
              onRepair={() =>
                run(async () => {
                  if (
                    server.provider !== "claude" ||
                    server.serverId !== "ralphx" ||
                    server.nativeScope !== "user"
                  ) {
                    throw new Error("This MCP registration is not eligible for automatic cleanup.");
                  }
                  await policy.retryLegacyRepair({
                    provider: "claude",
                    serverId: "ralphx",
                    scope: "user",
                  });
                })
              }
            />
          ))}

          {provider && (
            <div className="rounded-lg border border-dashed border-[var(--border-subtle)] bg-[var(--bg-surface)] p-4">
              <h4 className="text-sm font-medium text-[var(--text-primary)]">Add an exact-name deny</h4>
              <p className="mt-1 text-xs text-[var(--text-muted)]">
                Use a provider server ID and optionally a tool name when the provider catalog cannot enumerate tools.
              </p>
              <div className="mt-3 flex flex-wrap gap-2">
                <Input value={exactServer} onChange={(event) => setExactServer(event.target.value)} placeholder="Server ID" className="min-w-[180px] flex-1" aria-label="Exact MCP server ID" />
                <Input value={exactTool} onChange={(event) => setExactTool(event.target.value)} placeholder="Tool name (optional)" className="min-w-[200px] flex-1" aria-label="Exact MCP tool name" />
                <Button
                  type="button"
                  variant="outline"
                  disabled={!exactServer.trim() || policy.isUpdating}
                  onClick={() => void run(async () => {
                    const serverId = exactServer.trim();
                    const toolName = exactTool.trim();
                    if (toolName) {
                      await policy.updateTool({ projectId, provider, serverId, toolName, state: "disabled" });
                    } else {
                      await policy.updateServer({ projectId, provider, serverId, state: "disabled" });
                    }
                    setExactServer("");
                    setExactTool("");
                  })}
                >
                  <Plus className="mr-1.5 h-4 w-4" /> Add deny
                </Button>
              </div>
            </div>
          )}
        </div>
      </SectionCard>

    </div>
  );
}
