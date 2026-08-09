use std::sync::{Arc, Mutex};
use std::time::Duration;

use ralphx_events::catalog::AGENT_RUN_COMPLETED;
use ralphx_lib::commands::install_agent_workspace_completion_dispatch_for_test;
use serde_json::{json, Value};
use tauri::{Emitter, Listener};

#[tokio::test]
async fn correlated_tauri_and_bus_delivery_fans_out_once_with_unchanged_payload() {
    let app = tauri::test::mock_builder()
        .build(super::tauri_context())
        .expect("mock app should build");
    let observed_payload = Arc::new(Mutex::new(None::<Value>));
    let callback_payload = Arc::clone(&observed_payload);
    app.handle().listen_any(AGENT_RUN_COMPLETED, move |event| {
        *callback_payload.lock().expect("payload lock") =
            Some(serde_json::from_str(event.payload()).expect("completion payload"));
    });
    let dispatch = install_agent_workspace_completion_dispatch_for_test(app.handle().clone());
    let payload = json!({
        "conversation_id": "11111111-1111-1111-1111-111111111111",
        "context_type": "project",
        "context_id": "project-1",
        "run_id": "22222222-2222-2222-2222-222222222222"
    });

    dispatch.emit(AGENT_RUN_COMPLETED, payload.clone());
    for _ in 0..40 {
        if dispatch.observed_fanout_counts() == (1, 1, 1)
            && dispatch.pending_completion_correlations() == 0
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    tokio::time::sleep(Duration::from_millis(25)).await;

    assert_eq!(
        observed_payload.lock().expect("payload lock").clone(),
        Some(payload.clone())
    );
    assert_eq!(dispatch.observed_fanout_counts(), (1, 1, 1));
    assert_eq!(
        dispatch.pending_completion_correlations(),
        0,
        "both correlated transports must settle the shared event identity"
    );

    app.handle()
        .emit(AGENT_RUN_COMPLETED, payload)
        .expect("uncorrelated Tauri test event should emit");
    tokio::time::sleep(Duration::from_millis(25)).await;
    assert_eq!(
        dispatch.observed_fanout_counts(),
        (1, 1, 1),
        "an uncorrelated Tauri callback must not schedule automation"
    );
    dispatch.shutdown().await;
}
