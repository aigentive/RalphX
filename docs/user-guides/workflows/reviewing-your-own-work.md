# Reviewing your own work with RalphX

Use Workspace Review to check the local changes RalphX made in your workspace before publishing the branch. You will finish with local findings resolved or consciously overridden, then publish the reviewed branch when you are ready.

**Before you start:** [Implementing a feature with RalphX](implementing-a-feature.md)

## Open the local review gate

1. Open the **Review** artifact tab in the **Agent** workspace after implementation is stable.
2. Use Workspace Review to inspect the local workspace delta against its base before you publish.
3. Treat this as a local quality gate, not a GitHub review.

## Run and read the review

1. Click **Run review** to start a local reviewer pass.

![The RalphX Review tab showing a blocking Workspace Review result and its Overview](../../../assets/public/guides/review-run.png)

2. Use **Retry review** if that attempt needs another run.
3. Open **Overview** for the result and **Requested Changes** for blocking or requested fixes.

![The RalphX Requested Changes artifact tab](../../../assets/public/guides/review-requested-changes.png)

4. Compare each finding with the branch changes and approved plan.
5. Click **View transcript** when you need the reviewer's reasoning.

## Resolve findings

1. Click **Fix Issues** when you want RalphX to address actionable findings in the workspace.
2. Review the fixes against the approved intent.
3. Click **Update review** after the workspace changes and needs a refreshed assessment.
4. Click **Run again** after the fixes settle and you want another full reviewer pass.
5. Repeat until the current local result is acceptable; do not rely on a result from before a material delta change.

## Use an override deliberately

1. Read every blocking finding before considering an override.
2. Click **Approve anyway** only when you consciously accept the remaining blocking findings.
3. Record the reason in the conversation when a future reviewer needs that context.
4. Re-run review instead when the finding can be addressed safely.

## Publish the reviewed branch

1. Confirm that the local review passed or that you consciously used **Approve anyway**.
2. Open the **Commit & Publish** artifact tab and review the branch hand-off information.

![The RalphX Commit & Publish tab showing the workspace changes and publish action](../../../assets/public/guides/commit-publish-tab.png)

3. Publish only when the local review outcome and branch contents are acceptable to you.
4. Use the separate Review PR workflow for a GitHub pull-request review.

## What you have now

You have reviewed the local workspace delta, resolved its findings, or consciously overridden them with **Approve anyway**. The reviewed branch can now be published through **Commit & Publish**; this workflow completed a local quality gate, not a GitHub PR review.

## Next

- [Reviewing a pull request with RalphX](reviewing-a-pull-request.md)
