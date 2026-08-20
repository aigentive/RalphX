// Plain-language lookup for typed clone failure codes. No git commands, ref
// syntax, or raw backend codes are ever rendered on screen.

const FAILURE_MESSAGES: Record<string, string> = {
  CLONE_URL_INVALID:
    "That doesn't look like a repository URL. Use an HTTPS or SSH git URL, or an owner/repo shorthand.",
  CLONE_DEST_INVALID: "That destination isn't valid. Choose a different parent folder or name.",
  CLONE_DEST_NOT_EMPTY: "That folder already has files in it. Choose an empty folder or a different name.",
  CLONE_AUTH_FAILED: "RalphX couldn't authenticate with this repository.",
  CLONE_NOT_FOUND: "That repository couldn't be found. Check the URL and your access.",
  CLONE_NETWORK: "A network problem interrupted the clone. Check your connection and try again.",
  CLONE_TIMEOUT: "The clone took too long and was stopped. Try again, or use a faster connection.",
  CLONE_CANCELLED: "Clone cancelled.",
  CLONE_UNKNOWN: "Something went wrong while cloning. Try again.",
};
const DEFAULT_FAILURE_MESSAGE = "Something went wrong while cloning. Try again.";

export function failureMessage(code: string): string {
  return FAILURE_MESSAGES[code] ?? DEFAULT_FAILURE_MESSAGE;
}
