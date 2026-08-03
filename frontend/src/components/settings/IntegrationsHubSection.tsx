/**
 * Landing page for the Integrations nav entry: a card grid summarising every
 * integration and external-access target, each drilling into its existing,
 * untouched settings panel.
 *
 * Status comes from the same TanStack-cached hooks the panels use, so opening
 * the hub costs no extra fetching and the two surfaces cannot disagree. A
 * failed or missing status read renders as "not connected" — display only, no
 * gating logic lives here.
 */

import { ArrowRight } from "lucide-react";

import { useApiKeys } from "@/hooks/useApiKeys";
import {
  isAtlassianConnected,
  useAtlassianIntegration,
} from "@/hooks/useAtlassianIntegration";
import { useClickUpIntegration } from "@/hooks/useClickUpIntegration";
import { useGitHubConnectionStatus } from "@/hooks/useGitHubConnectionStatus";
import { useGranolaIntegration } from "@/hooks/useGranolaIntegration";
import { useIsRemoteEnvironment } from "@/hooks/useActiveEnvironment";
import {
  isLinearConnected,
  useLinearIntegration,
} from "@/hooks/useLinearIntegration";
import { useClientOwnedFeatureFlag } from "@/lib/remote/feature-flag-authority";
import { HOST_ONLY_AFFORDANCE_HINT } from "@/lib/remote/host-affordances";

import { sectionMeta, type SettingsSectionId } from "./settings-registry";

export interface IntegrationsHubSectionProps {
  onNavigate: (section: SettingsSectionId) => void;
  onWarmSection: (section: SettingsSectionId) => void;
}

interface HubCard {
  section: SettingsSectionId;
  connected: boolean;
  isLoading: boolean;
  /** Short status line in the card footer, e.g. "Authenticated" / "2 keys". */
  status: string;
  unavailable?: boolean;
}

function CardGrid({
  heading,
  cards,
  narrow = false,
  onNavigate,
  onWarmSection,
}: {
  /** Omitted for the lead grid, which sits directly under the page subtitle. */
  heading?: string;
  cards: HubCard[];
  /** External-access grid is capped narrower than the provider grid. */
  narrow?: boolean;
  onNavigate: (section: SettingsSectionId) => void;
  onWarmSection: (section: SettingsSectionId) => void;
}) {
  return (
    <section className="settings-hub__group">
      {heading ? <p className="settings-section__label">{heading}</p> : null}
      <div
        className="settings-hub__grid"
        data-narrow={narrow ? "true" : undefined}
      >
        {cards.map((entry) => {
          const meta = sectionMeta(entry.section);
          const label = meta?.label ?? entry.section;
          return (
            <div
              key={entry.section}
              data-testid={`integration-card-${entry.section}`}
              data-connected={entry.connected}
              className="settings-hub__card"
              onPointerEnter={() => onWarmSection(entry.section)}
            >
              <span className="settings-hub__card-title">{label}</span>
              <p className="settings-hub__card-desc">{meta?.description}</p>
              <div className="settings-hub__card-foot">
                <span
                  className="settings-hub__status"
                  data-connected={entry.connected}
                  data-unavailable={entry.unavailable}
                >
                  <span
                    className="settings-hub__dot"
                    data-connected={entry.connected}
                    aria-hidden="true"
                  />
                  {entry.isLoading ? "Checking…" : entry.status}
                </span>
                <button
                  type="button"
                  className={
                    entry.connected
                      ? "settings-hub__action"
                      : "settings-hub__action settings-hub__action--setup"
                  }
                  // The visible label stays short per the mock; the accessible
                  // name keeps naming the target so the buttons stay distinct.
                  aria-label={
                    entry.connected ? `Manage ${label}` : `Set up ${label}`
                  }
                  disabled={entry.unavailable}
                  onClick={() => onNavigate(entry.section)}
                  onFocus={() => onWarmSection(entry.section)}
                >
                  {entry.connected ? (
                    "Manage"
                  ) : (
                    <>
                      Set up
                      <ArrowRight aria-hidden="true" />
                    </>
                  )}
                </button>
              </div>
            </div>
          );
        })}
      </div>
    </section>
  );
}

