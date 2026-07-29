# Finding your way around

Learn where RalphX keeps conversations, artifacts, navigation, start modes, and settings so you can begin work without guessing. At the end, you will know which workspace to open, which mode to choose, and where to adjust the app.

**Before you start:** [Installing RalphX and running it for the first time](01-install-and-first-run.md)

## Start in the Agents workspace

1. Click **Agents** in the left navigation.

2. Use this workspace to create and continue RalphX conversations.

3. Read the conversation area on one side of the workspace.

4. Use the artifact pane on the other side to inspect the material RalphX produces for the active conversation.

![The RalphX Agents workspace with conversation and artifact pane](../../assets/public/guides/agents-workspace-overview.png)

5. Keep the conversation and its artifacts together; they describe the same piece of work.

6. Remember that agents work in isolated git worktrees and never touch your working directory.

7. Open the active conversation before selecting a start mode or reviewing an artifact.

## Use the navigation rail

1. Click **Agents** to work with conversations, plans, and agent-driven development.

2. Click **GitHub** to use the connected GitHub view.

3. Click **Insights** to view RalphX insights.

4. Look for **Ticketing** only when you have a valid ticketing integration; a fresh install may not show it.

5. Use **Ticketing** for the connected ticketing dashboard when it is available.

6. Look for **Granola** only when a Granola dashboard integration is connected; a fresh install may not show it.

7. Use **Granola** for the connected Granola dashboard when it is available.

8. Look for **Automations** when the automation feature is available in your build.

9. Use **Automations** for the automation view when it is present.

10. Click **Report Issue** if you need to report a problem with RalphX.

11. Click **Settings** at the bottom of the rail to change app configuration.

12. The navigation rail changes with connected integrations, so absent integration views are expected on a fresh install.

13. Return to **Agents** whenever you want to resume a conversation.

## Choose a start mode

1. Start a conversation from **Agents**.

2. Choose **Plan** when you want to draft and refine a plan before execution.

![The RalphX start-mode picker with mode descriptions](../../assets/public/guides/start-mode-picker.png)

3. Use **Plan** for work that needs agreement on approach before code changes begin.

4. Choose **Agent** when you want to build, change, and review code in a branch.

5. Use **Agent** when you are ready for code work in an isolated worktree.

6. Choose **Review PR** when you want to review a remote GitHub PR through its local checkout and propose a user-approved GitHub review.

7. Use **Review PR** only with a project that can supply the local checkout.

8. Choose **Ask** when you want read-only questions about the project.

9. Use **Ask** when you need understanding without requesting code changes.

10. Choose **Automation** when you want to create and run a recurring agent workflow.

11. Its runs are sequential over a goal's ordered items and are not time-scheduled.

12. Choose **Persona** when you want to build or refine a reusable agent persona.

13. Choose **Plan**, **Agent**, **Review PR**, or **Ask** first; they are the primary modes surfaced by default.

14. Create a project before using **Plan**, **Agent**, **Review PR**, or **Automation**.

15. Use **Ask** or **Persona** when you do not need a project for the conversation.

16. Match the mode to your immediate goal instead of combining a question, plan, and code change in one start.

17. Start a new conversation when the work has a different goal or project context.

## Read the artifact pane

1. Keep the active conversation open in **Agents**.

2. Open **Issues** to inspect the issues artifact for that conversation.

3. Open **Plan** to inspect the plan artifact.

4. Open **Tasks** only when the Tasks feature is enabled.

5. Tasks is off by default; turn on **Enable Tasks** in Settings → **Planning** when you need it.

6. Use **Tasks** for tracked delivery; this is covered in a later guide.

7. Use **Graph** inside the **Tasks** artifact to view its dependency graph.

8. Use **Kanban** inside the **Tasks** artifact to view its board.

9. Treat **Graph** and **Kanban** as toggles within **Tasks**, not as top-level destinations.

10. Open **Review** when a review artifact is available for the conversation.

11. Open **Commit & Publish** when the conversation is ready for its commit and publishing workflow.

12. Use the artifact that matches the current conversation stage instead of switching away from the conversation.

13. Wait for an artifact to appear when RalphX is still producing it.

14. Keep artifact review focused on the active conversation so you do not confuse work from separate conversations.

## Find the right settings

1. Click **Settings** at the bottom of the left navigation rail.

2. Start in **Providers**, the default settings section, when you need to confirm a provider.

![The RalphX Settings Providers section](../../assets/public/guides/settings-overview.png)

3. Use the Harness group for **Providers**, **Models**, **Agents**, and **MCP**.

4. Use the Workspace group for **Repository** and **Setup & Validation**.

5. Use the General group for **Tasks**, **Planning**, **Workspace**, **Capacity**, **Personas**, and **Capabilities**.

6. Use the Integrations group for **Atlassian**, **GitHub**, **Linear**, **ClickUp**, and **Granola**.

7. Use the External Access group for **API Keys** and **External MCP**.

8. Use the Preferences group for **Updates**, **Accessibility**, and **Notifications**.

9. Open Settings → **Integrations** → **GitHub** when you need the GitHub connection settings.

10. Change only the setting that supports your current workflow, then return to **Agents**.

11. Revisit **Providers** if a configured agent runtime is no longer available.

12. Use **Planning** only when you want to adjust planning-related behavior, including the off-by-default Tasks feature.

13. Close Settings after confirming the change you intended to make.

## What you have now

You know how to open the Agents workspace, choose an appropriate start mode, and read its conversation artifacts. You also know that RalphX runs agent work in isolated git worktrees and leaves your working directory untouched. Settings and integration-dependent navigation are available when you need to adjust the app.

## Next

- [Planning a feature with RalphX](workflows/planning-a-feature.md)
