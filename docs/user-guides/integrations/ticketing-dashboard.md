# Working with tickets

Bring Jira, Linear, and ClickUp tickets into one RalphX view so you can find the work that matters, set up its status presentation, and begin an agent conversation with the ticket attached. At the end, you will have a focused ticket view and a clear handoff from triage to RalphX work.

**Before you start:** connect at least one ticketing provider with [Jira and Confluence](connect-jira-and-confluence.md), [Linear](connect-linear.md), or [ClickUp](connect-clickup.md).

## Open the ticketing dashboard

1. Click **Ticketing** in the left navigation.

   This item appears only when RalphX has at least one enabled, connected ticketing provider.

   If it is absent, finish connecting a provider, then return to the navigation rail.

2. Choose a provider when RalphX shows more than one.

   The dashboard places connected Jira, Linear, and ClickUp tickets in the same view.

   Select the provider tab whose tickets you want to triage.

   When only one provider is connected, RalphX shows its name instead of a switcher.

3. Choose the scope that supplies the tickets you need.

   For Jira, choose a **Project** before the dashboard loads its project tickets and statuses.

   For Linear, use **Project** to narrow the tickets shown; status management still uses Linear's global workflow states.

   For ClickUp, choose a **Space** before tickets load.

   After choosing a ClickUp Space, use its Folder or List in the location rail when you need a narrower workflow.

4. Use **List** for a grouped ticket list or **Kanban** for status columns.

   Choose the view that makes the current triage decision easiest to see.

   The available columns use the selected provider and scope.

5. Narrow the view with the Status, Assignee, Sprint, and text filters when they appear.

   Use **Reset** to clear the active filters and return to the selected provider scope.

   Click **Refresh tickets** after you expect provider-side changes and need the dashboard to fetch them again.

## Inspect and triage a ticket

1. Open a ticket from the list or board.

   Read its description, comments, assignee, labels, and linked RalphX work in the detail sheet.

   Click **Open in provider** when you need the provider's full page in your browser.

2. Change a ticket only when the provider allows that action.

   Use the Status selector to move the ticket through an available provider workflow transition.

   Use **Assign to me** for an unassigned ticket when the connected provider supports assignment.

   Add a comment or update labels only when those controls are available for the provider and your permissions.

3. Treat unavailable actions as provider or permission limits.

   RalphX keeps browsing available where it can, but write actions depend on the selected provider's capabilities.

   Reconnect the provider with the required ticket permissions if the dashboard reports limited access.

## Set up the status presentation

1. Select the provider and scope you want to configure, then click **Statuses**.

   The dialog identifies the provider and the active scope, so check them before changing presentation.

   Jira status presentation is scoped to the selected project.

   Linear status presentation is scoped to all Linear workflow states.

   ClickUp status presentation is scoped to the selected Space, Folder, or List.

2. Let RalphX load the selected scope, then click **Sync** whenever you need to refresh it from the provider.

   Sync refreshes provider-owned status information, including the name, category, color, order, and whether the provider still supplies the status.

   Statuses that disappear from a later provider refresh stay in RalphX and are marked stale rather than being removed immediately.

3. Use the move controls to set the order RalphX uses for that scope.

   This changes the order in the ticket list groups, Kanban columns, and status filter for the selected scope.

   It does not change the provider's workflow configuration.

4. Choose a color to override the provider color when the RalphX view needs a clearer visual grouping.

   Click **Reset** to return that status to its provider color.

   Color overrides are local to RalphX and do not update Jira, Linear, or ClickUp.

5. Turn off a status's visible switch when you want to hide it from RalphX ticket columns.

   The status remains available and unchanged in the ticketing provider.

   The dialog shows the current ticket count beside each status to help you decide what to hide.

## Start RalphX work from a ticket

1. Open the ticket you want to work on and click **Start conversation** in the RalphX Work panel.

   RalphX opens the **Start Conversation** dialog with that ticket ready to attach as a reference.

   Choose the target **Project** if it is not already selected.

2. Click **Open composer**.

   RalphX opens an Agent composer for the selected project with the ticket attached.

   Describe the implementation or investigation you want; the ticket remains part of the conversation context.

3. Use **Bind existing conversation** instead when Jira or Linear work already has a RalphX conversation.

   Select the conversation to associate it with the ticket, then use the RalphX Work panel to revisit the linked work.

   ClickUp currently supports starting a new conversation from a ticket, but does not expose the existing-conversation binding control.

## What you have now

You can browse connected Jira, Linear, and ClickUp tickets from the Ticketing dashboard, focus them by provider and scope, and tailor their status presentation locally in RalphX. You can also turn a selected ticket into a new agent conversation with the ticket attached, or link existing Jira and Linear conversations to their tickets.

## Next

- [Planning a feature with RalphX](../workflows/planning-a-feature.md)
