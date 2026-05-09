use tauri::menu::{Menu, MenuEvent};
use tauri::{AppHandle, Emitter, Runtime};

pub(crate) const MENU_CHECK_FOR_UPDATES_ID: &str = "ralphx-check-for-updates";
pub(crate) const MENU_RELEASE_NOTES_ID: &str = "ralphx-release-notes";
pub(crate) const EVENT_CHECK_FOR_UPDATES: &str = "ralphx://check-for-updates";
pub(crate) const EVENT_SHOW_RELEASE_NOTES: &str = "ralphx://show-release-notes";

pub(crate) fn build_app_menu<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    let menu = Menu::default(app)?;

    #[cfg(target_os = "macos")]
    install_ralphx_app_menu_items(app, &menu)?;

    Ok(menu)
}

pub(crate) fn handle_menu_event<R: Runtime>(app: &AppHandle<R>, event: MenuEvent) {
    if let Some(event_name) = menu_event_name_for_id(event.id().as_ref()) {
        if let Err(error) = app.emit(event_name, ()) {
            tracing::warn!(%error, %event_name, "Failed to emit RalphX menu event");
        }
    }
}

pub(crate) fn menu_event_name_for_id(id: &str) -> Option<&'static str> {
    match id {
        MENU_CHECK_FOR_UPDATES_ID => Some(EVENT_CHECK_FOR_UPDATES),
        MENU_RELEASE_NOTES_ID => Some(EVENT_SHOW_RELEASE_NOTES),
        _ => None,
    }
}

#[cfg(target_os = "macos")]
fn install_ralphx_app_menu_items<R: Runtime>(
    app: &AppHandle<R>,
    menu: &Menu<R>,
) -> tauri::Result<()> {
    use tauri::menu::{MenuItem, MenuItemKind, PredefinedMenuItem};

    let check_for_updates = MenuItem::with_id(
        app,
        MENU_CHECK_FOR_UPDATES_ID,
        "Check for Updates...",
        true,
        None::<&str>,
    )?;
    let release_notes = MenuItem::with_id(
        app,
        MENU_RELEASE_NOTES_ID,
        "Release Notes",
        true,
        None::<&str>,
    )?;
    let separator = PredefinedMenuItem::separator(app)?;

    if let Some(MenuItemKind::Submenu(app_submenu)) = menu.items()?.into_iter().next() {
        app_submenu.insert_items(&[&check_for_updates, &release_notes, &separator], 2)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;
    use tauri::menu::MenuId;
    use tauri::Listener;

    #[test]
    fn maps_ralphx_menu_ids_to_frontend_events() {
        assert_eq!(
            menu_event_name_for_id(MENU_CHECK_FOR_UPDATES_ID),
            Some(EVENT_CHECK_FOR_UPDATES)
        );
        assert_eq!(
            menu_event_name_for_id(MENU_RELEASE_NOTES_ID),
            Some(EVENT_SHOW_RELEASE_NOTES)
        );
        assert_eq!(menu_event_name_for_id("unrelated"), None);
    }

    #[test]
    fn handle_menu_event_emits_frontend_event() {
        let app = crate::testing::create_mock_app();
        let (sender, receiver) = mpsc::channel();
        let _unlisten = app.handle().listen(EVENT_CHECK_FOR_UPDATES, move |_| {
            sender.send(()).expect("send event notification");
        });

        handle_menu_event(
            app.handle(),
            MenuEvent {
                id: MenuId::new(MENU_CHECK_FOR_UPDATES_ID),
            },
        );

        receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("menu event should emit frontend event");
    }

    #[test]
    fn handle_menu_event_ignores_unrelated_ids() {
        let app = crate::testing::create_mock_app();
        let (sender, receiver) = mpsc::channel();
        let _unlisten = app.handle().listen(EVENT_CHECK_FOR_UPDATES, move |_| {
            sender.send(()).expect("send event notification");
        });

        handle_menu_event(
            app.handle(),
            MenuEvent {
                id: MenuId::new("unrelated"),
            },
        );

        assert!(receiver.recv_timeout(Duration::from_millis(50)).is_err());
    }
}
