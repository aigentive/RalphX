# Tracked delivery with Tasks

Use Tasks when an approved plan needs a visible board, dependencies, and delivery that RalphX carries task by task. You will turn the plan into tracked proposals, follow their work in one artifact, and understand what happens after you approve a completed task.

**Before you start:** [Implementing a feature with RalphX](../workflows/implementing-a-feature.md)

## Enable Tasks

1. Open Settings → **Automation** → **Tasks**.

   Tasks is off by default.

   This setting applies before you choose the tracked delivery path.

2. Turn on **Enable Tasks**.

   Leave it enabled while task-managed work is in progress.

   The direct implementation path remains available when Tasks is off.

## Create tracked proposals from an approved plan

1. Return to the approved plan in its **Plan** artifact.

   Check that the plan still describes the delivery you want to track.

2. Click **Create Proposals**.

   RalphX creates tracked work from the approved plan and takes you to the Tasks artifact.

   Use this path when the work benefits from separate, visible pieces and their dependencies.

3. Choose the direct path instead for a small, linear change.

   **Implement Directly**, covered in the prerequisite guide, starts one Agent workspace for the whole plan.

   **Create Proposals** keeps the approved plan as a set of tracked tasks rather than replacing the direct path.

## Follow the work in Tasks

1. Open the **Tasks** artifact in the Agent workspace.

   Tasks is the delivery surface for this plan.

   **Kanban** and **Graph** are view toggles inside this artifact, not top-level destinations.

2. Select **Kanban** when you want to follow work as a board.

   Use the board to see the tasks for the selected plan and open the item that needs attention.

3. Select **Graph** when you want to understand dependencies.

   Use it to see which work must finish before related work can proceed.

4. Set the Active Plan when you need to change the work in view.

   Active Plan filtering scopes the Kanban and Graph views to tasks that belong to that plan.

   Select the plan that matches the approved delivery you are following before judging progress or dependencies.

   For the persisted selection and filtering design, see the [Active Plan architecture](../../architecture/active-plan-api.md).

## Start or continue a task

1. Open a task from **Kanban** or **Graph** to read its detail.

   Use the action offered for the task's current situation.

2. Click **Start** for work that is ready to begin.

   RalphX starts that task's delivery work.

3. Click **Resume** when a paused task should continue.

   Resume the same task after you have resolved the reason it was paused.

4. Click **Restart** when failed, stopped, or cancelled work needs another attempt.

   Add a correction when needed so the next attempt has the context it was missing.

5. Click **Start Ideation** for a backlog task that needs more planning before implementation.

   Use this when the task needs its own planning conversation rather than immediate delivery.

## Approve completed work

1. Review a completed task and approve it when it meets the plan's intent.

   Approval hands the task to RalphX's merge pipeline automatically.

2. Follow the resulting delivery status in the task detail.

   RalphX prepares the task's branch, keeps it current with its target, runs the configured checks, and merges the accepted change when those steps succeed.

   If RalphX needs attention, return to the task detail and address the reported issue before continuing.

3. Use the architecture reference when you need implementation-level merge behavior.

   The [merge pipeline architecture](../../architecture/merge-pipeline.md) documents states, transitions, and recovery details that are intentionally outside this guide.

## What you have now

You have enabled the opt-in Tasks feature and converted an approved plan into tracked delivery work. The Tasks artifact gives you a board or dependency view scoped by Active Plan, and task approval hands completed work to the merge pipeline.

## Next

- [Reviewing your own work with RalphX](../workflows/reviewing-your-own-work.md)
