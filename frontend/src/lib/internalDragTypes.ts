export const RALPHX_TERMINAL_DOCK_DRAG_TYPE = "application/x-ralphx-terminal-dock";

let ralphxTerminalDockDragActive = false;

export function setRalphxTerminalDockDragActive(active: boolean) {
  ralphxTerminalDockDragActive = active;
}

export function isRalphxTerminalDockDragActive() {
  return ralphxTerminalDockDragActive;
}
