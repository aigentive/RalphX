#[cfg(target_os = "macos")]
use super::runtime_wiring::{
    macos_traffic_light_origin_y, macos_traffic_light_target_center_y,
    should_recenter_macos_traffic_lights,
};

#[cfg(target_os = "macos")]
#[test]
fn traffic_light_target_center_tracks_navbar_midline_from_titlebar_top() {
    let title_bar_height = 64.0;
    let target_center_y = macos_traffic_light_target_center_y(title_bar_height);

    assert_eq!(target_center_y, 40.0);
    assert_eq!(title_bar_height - target_center_y, 24.0);
}

#[cfg(target_os = "macos")]
#[test]
fn traffic_light_origin_centers_button_on_converted_parent_coordinate() {
    let target_center_y_in_button_parent = 18.0;
    let button_height = 14.0;

    assert_eq!(
        macos_traffic_light_origin_y(target_center_y_in_button_parent, button_height),
        11.0
    );
}

#[cfg(target_os = "macos")]
#[test]
fn traffic_light_centering_reapplies_after_native_layout_events() {
    use tauri::{PhysicalSize, WindowEvent};

    assert!(should_recenter_macos_traffic_lights(&WindowEvent::Focused(true)));
    assert!(should_recenter_macos_traffic_lights(&WindowEvent::Resized(
        PhysicalSize::new(1200, 800),
    )));
    assert!(!should_recenter_macos_traffic_lights(&WindowEvent::Focused(false)));
}
