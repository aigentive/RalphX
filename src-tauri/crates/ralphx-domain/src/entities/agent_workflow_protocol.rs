use std::io::{self, Read, Write};

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const AGENT_WORKFLOW_PROTOCOL_VERSION: u16 = 1;
pub const AGENT_WORKFLOW_MAX_FRAME_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentWorkflowFrame {
    pub version: u16,
    pub run_id: String,
    pub attempt: u32,
    pub runner_instance_id: String,
    pub message: AgentWorkflowProtocolMessage,
}

impl AgentWorkflowFrame {
    pub fn validate(&self) -> Result<(), String> {
        if self.version != AGENT_WORKFLOW_PROTOCOL_VERSION {
            return Err(format!(
                "Unsupported workflow protocol version {}",
                self.version
            ));
        }
        if self.run_id.is_empty() || self.runner_instance_id.is_empty() {
            return Err("Workflow frame lineage is required".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentWorkflowProtocolMessage {
    Execute {
        script: String,
        args: Value,
    },
    Ready,
    HostCall {
        call_id: String,
        operation: String,
        payload: Value,
    },
    HostResponse {
        call_id: String,
        result: Option<Value>,
        error: Option<String>,
    },
    Completed {
        result: Value,
    },
    Failed {
        error: String,
    },
    Shutdown,
}

pub fn write_workflow_frame(writer: &mut impl Write, frame: &AgentWorkflowFrame) -> io::Result<()> {
    frame
        .validate()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let payload = serde_json::to_vec(frame).map_err(io::Error::other)?;
    if payload.len() > AGENT_WORKFLOW_MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Workflow frame exceeds size limit",
        ));
    }
    writer.write_all(&(payload.len() as u32).to_be_bytes())?;
    writer.write_all(&payload)?;
    writer.flush()
}

pub fn read_workflow_frame(reader: &mut impl Read) -> io::Result<AgentWorkflowFrame> {
    let mut length = [0_u8; 4];
    reader.read_exact(&mut length)?;
    let length = u32::from_be_bytes(length) as usize;
    if length == 0 || length > AGENT_WORKFLOW_MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Invalid workflow frame length",
        ));
    }
    let mut payload = vec![0; length];
    reader.read_exact(&mut payload)?;
    let frame: AgentWorkflowFrame = serde_json::from_slice(&payload).map_err(io::Error::other)?;
    frame
        .validate()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    Ok(frame)
}
