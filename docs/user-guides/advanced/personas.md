# Customizing your agents' voice with Personas

Create a reusable voice or standing context for a Project Agent conversation. At the end, you will have an approved persona bound to a conversation, ready for its future sends.

**Before you start:** [Finding your way around](../02-tour-of-the-app.md)

## Enable Personas and choose a scope

1. Open Settings → **Agents** → **Personas**, then turn on **Enable agent personas**.

   Personas are off by default.

   The in-app description calls this feature **Experimental**.

   A persona is a prompt-only behavior profile: "Craft reusable voices for project agents."

   It is not a model, a skill, a project setting, or a separate agent.

2. Choose the scope you need before creating a persona.

   A global persona is available across all projects.

   A project persona is available only for that one project.

   In either case, its binding is per-conversation, not a project-wide default.

## Build a persona with an agent

1. In the Personas management list, click **Build with Agent**.

   Use **Scope:** to narrow the list by global or project personas.

   Use the **All**, **Drafts**, and **Archived** tabs to find the state you need.

   Click **+ New** instead when you want to write a draft yourself.

2. In **Build persona with agent**, choose where the persona is available.

   Choose **Global — all projects** for a reusable voice across projects.

   Choose **Project:** and select one project for project-only context.

   Click **Start build**.

3. Keep the **Persona** start mode selected and describe the voice, working style, and standing context you want.

   RalphX opens a normal conversation for the builder and saves its result to the conversation-bound draft.

   Build one persona per Persona conversation; start another conversation for a separate voice.

4. Open the **Persona** artifact tab when the builder has produced a draft.

   Review the saved persona there, then click **Approve Persona** to make an agent-built persona available for use.

   This is the approval path for agent-built personas.

## Write and activate a draft yourself

1. Click **+ New** in the Personas management list.

2. Complete the **Scope**, **Name**, **Slug**, **Description**, and **Instructions** fields.

   Use a global scope for all projects, or choose one project for a project-only persona.

   Give the instructions the specific voice and standing context you want injected into later conversation sends.

3. Click **Save** to create the hand-written draft.

   The editor has no approval control.

4. Return to the list, open **Drafts**, and click **Activate** on that draft.

   **Activate** promotes the hand-written draft.

   Do not look for **Approve Persona** in the editor; it belongs on the Persona artifact tab for agent-built personas.

## Use a persona in a project conversation

1. Start or open an eligible project Agent conversation.

2. Open the conversation persona chip and choose an available persona.

   The picker offers global personas and personas scoped to the current project.

   Choose **Create persona for this project** to open a project-locked Persona builder from this conversation.

3. Use **View injected prompt** when you want to inspect the prompt that RalphX will use.

   RalphX injects the active persona only into future sends for this conversation.

4. Choose **Remove persona** to clear the binding from the conversation.

   Switching or removing a persona stops an active agent before the new binding takes effect.

   Conversation history remains available, and the next send uses the new binding.

## Refine or retire a persona

1. Click **Refine with Agent** on an approved persona when you want the builder to revise it.

   Refinement keeps the source persona's scope.

   Review the resulting draft in the Persona artifact tab and use **Approve Persona** when it is ready.

2. Archive an active persona when you no longer want it available.

   Confirm **Archive persona** in the confirmation dialog.

   Archiving clears active conversation bindings for that persona.

3. Delete a draft you no longer need.

   Confirm **Delete draft** in the confirmation dialog.

## Know where Personas do not apply

1. Use Personas only for eligible Project Agent conversations.

   Personas do not apply to teammates or delegated agents.

   They also do not apply to pipeline workers, reviewers, or mergers.

   Ideation, Task, and Merge conversations remain persona-less.

   External MCP sends remain persona-less too.

2. Treat a missing persona effect in those places as expected behavior.

   Persona controls can remain visible but disabled when the role cannot receive prompt injection.

## What you have now

You have an approved persona with either global or project scope, and you know that its use is bound to one project conversation at a time. You can build it with an agent through the Persona artifact approval path, or write it manually through the Save-then-Activate path.

## Next

- [Implementing a feature with RalphX](../workflows/implementing-a-feature.md)
