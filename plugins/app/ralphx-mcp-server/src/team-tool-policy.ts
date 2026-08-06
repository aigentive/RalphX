const TEAM_TOOL_NAMES = new Set([
  "team_add_member",
  "team_assign",
  "team_list",
  "team_stop_member",
  "team_send_message",
  "team_roster",
  "team_status",
]);

/** Team coordinator tools exist only inside the backend-owned RX Team mode. */
export function applyTeamToolPolicy(tools: string[]): string[] {
  if (process.env.RALPHX_COORDINATION_MODE === "rx_native_team") {
    return tools;
  }
  return tools.filter((tool) => !TEAM_TOOL_NAMES.has(tool));
}
