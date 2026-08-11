#[cfg(target_os = "macos")]
use super::runtime_wiring::{
    macos_traffic_light_origin_y, macos_traffic_light_target_center_y,
    should_recenter_macos_traffic_lights,
};

#[test]
fn verification_runtime_coordination_is_arc_shared() {
    let source = crate::application::AppState::new_test();
    let mut target = crate::application::AppState::new_test();
    super::runtime_wiring::share_plan_verification_runtime(&source, &mut target);

    assert!(std::sync::Arc::ptr_eq(
        &source.plan_verification_locks,
        &target.plan_verification_locks
    ));
    assert!(std::sync::Arc::ptr_eq(
        &source.plan_verification_admissions,
        &target.plan_verification_admissions
    ));
}

#[test]
fn event_sink_and_bus_are_physically_shared_between_app_states() {
    use ralphx_events::catalog::AGENT_RUN_COMPLETED;
    use ralphx_events::{EventEnvelope, EventSink, RecordingEventSink};

    let mut source = crate::application::AppState::new_test();
    let sink = std::sync::Arc::new(RecordingEventSink::new());
    source.events = sink.clone() as std::sync::Arc<dyn EventSink>;
    let mut target = crate::application::AppState::new_test();
    super::runtime_wiring::share_event_runtime(&source, &mut target);
    let mut target_subscriber = target.internal_event_bus.subscribe();
    let envelope = EventEnvelope::new(AGENT_RUN_COMPLETED, serde_json::json!({"ok": true}));

    source.events.emit(&envelope.name, envelope.payload.clone());
    source
        .internal_event_bus
        .publish(envelope.clone())
        .expect("shared target subscriber");

    assert!(std::sync::Arc::ptr_eq(&source.events, &target.events));
    assert_eq!(sink.events().len(), 1);
    assert_eq!(
        target_subscriber.try_recv().expect("shared bus envelope"),
        envelope
    );
}

#[test]
fn repair_publish_continuation_is_pointer_identical_in_paired_app_states() {
    let source = crate::application::AppState::new_test();
    let mut target = crate::application::AppState::new_test();

    super::runtime_wiring::share_agent_workspace_repair_publish_continuation(&source, &mut target);

    assert!(std::sync::Arc::ptr_eq(
        &source.agent_workspace_repair_publish_continuation,
        &target.agent_workspace_repair_publish_continuation
    ));
}

#[test]
fn pr_fix_review_publish_resumer_is_pointer_identical_in_paired_app_states() {
    let source = crate::application::AppState::new_test();
    let mut target = crate::application::AppState::new_test();

    super::runtime_wiring::share_agent_workspace_pr_fix_review_publish_resumer(
        &source,
        &mut target,
    );

    assert!(std::sync::Arc::ptr_eq(
        &source.agent_workspace_pr_fix_review_publish_resumer,
        &target.agent_workspace_pr_fix_review_publish_resumer
    ));
}

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

    assert!(should_recenter_macos_traffic_lights(&WindowEvent::Focused(
        true
    )));
    assert!(should_recenter_macos_traffic_lights(&WindowEvent::Resized(
        PhysicalSize::new(1200, 800),
    )));
    assert!(!should_recenter_macos_traffic_lights(
        &WindowEvent::Focused(false)
    ));
}

#[test]
fn startup_coordinator_is_pointer_identical_in_paired_app_states() {
    let source = crate::application::AppState::new_test();
    let mut target = crate::application::AppState::new_test();

    super::runtime_wiring::share_startup_coordinator(&source, &mut target);

    assert!(std::sync::Arc::ptr_eq(
        &source.startup_coordinator,
        &target.startup_coordinator
    ));
}
