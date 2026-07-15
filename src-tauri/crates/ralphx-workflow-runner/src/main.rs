use std::cell::RefCell;
use std::io;
use std::rc::Rc;
use std::time::{Duration, Instant};

use ralphx_domain::entities::agent_workflow_protocol::{
    read_workflow_frame, write_workflow_frame, AgentWorkflowFrame, AgentWorkflowProtocolMessage,
};
use rquickjs::{function::Func, Context, Error, Promise, Runtime, Value};
use serde_json::Value as JsonValue;

const MEMORY_LIMIT_BYTES: usize = 64 * 1024 * 1024;
const STACK_LIMIT_BYTES: usize = 512 * 1024;
const WALL_CLOCK_LIMIT: Duration = Duration::from_secs(300);

fn main() {
    if let Err(error) = run() {
        eprintln!("ralphx-workflow-runner: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let input = read_workflow_frame(&mut io::stdin().lock()).map_err(|error| error.to_string())?;
    let (script, args) = match &input.message {
        AgentWorkflowProtocolMessage::Execute { script, args } => (script.clone(), args.clone()),
        _ => return Err("First workflow frame must be execute".into()),
    };
    let lineage = input.clone();
    send(&lineage, AgentWorkflowProtocolMessage::Ready)?;
    match evaluate(&lineage, &script, args) {
        Ok(result) => send(&lineage, AgentWorkflowProtocolMessage::Completed { result }),
        Err(error) => send(&lineage, AgentWorkflowProtocolMessage::Failed { error }),
    }
}

fn send(lineage: &AgentWorkflowFrame, message: AgentWorkflowProtocolMessage) -> Result<(), String> {
    write_workflow_frame(
        &mut io::stdout().lock(),
        &AgentWorkflowFrame {
            version: lineage.version,
            run_id: lineage.run_id.clone(),
            attempt: lineage.attempt,
            runner_instance_id: lineage.runner_instance_id.clone(),
            message,
        },
    )
    .map_err(|error| error.to_string())
}

fn evaluate(
    lineage: &AgentWorkflowFrame,
    script: &str,
    args: JsonValue,
) -> Result<JsonValue, String> {
    let runtime = Runtime::new().map_err(|error| error.to_string())?;
    runtime.set_memory_limit(MEMORY_LIMIT_BYTES);
    runtime.set_max_stack_size(STACK_LIMIT_BYTES);
    let deadline = Instant::now() + WALL_CLOCK_LIMIT;
    runtime.set_interrupt_handler(Some(Box::new(move || Instant::now() >= deadline)));
    let context = Context::full(&runtime).map_err(|error| error.to_string())?;
    let lineage = lineage.clone();
    let stdin = Rc::new(RefCell::new(io::stdin()));
    context.with(|ctx| {
        let args = ctx
            .json_parse(serde_json::to_string(&args).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
        ctx.globals().set("args", args).map_err(|error| error.to_string())?;
        let stdin = stdin.clone();
        ctx.globals()
            .set(
                "__ralphxHostCall",
                Func::from(move |operation: String, payload_json: String| {
                    host_call(&lineage, &stdin, operation, payload_json)
                }),
            )
            .map_err(|error| error.to_string())?;
        ctx.eval::<(), _>(r#"
            globalThis.meta = Object.freeze({ protocolVersion: 1 });
            globalThis.phase = (name) => JSON.parse(__ralphxHostCall("phase", JSON.stringify({ name })));
            globalThis.log = (level, message) => JSON.parse(__ralphxHostCall("log", JSON.stringify({ level, message })));
            globalThis.agent = (prompt, options = {}) => JSON.parse(__ralphxHostCall("agent", JSON.stringify({ prompt, ...options })));
            globalThis.parallel = async (items) => {
                if (!Array.isArray(items)) throw new TypeError("parallel() requires an array");
                if (items.every((item) => typeof item === "function")) {
                    return Promise.all(items.map((item) => item()));
                }
                return JSON.parse(__ralphxHostCall("parallel", JSON.stringify({ items })));
            };
            globalThis.pipeline = async (steps, initial) => { let value = initial; for (const step of steps) value = await step(value); return value; };
            globalThis.checkpoint = (key, value) => JSON.parse(__ralphxHostCall("checkpoint", JSON.stringify({ key, value })));
        "#).map_err(|error| error.to_string())?;
        let promise: Promise = ctx
            .eval(format!("(async () => {{ {script}\n }})()"))
            .map_err(|error| error.to_string())?;
        let value: Value = promise.finish().map_err(|error| error.to_string())?;
        let json = ctx
            .json_stringify(value)
            .map_err(|error| error.to_string())?
            .map(|value| value.to_string().map_err(|error| error.to_string()))
            .transpose()?
            .unwrap_or_else(|| "null".into());
        serde_json::from_str(&json).map_err(|error| error.to_string())
    })
}

fn host_call(
    lineage: &AgentWorkflowFrame,
    stdin: &Rc<RefCell<io::Stdin>>,
    operation: String,
    payload_json: String,
) -> rquickjs::Result<String> {
    let call_id = uuid::Uuid::new_v4().to_string();
    let payload = serde_json::from_str(&payload_json)
        .map_err(|error| Error::new_from_js_message("JSON", "host call", error.to_string()))?;
    send(
        lineage,
        AgentWorkflowProtocolMessage::HostCall {
            call_id: call_id.clone(),
            operation,
            payload,
        },
    )
    .map_err(|error| Error::new_from_js_message("host", "JavaScript", error))?;
    let response = read_workflow_frame(&mut *stdin.borrow_mut())
        .map_err(|error| Error::new_from_js_message("host", "JavaScript", error.to_string()))?;
    if response.run_id != lineage.run_id
        || response.attempt != lineage.attempt
        || response.runner_instance_id != lineage.runner_instance_id
    {
        return Err(Error::new_from_js_message(
            "host",
            "JavaScript",
            "Stale workflow response lineage",
        ));
    }
    match response.message {
        AgentWorkflowProtocolMessage::HostResponse {
            call_id: response_id,
            result,
            error,
        } if response_id == call_id => {
            if let Some(error) = error {
                Err(Error::new_from_js_message("host", "JavaScript", error))
            } else {
                serde_json::to_string(&result.unwrap_or(JsonValue::Null)).map_err(|error| {
                    Error::new_from_js_message("host", "JavaScript", error.to_string())
                })
            }
        }
        _ => Err(Error::new_from_js_message(
            "host",
            "JavaScript",
            "Mismatched workflow host response",
        )),
    }
}
