import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from "@tauri-apps/plugin-notification";

describe("notification plugin web mock", () => {
  it("grants permission and ignores sends without loading the native binding", async () => {
    await expect(isPermissionGranted()).resolves.toBe(true);
    await expect(requestPermission()).resolves.toBe("granted");
    expect(() => {
      sendNotification({ title: "RalphX", body: "Web-mode mock" });
    }).not.toThrow();
  });
});
