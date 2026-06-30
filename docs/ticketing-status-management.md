# Ticketing Status Management

RalphX manages ticketing status presentation from the main Ticketing view.

## Where It Is

1. Open `Ticketing`.
2. Select the provider.
3. Select the status scope:
   - Jira: select a project.
   - ClickUp: select a Space. Folder/list drill-downs still use the parent Space status set.
   - Linear: current UI manages the global Linear workflow-state scope.
4. Click `Statuses` in the Ticketing header.

## Sync Contract

- Opening `Statuses` syncs the selected scope from the provider API.
- `Sync` runs the same provider API refresh again.
- Provider-owned fields are refreshed from Jira, Linear, or ClickUp: status id, name, category, provider color, provider order, terminal/stale state.
- RalphX-owned fields are preserved across sync: display order, color override, and visibility.
- Provider statuses missing from a later sync are kept and marked stale instead of being deleted immediately.
- Status presentation changes are local to RalphX. Reordering, color overrides, color reset, and visibility changes do not mutate Jira, Linear, or ClickUp configuration.
- Provider write APIs are used only by ticket actions such as moving a ticket, assigning, commenting, or changing labels.

## Custom Presentation

- Use the arrow buttons to reorder statuses for the selected scope.
- Use the color picker to set a RalphX override color.
- Use `Reset` to fall back to the provider color.
- Use the visibility switch to hide a synced status from RalphX ticket columns. The status remains unchanged in the ticketing provider.
- The modal shows the current ticket count next to each status name.
- The list grouping, kanban columns, and status filter use the same RalphX-resolved order and color.

## Provider Notes

- Jira project statuses do not expose a reliable board order through the project status API, so RalphX seeds order by category and then preserves the user order.
- Linear workflow states expose provider colors and order; RalphX preserves local presentation after the first sync.
- ClickUp Space statuses expose colors and `orderindex`; RalphX uses the Space status name as the grouping identity because ClickUp tasks expose status by name.
