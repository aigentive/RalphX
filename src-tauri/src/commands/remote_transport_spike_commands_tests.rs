use super::remote_transport_spike_commands::DebugStartRemoteTransportCorsProbeInput;
use crate::remote_server::transport_spike::DebugCorsProbeOrdering;

#[test]
fn start_probe_input_deserializes_the_camel_case_ordering() {
    let input: DebugStartRemoteTransportCorsProbeInput =
        serde_json::from_str(r#"{"ordering":"authBeforeOptions"}"#)
            .expect("Tauri command input should deserialize");

    assert_eq!(input.ordering, DebugCorsProbeOrdering::AuthBeforeOptions);
}
