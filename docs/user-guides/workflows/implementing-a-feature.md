# Implementing a feature with RalphX

Use the direct delivery path to turn an approved plan bundle into a branch with the requested change. You will finish in an **Agent** workspace with the work ready for RalphX's local review gate.

**Before you start:** [Planning a feature with RalphX](planning-a-feature.md)

## Start direct implementation

1. Return to the approved plan bundle in the **Plan** artifact tab.
2. Confirm that the Plan Overview and Implementation Blueprint still describe the work you want done.
3. Click **Implement Directly** to begin the default delivery path.
4. Wait for the conversation to switch into the **Agent** workspace.
5. Keep the approved artifacts open as the source of intent for the branch.
6. Return to planning instead if you need to change to **Create Proposals** and tracked delivery.

## Follow the implementation

1. Read progress updates as RalphX works through the plan.

![The RalphX Agent workspace during implementation](../../../assets/public/guides/agent-workspace-implementing.png)

2. Answer questions that need a product decision or missing context.
3. Give a concrete correction if progress reveals a misunderstanding.
4. Reopen the **Plan** artifact tab to compare progress with the approved intent.
5. Open the **Issues** artifact tab when RalphX identifies a blocker, then resolve it before asking RalphX to finish.
6. Ask RalphX to explain a change when you need to decide whether it still fits the plan.
7. Keep unrelated requests out of this branch.
8. Return to planning if the feature itself must be redefined.

## Hand off to local review

1. Read RalphX's implementation summary against the approved plan bundle.
2. Check that the summary covers the requested behavior and validation.
3. Resolve any unanswered product question.
4. Open the **Review** artifact tab from the **Agent** workspace.
5. Treat Workspace Review as the local quality gate for the workspace delta, not as a GitHub review.
6. Use review findings to request a fix or decide that the branch is ready to publish.
7. Open **Commit & Publish** only for the later branch hand-off; implementation completion and local review do not publish the branch.

## What you have now

You have an approved plan implemented on a branch in an **Agent** workspace. The direct path produced a workspace delta and a **Commit & Publish** hand-off surface, but the change still needs the local Workspace Review gate before publication.

## Next

- [Reviewing your own work with RalphX](reviewing-your-own-work.md)
