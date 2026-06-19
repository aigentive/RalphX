import { useMemo } from "react";
import { useQuery } from "@tanstack/react-query";

import { atlassianApi } from "@/api/atlassian";
import { linearApi } from "@/api/linear";

export type AgentIssueTab = "jira" | "linear";

export function useAgentIssueTabs(enabled: boolean): readonly AgentIssueTab[] {
  const atlassianSettingsQuery = useQuery({
    queryKey: ["atlassian", "settings"],
    queryFn: () => atlassianApi.getSettings(),
    staleTime: 30_000,
    enabled,
  });
  const linearSettingsQuery = useQuery({
    queryKey: ["linear", "settings"],
    queryFn: () => linearApi.getSettings(),
    staleTime: 30_000,
    enabled,
  });

  return useMemo(() => {
    if (!enabled) {
      return [];
    }
    const tabs: AgentIssueTab[] = [];
    if (
      atlassianSettingsQuery.data?.enabled &&
      atlassianSettingsQuery.data?.jiraAvailable
    ) {
      tabs.push("jira");
    }
    if (
      linearSettingsQuery.data?.enabled &&
      linearSettingsQuery.data?.issueSearchAvailable
    ) {
      tabs.push("linear");
    }
    return tabs;
  }, [
    atlassianSettingsQuery.data?.enabled,
    atlassianSettingsQuery.data?.jiraAvailable,
    enabled,
    linearSettingsQuery.data?.enabled,
    linearSettingsQuery.data?.issueSearchAvailable,
  ]);
}
