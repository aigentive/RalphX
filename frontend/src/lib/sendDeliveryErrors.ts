/**
 * Send failures that are NOT "nothing happened".
 *
 * Gate 1 writes a turn into the live agent process before persisting it, so a
 * repository failure after that write means the agent IS answering a message we
 * could not store. Reporting that as a plain send failure would drop the user's
 * bubble and the spinner while the response streams in.
 *
 * Backend marker: `MESSAGE_DELIVERED_NOT_PERSISTED_PREFIX` in
 * `src-tauri/src/application/chat_service/chat_service_types.rs`.
 */
export const MESSAGE_DELIVERED_NOT_PERSISTED_PREFIX =
  "[Message delivered but not saved:";

export function isMessageDeliveredNotPersistedError(error: unknown): boolean {
  return (
    typeof error === "string" &&
    error.startsWith(MESSAGE_DELIVERED_NOT_PERSISTED_PREFIX)
  );
}
