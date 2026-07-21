use super::agent_workflow_protocol::*;

fn frame() -> AgentWorkflowFrame {
    AgentWorkflowFrame {
        version: AGENT_WORKFLOW_PROTOCOL_VERSION,
        run_id: "run-1".into(),
        attempt: 2,
        runner_instance_id: "runner-1".into(),
        message: AgentWorkflowProtocolMessage::Ready,
    }
}

#[test]
fn framed_json_round_trips_with_attempt_lineage() {
    let mut bytes = Vec::new();
    write_workflow_frame(&mut bytes, &frame()).unwrap();
    assert_eq!(read_workflow_frame(&mut bytes.as_slice()).unwrap(), frame());
}

#[test]
fn protocol_version_and_oversized_frames_fail_closed() {
    let mut invalid = frame();
    invalid.version += 1;
    assert!(write_workflow_frame(&mut Vec::new(), &invalid).is_err());
    let bytes = ((AGENT_WORKFLOW_MAX_FRAME_BYTES + 1) as u32)
        .to_be_bytes()
        .to_vec();
    assert!(read_workflow_frame(&mut bytes.as_slice()).is_err());
}
