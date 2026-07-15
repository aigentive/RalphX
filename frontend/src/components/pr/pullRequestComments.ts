import type { PullRequestIssueComment } from "@/api/github";

export interface PartitionedIssueComments {
  /** Human-authored comments (not a bot, not Codecov). */
  human: PullRequestIssueComment[];
  /** Count of automated (bot / Codecov) comments that were filtered out. */
  hiddenBotCount: number;
}

/** A comment is automated when it is flagged as a bot or as a Codecov report. */
export function isAutomatedComment(comment: PullRequestIssueComment): boolean {
  return comment.isBot || comment.isCodecov;
}

/**
 * Split issue comments into the human comments worth showing and a count of the
 * automated (bot/Codecov) comments hidden from the PR tab.
 */
export function partitionIssueComments(
  comments: PullRequestIssueComment[],
): PartitionedIssueComments {
  const human: PullRequestIssueComment[] = [];
  let hiddenBotCount = 0;
  for (const comment of comments) {
    if (isAutomatedComment(comment)) {
      hiddenBotCount += 1;
    } else {
      human.push(comment);
    }
  }
  return { human, hiddenBotCount };
}
