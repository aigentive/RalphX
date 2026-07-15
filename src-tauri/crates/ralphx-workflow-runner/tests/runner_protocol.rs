use std::process::{Command, Stdio};

use ralphx_domain::entities::agent_workflow_protocol::{
    read_workflow_frame, write_workflow_frame, AgentWorkflowFrame, AgentWorkflowProtocolMessage,
    AGENT_WORKFLOW_PROTOCOL_VERSION,
};

fn execute(script: &str) -> AgentWorkflowProtocolMessage {
    let mut child = Command::new(env!("CARGO_BIN_EXE_ralphx-workflow-runner"))
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let request = AgentWorkflowFrame {
        version: AGENT_WORKFLOW_PROTOCOL_VERSION,
        run_id: "run-1".into(),
        attempt: 1,
        runner_instance_id: "runner-1".into(),
        message: AgentWorkflowProtocolMessage::Execute {
            script: script.into(),
            args: serde_json::json!({"input": 2}),
        },
    };
    write_workflow_frame(child.stdin.as_mut().unwrap(), &request).unwrap();
    let stdout = child.stdout.as_mut().unwrap();
    assert!(matches!(
        read_workflow_frame(stdout).unwrap().message,
        AgentWorkflowProtocolMessage::Ready
    ));
    let terminal = read_workflow_frame(stdout).unwrap().message;
    assert!(child.wait().unwrap().success());
    terminal
}

#[test]
fn executes_top_level_await_without_ambient_os_capabilities() {
    let terminal = execute(
        "await Promise.resolve(); return { value: args.input + 1, process: typeof process, require: typeof require, fetch: typeof fetch };",
    );
    assert_eq!(
        terminal,
        AgentWorkflowProtocolMessage::Completed {
            result: serde_json::json!({"value": 3, "process": "undefined", "require": "undefined", "fetch": "undefined"})
        }
    );
}

#[test]
fn sandbox_escape_attempt_is_a_failed_terminal_result() {
    assert!(matches!(
        execute("return process.env;"),
        AgentWorkflowProtocolMessage::Failed { .. }
    ));
}

#[test]
fn parallel_agent_descriptors_use_one_backend_concurrency_call() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_ralphx-workflow-runner"))
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let request = AgentWorkflowFrame {
        version: AGENT_WORKFLOW_PROTOCOL_VERSION,
        run_id: "run-parallel".into(),
        attempt: 1,
        runner_instance_id: "runner-parallel".into(),
        message: AgentWorkflowProtocolMessage::Execute {
            script: r#"
                return await parallel([
                    { prompt: "review A", logicalKey: "review-a" },
                    { prompt: "review B", logicalKey: "review-b" }
                ]);
            "#
            .into(),
            args: serde_json::json!({}),
        },
    };
    write_workflow_frame(child.stdin.as_mut().unwrap(), &request).unwrap();
    let stdout = child.stdout.as_mut().unwrap();
    assert!(matches!(
        read_workflow_frame(stdout).unwrap().message,
        AgentWorkflowProtocolMessage::Ready
    ));
    let call = read_workflow_frame(stdout).unwrap();
    let (call_id, payload) = match call.message {
        AgentWorkflowProtocolMessage::HostCall {
            call_id,
            operation,
            payload,
        } => {
            assert_eq!(operation, "parallel");
            (call_id, payload)
        }
        other => panic!("expected parallel host call, got {other:?}"),
    };
    assert_eq!(payload["items"].as_array().unwrap().len(), 2);
    write_workflow_frame(
        child.stdin.as_mut().unwrap(),
        &AgentWorkflowFrame {
            message: AgentWorkflowProtocolMessage::HostResponse {
                call_id,
                result: Some(serde_json::json!([{"ok": "a"}, {"ok": "b"}])),
                error: None,
            },
            ..request
        },
    )
    .unwrap();
    assert_eq!(
        read_workflow_frame(stdout).unwrap().message,
        AgentWorkflowProtocolMessage::Completed {
            result: serde_json::json!([{"ok": "a"}, {"ok": "b"}])
        }
    );
    assert!(child.wait().unwrap().success());
}
