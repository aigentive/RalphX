# Delivering large projects with automated, supervised goals

Use an automation when a project goal is too large for one plan-and-implement cycle. RalphX breaks that project-scoped goal into ordered goal items and carries them forward one at a time: each run is a normal agent conversation that plans, implements, and publishes one pull request. You choose how closely to supervise each stage.

**Before you start:** [Implementing a feature](../workflows/implementing-a-feature.md)

## Create the goal

1. Open **Automations** from the navigation rail, then click **New automation**.

   The page is feature-flag gated, but the shipped `automations_page` default is on.

   If **Automations** is absent, ask the person who manages your RalphX configuration to enable that feature flag.

2. Select the project and describe the outcome, scope, and constraints in the outline.

   The setup conversation turns that outline into a complete goal and an ordered goal-item list.

   Review the proposed scope and ask follow-up questions in that conversation before you activate the automation.

   An automation has one open run at a time, so the ordered items advance in sequence rather than competing with one another.

3. Open the resulting automation detail and use **Overview** to inspect its summary.

   Review the configuration summary and the **Spec & inputs** card to inspect the saved configuration, generated specification, and inputs that informed it.

   Open **Runs** to see the current goal item and the history of prior runs.

## Choose the supervision level

1. From **Automations**, click **New automation** and finish the outline to open its setup conversation.

   In that conversation, open the **Automation** artifact tab, then find the **Settings** section.

   This is the exact path to the three supervision controls. They are not on the automation detail page.

### Plan approval

1. Set **Plan approval** to **Manual** when you want to approve every run plan yourself.

   Manual is the default.

   Every run plans before it implements, then waits at the plan gate for the current plan to be approved.

2. Choose **Automatic (judge)** only when you want RalphX's plan judge to assess each plan against the goal item.

   A judge failure pauses the automation for your review; it never treats the failed check as approval.

### PR merge

1. Set **PR merge** to **Manual** when you want to merge each pull request yourself.

   Manual is the default.

2. Choose **Automatic** only when GitHub should arm its native squash auto-merge for the run pull request after publication.

   Keep GitHub access connected and use manual merge if you need a deliberate merge decision for each item.

### Deep plan verification

1. Leave **Deep plan verification** off unless you want the extra adversarial plan-verification loop.

   It is off by default.

   Its result is advisory input to the plan judge: a verification failure becomes unavailable verification and does not block the plan gate.

## Run, review, and revise each item

1. Click **Approve** when the setup conversation and automation configuration are ready to activate.

   Use **Run now** when an active automation is ready to begin its next eligible run.

   The run opens as an ordinary agent conversation in plan mode before implementation begins.

2. When a run is awaiting plan approval, open that run's agent conversation from **Runs** and review its plan artifact.

   Click **Approve Plan** in the run's agent conversation to let that run implement the approved plan.

   Do not use the **Run plan** dialog to approve a plan. It displays the plan only and has no approval control.

3. Send a message in a parked run's conversation when you want the agent to revise the plan before approval.

   The conversation remains editable while it awaits plan approval.

   Review the replacement plan, then click **Approve Plan** only when the new version is ready.

4. Use **Pause automation** to pause active work without discarding the current run.

   Use **Resume automation** to continue a paused automation.

   Use **Restart automation** only after an automation reaches **Stopped** and you want a fresh pending run from the durable history.

5. Click **Cancel automation** when you want to end the current automation attempt.

   There is no “Stop automation” control. **Stopped** is a resulting status; cancellation is the terminal action in the interface.

6. In **Runs**, use **Resume** for an eligible latest run, **Delete run** to remove a run, and **Retry plan judge** when a parked run's plan judge failed.

   Open a run conversation for its detailed plan, implementation, pull request, and normal agent usage information.

## Track progress and cost

1. Return to **Overview** to follow the goal-item progress and the automation's current state.

   The configuration summary shows usage totals across the automation's run conversations: input tokens, output tokens, cache creation and read tokens, and estimated cost when RalphX recorded it.

2. Use the linked run conversation when you need the provider, model, runtime, and usage detail for one particular pull request.

   RalphX continues only after each item reaches its normal run outcome, preserving one open run at a time.

## Use the Tasks bridge only when you need it

1. Use the ideation bridge only with the off-by-default Tasks feature; it turns a verified automation plan into task proposals and dependencies, as covered in [Tracked delivery with Tasks](task-managed-delivery.md).

## What you have now

You have a project-scoped, ordered goal that RalphX can deliver across multiple pull requests, one supervised agent run at a time. You know where to inspect the goal, revise a parked plan, and choose whether people, a plan judge, or GitHub should carry each approval step.

## Next

- [Tracked delivery with Tasks](task-managed-delivery.md)
