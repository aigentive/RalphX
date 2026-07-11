/**
 * Mock implementation of @tauri-apps/plugin-notification for web mode.
 *
 * Browser mode has no native notification bridge, so permission is treated as granted and sends
 * resolve without producing an OS notification.
 */

export type PermissionState = "granted" | "denied" | "prompt" | "prompt-with-rationale";

export interface NotificationOptions {
  title?: string;
  body?: string;
}

export async function isPermissionGranted(): Promise<boolean> {
  return true;
}

export async function requestPermission(): Promise<PermissionState> {
  return "granted";
}

export function sendNotification(_options: string | NotificationOptions): void {}
