# Planning a feature with RalphX

Use a planning conversation to turn an idea into an implementation-ready plan bundle before code changes begin. You will finish with a verified, approved Plan Overview and Implementation Blueprint, then choose how to deliver the work.

**Before you start:** [Finding your way around](../02-tour-of-the-app.md)

## Start the planning conversation

1. Open the project you want to change and start a conversation in **Plan**.
2. Describe the outcome, who it helps, relevant existing behavior, and constraints.
3. State what must not change, then send the request.

## Clarify the scope

1. Answer each clarifying question with the needed decision or fact.
2. Correct RalphX if it misunderstands the goal or includes work outside the feature.
3. Name compatibility, performance, security, or release constraints that must shape the plan.
4. Give an example when two interpretations would lead to different behavior.

## Read and refine the plan bundle

1. Open the **Plan** artifact tab.
2. Read the Plan Overview for the outcome, scope, and acceptance criteria.
3. Read the Implementation Blueprint for the proposed implementation and validation steps.

![The RalphX Plan Overview and Implementation Blueprint](../../../assets/public/guides/plan-bundle.png)

4. Return to the conversation to correct missing behavior, remove out-of-scope work, or add a needed acceptance criterion.
5. Re-read both artifacts after a material change.

## Verify and approve the plan

1. Click **Verify Plan** to check that the bundle is safe and complete enough to execute.
2. Review the verification feedback and resolve any meaningful gap in the conversation.
3. Click **Verify Plan** again after a material revision.
4. Read the final Plan Overview and Implementation Blueprint.
5. Click **Approve Plan** when you are ready to authorize delivery; verification does not approve the plan for you.

## Choose a delivery path

1. Read the recommendation shown after approval.
2. Choose **Implement Directly** for the default path: a small, linear change implemented and reviewed as one coherent branch.
3. Choose **Create Proposals** when you need tracked tasks or separate review checkpoints.
4. If **Create Proposals** is unavailable, open Settings → **Automation** → **Tasks** and turn on **Enable Tasks**; Tasks is off by default.
5. Return to the approved plan and select the path you want. The recommendation is guidance, not an automatic choice.

## What you have now

You have a verified, approved Plan Overview and Implementation Blueprint, plus an explicit delivery choice. Direct implementation is the normal path for small, linear work; proposals are the tracked-delivery path after you opt into Tasks.

## Next

- [Implementing a feature with RalphX](implementing-a-feature.md)
