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

import {
  ArrowRight,
  Blocks,
  GitPullRequest,
  KeyRound,
  NotebookPen,
  Server,
  SquareKanban,
  Ticket,
  type LucideIcon,
} from "lucide-react";

import { useApiKeys } from "@/hooks/useApiKeys";
import {
  isAtlassianConnected,
  useAtlassianIntegration,
} from "@/hooks/useAtlassianIntegration";
import { useClickUpIntegration } from "@/hooks/useClickUpIntegration";
import { useGitHubConnectionStatus } from "@/hooks/useGitHubConnectionStatus";
import { useGranolaIntegration } from "@/hooks/useGranolaIntegration";
import {
  isLinearConnected,
  useLinearIntegration,
} from "@/hooks/useLinearIntegration";

import { sectionMeta, type SettingsSectionId } from "./settings-registry";

export interface IntegrationsHubSectionProps {
  onNavigate: (section: SettingsSectionId) => void;
  onWarmSection: (section: SettingsSectionId) => void;
}

interface HubCard {
  section: SettingsSectionId;
  icon: LucideIcon;
  connected: boolean;
  isLoading: boolean;
  /** Short status line under the title, e.g. "Authenticated" / "2 keys". */
  status: string;
}

function CardGrid({
  heading,
  cards,
  onNavigate,
  onWarmSection,
}: {
  heading: string;
  cards: HubCard[];
  onNavigate: (section: SettingsSectionId) => void;
  onWarmSection: (section: SettingsSectionId) => void;
}) {
  return (
    <section className="settings-hub__group">
      <p className="settings-section__label">{heading}</p>
      <div className="settings-hub__grid">
        {cards.map((entry) => {
          const meta = sectionMeta(entry.section);
          const label = meta?.label ?? entry.section;
          const Icon = entry.icon;
          return (
            <div
              key={entry.section}
              data-testid={`integration-card-${entry.section}`}
              data-connected={entry.connected}
              className="settings-hub__card"
              onPointerEnter={() => onWarmSection(entry.section)}
            >
              <div className="settings-hub__card-head">
                <span className="settings-hub__card-icon">
                  <Icon aria-hidden="true" />
                </span>
                <span className="settings-hub__card-title">{label}</span>
                <span
                  className="settings-hub__dot"
                  data-connected={entry.connected}
                  aria-hidden="true"
                />
              </div>
              <p className="settings-hub__card-desc">{meta?.description}</p>
              <p className="settings-hub__card-status">
                {entry.isLoading ? "Checking…" : entry.status}
              </p>
              <button
                type="button"
                className={
                  entry.connected ? "settings-btn-ghost" : "settings-btn-primary"
                }
                onClick={() => onNavigate(entry.section)}
                onFocus={() => onWarmSection(entry.section)}
              >
                {entry.connected ? `Manage ${label}` : `Set up ${label}`}
                {entry.connected ? null : <ArrowRight aria-hidden="true" />}
              </button>
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

  const atlassianConnected = isAtlassianConnected(atlassian.settings);
  const githubConnected = github.data?.state === "authenticated";
  const linearConnected = isLinearConnected(linear.settings);
  const keyCount = apiKeys.data?.length ?? 0;

  const providers: HubCard[] = [
    {
      section: "integrations",
      icon: Blocks,
      connected: atlassianConnected,
      isLoading: atlassian.isLoading,
      status: atlassianConnected ? "Connected" : "Not configured",
    },
    {
      section: "github",
      icon: GitPullRequest,
      connected: githubConnected,
      isLoading: github.isLoading,
      status: githubConnected ? "Authenticated" : "Not authenticated",
    },
    {
      section: "linear",
      icon: SquareKanban,
      connected: linearConnected,
      isLoading: linear.isLoading,
      status: linearConnected ? "Issue references enabled" : "Not configured",
    },
    {
      section: "clickup",
      icon: Ticket,
      connected: clickup.connected,
      isLoading: clickup.isLoading,
      status: clickup.connected ? "Task references enabled" : "Not configured",
    },
    {
      section: "granola",
      icon: NotebookPen,
      connected: granola.connected,
      isLoading: granola.isLoading,
      status: granola.connected ? "Note references enabled" : "Not configured",
    },
  ];

  const externalAccess: HubCard[] = [
    {
      section: "api-keys",
      icon: KeyRound,
      connected: keyCount > 0,
      isLoading: apiKeys.isLoading,
      status: keyCount === 1 ? "1 key" : `${keyCount} keys`,
    },
    {
      section: "external-mcp",
      icon: Server,
      // The external MCP server is configuration, not a connection RalphX can
      // probe from here; the panel owns its own enablement state.
      connected: false,
      isLoading: false,
      status: "Configure server access",
    },
  ];

  return (
    <div className="settings-hub" data-testid="integrations-hub">
      <CardGrid
        heading="Connected tools"
        cards={providers}
        onNavigate={onNavigate}
        onWarmSection={onWarmSection}
      />
      <CardGrid
        heading="External access"
        cards={externalAccess}
        onNavigate={onNavigate}
        onWarmSection={onWarmSection}
      />
    </div>
  );
}

export default IntegrationsHubSection;
