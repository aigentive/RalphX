use objc2::{AllocAnyThread, MainThreadMarker};
use objc2_app_kit::{NSApplication, NSImage};
use objc2_foundation::NSData;
use tracing::warn;

const DEV_DOCK_ICON_PNG: &[u8] = include_bytes!("../../icons/dev-light.png");

pub(crate) fn set_light_dev_dock_icon() {
    let mtm = unsafe { MainThreadMarker::new_unchecked() };
    let app = NSApplication::sharedApplication(mtm);
    let data = NSData::with_bytes(DEV_DOCK_ICON_PNG);
    let Some(app_icon) = NSImage::initWithData(NSImage::alloc(), &data) else {
        warn!("failed to construct macOS dev Dock icon from bundled PNG");
        return;
    };

    unsafe { app.setApplicationIconImage(Some(&app_icon)) };
}