export function IntegrationsHubSection({
  onNavigate,
  onWarmSection,
}: IntegrationsHubSectionProps) {
  const atlassian = useAtlassianIntegration();
  const github = useGitHubConnectionStatus();
  const linear = useLinearIntegration();
  const clickup = useClickUpIntegration();
  const granola = useGranolaIntegration();
  const apiKeys = useApiKeys();
  const isRemoteEnvironment = useIsRemoteEnvironment();
  // Client-owned flag — the env-scoped query strips it, so it must come from uiStore.
  const remoteEnvironments = useClientOwnedFeatureFlag("remoteEnvironments");

  const atlassianConnected = isAtlassianConnected(atlassian.settings);
  const githubConnected = github.data?.state === "authenticated";
  const linearConnected = isLinearConnected(linear.settings);
  const keyCount = apiKeys.data?.length ?? 0;

  const providers: HubCard[] = [
    {
      section: "integrations",
      connected: atlassianConnected,
      isLoading: atlassian.isLoading,
      status: isRemoteEnvironment
        ? HOST_ONLY_AFFORDANCE_HINT
        : atlassianConnected
          ? "Connected"
          : "Not configured",
      unavailable: isRemoteEnvironment,
    },
    {
      section: "github",
      connected: githubConnected,
      isLoading: github.isLoading,
      status: isRemoteEnvironment
        ? HOST_ONLY_AFFORDANCE_HINT
        : githubConnected
          ? "Authenticated"
          : "Not authenticated",
      unavailable: isRemoteEnvironment,
    },
    {
      section: "linear",
      connected: linearConnected,
      isLoading: linear.isLoading,
      status: isRemoteEnvironment
        ? HOST_ONLY_AFFORDANCE_HINT
        : linearConnected
          ? "Issue references enabled"
          : "Not configured",
      unavailable: isRemoteEnvironment,
    },
    {
      section: "clickup",
      connected: clickup.connected,
      isLoading: clickup.isLoading,
      status: isRemoteEnvironment
        ? HOST_ONLY_AFFORDANCE_HINT
        : clickup.connected
          ? "Task references enabled"
          : "Not configured",
      unavailable: isRemoteEnvironment,
    },
    {
      section: "granola",
      connected: granola.connected,
      isLoading: granola.isLoading,
      status: isRemoteEnvironment
        ? HOST_ONLY_AFFORDANCE_HINT
        : granola.connected
          ? "Note references enabled"
          : "Not configured",
      unavailable: isRemoteEnvironment,
    },
  ];

  const externalAccess: HubCard[] = [
    {
      section: "api-keys",
      connected: keyCount > 0,
      isLoading: apiKeys.isLoading,
      status: isRemoteEnvironment
        ? HOST_ONLY_AFFORDANCE_HINT
        : keyCount === 1
          ? "1 key"
          : `${keyCount} keys`,
      unavailable: isRemoteEnvironment,
    },
    {
      section: "external-mcp",
      // The external MCP server is configuration, not a connection RalphX can
      // probe from here; the panel owns its own enablement state.
      connected: false,
      isLoading: false,
      status: "Configure server access",
    },
  ];

  // Remote host/client panes ship dark behind `remoteEnvironments`; the hub is a
  // navigation surface, so a hidden leaf must not have a card pointing at it.
  if (remoteEnvironments) {
    externalAccess.push(
      {
        section: "remote-access",
        // Listener state lives behind the pane's own deferred invokes; the hub
        // does not probe it, matching the external-mcp card's display-only rule.
        connected: false,
        isLoading: false,
        status: "Configure host access",
      },
      {
        section: "connections",
        connected: false,
        isLoading: false,
        status: "Manage paired environments",
      },
    );
  }

  return (
    <div className="settings-hub" data-testid="integrations-hub">
      <CardGrid
        cards={providers}
        onNavigate={onNavigate}
        onWarmSection={onWarmSection}
      />
      <CardGrid
        heading="External access"
        cards={externalAccess}
        narrow
        onNavigate={onNavigate}
        onWarmSection={onWarmSection}
      />
    </div>
  );
}

export default IntegrationsHubSection;
