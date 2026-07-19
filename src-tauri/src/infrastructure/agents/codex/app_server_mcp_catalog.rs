use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

use crate::domain::agents::{
    AgentHarnessKind, McpServerKey, NativeMcpServerSnapshot, NativeMcpState,
};
use crate::infrastructure::tool_paths::{
    has_safe_absolute_binary_path_shape, is_safe_launchable_binary_path,
};
use crate::utils::path_safety::validate_absolute_non_root_path;

const APP_SERVER_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_RESPONSE_LINE_BYTES: usize = 8 * 1024 * 1024;
const MAX_PAGES: usize = 50;
const MAX_SERVERS: usize = 500;
const PAGE_SIZE: u32 = 100;

pub(crate) async fn discover_native_mcp_servers_via_app_server(
    cli_path: &Path,
    codex_root: &Path,
    project_root: Option<&Path>,
    effective_env: &HashMap<String, String>,
) -> Result<Vec<NativeMcpServerSnapshot>, String> {
    let cli_path = validate_cli_path(cli_path)?;
    let codex_root = validate_absolute_non_root_path(codex_root, "Codex config root")
        .map_err(|error| error.to_string())?;
    let project_root = project_root
        .map(|path| {
            validate_absolute_non_root_path(path, "Codex project root")
                .map_err(|error| error.to_string())
        })
        .transpose()?;

    let mut command = Command::new(cli_path);
    command
        .args(["app-server", "--stdio"])
        .envs(effective_env)
        .env("CODEX_HOME", &codex_root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    if let Some(project_root) = project_root.as_deref() {
        command.current_dir(project_root);
    }

    let mut session = AppServerSession::spawn(command).await?;
    let result = tokio::time::timeout(
        APP_SERVER_TIMEOUT,
        read_catalog(&mut session, project_root.as_deref()),
    )
    .await
    .map_err(|_| "Codex app-server MCP catalog timed out".to_string())?;
    session.stop().await;
    result
}

fn validate_cli_path(path: &Path) -> Result<PathBuf, String> {
    if !has_safe_absolute_binary_path_shape(path) || !is_safe_launchable_binary_path(path) {
        return Err("Codex app-server executable path is not safely launchable".to_string());
    }
    Ok(path.to_path_buf())
}

async fn read_catalog(
    session: &mut AppServerSession,
    project_root: Option<&Path>,
) -> Result<Vec<NativeMcpServerSnapshot>, String> {
    session
        .request(
            1,
            "initialize",
            json!({
                "clientInfo": {"name": "ralphx", "version": env!("CARGO_PKG_VERSION")},
                "capabilities": {"experimentalApi": true}
            }),
        )
        .await?;
    session.notify("initialized", json!({})).await?;

    let config = session
        .request(
            2,
            "config/read",
            json!({
                "cwd": project_root.map(|path| path.to_string_lossy().to_string()),
                "includeLayers": true
            }),
        )
        .await?;
    let mut servers = snapshots_from_config(&config)?;

    let mut cursor: Option<String> = None;
    for page in 0..MAX_PAGES {
        let response = session
            .request(
                3 + page as u64,
                "mcpServerStatus/list",
                json!({
                    "cursor": cursor,
                    "detail": "toolsAndAuthOnly",
                    "limit": PAGE_SIZE
                }),
            )
            .await?;
        merge_status_page(&mut servers, &response)?;
        cursor = response
            .get("nextCursor")
            .and_then(Value::as_str)
            .map(str::to_string);
        if cursor.is_none() {
            return Ok(servers.into_values().collect());
        }
        if cursor.as_ref().is_some_and(|value| value.len() > 512) {
            return Err("Codex app-server returned an invalid pagination cursor".to_string());
        }
    }
    Err("Codex app-server MCP catalog exceeded the pagination limit".to_string())
}

fn snapshots_from_config(
    result: &Value,
) -> Result<BTreeMap<String, NativeMcpServerSnapshot>, String> {
    let definitions = result
        .get("config")
        .and_then(|config| config.get("mcp_servers"))
        .and_then(Value::as_object)
        .ok_or_else(|| "Codex app-server config/read omitted MCP metadata".to_string())?;
    if definitions.len() > MAX_SERVERS {
        return Err("Codex app-server MCP catalog exceeded the server limit".to_string());
    }
    let origin_scopes = config_origin_scopes(
        definitions.keys().map(String::as_str),
        result.get("origins").and_then(Value::as_object),
    );
    let mut servers = BTreeMap::new();
    for (server_id, definition) in definitions {
        let key = McpServerKey::new(AgentHarnessKind::Codex, server_id.clone())?;
        let enabled = definition
            .get("enabled")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let known_tools = configured_tool_names(definition);
        servers.insert(
            server_id.clone(),
            NativeMcpServerSnapshot {
                key,
                native_scope: origin_scopes.get(server_id).cloned(),
                native_state: if enabled {
                    NativeMcpState::Enabled
                } else {
                    NativeMcpState::Disabled
                },
                known_tools,
                diagnostic: None,
            },
        );
    }
    Ok(servers)
}

fn configured_tool_names(definition: &Value) -> Vec<String> {
    ["enabled_tools", "disabled_tools"]
        .into_iter()
        .flat_map(|field| {
            definition
                .get(field)
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
        })
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn config_origin_scopes<'a>(
    server_ids: impl IntoIterator<Item = &'a str>,
    origins: Option<&serde_json::Map<String, Value>>,
) -> BTreeMap<String, String> {
    let server_prefixes = server_ids
        .into_iter()
        .map(|server_id| (server_id, format!("mcp_servers.{server_id}.")))
        .collect::<Vec<_>>();
    let Some(origins) = origins else {
        return BTreeMap::new();
    };
    let mut scopes = BTreeMap::new();
    for (path, metadata) in origins {
        let Some((server_id, _)) = server_prefixes
            .iter()
            .filter(|(_, prefix)| path.starts_with(prefix))
            .max_by_key(|(server_id, _)| server_id.len())
        else {
            continue;
        };
        let Some(source) = metadata.pointer("/name/type").and_then(Value::as_str) else {
            continue;
        };
        let scope = match source {
            "project" => "project",
            "user" => "user",
            "system" | "mdm" | "enterpriseManaged" => "managed",
            _ => "effective",
        };
        scopes
            .entry((*server_id).to_string())
            .or_insert_with(|| scope.to_string());
    }
    scopes
}

fn merge_status_page(
    servers: &mut BTreeMap<String, NativeMcpServerSnapshot>,
    response: &Value,
) -> Result<(), String> {
    let data = response
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| "Codex app-server status response omitted server metadata".to_string())?;
    if servers.len().saturating_add(data.len()) > MAX_SERVERS {
        return Err("Codex app-server MCP catalog exceeded the server limit".to_string());
    }
    for status in data {
        let server_id = status
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| "Codex app-server status entry omitted a server ID".to_string())?;
        let key = McpServerKey::new(AgentHarnessKind::Codex, server_id.to_string())?;
        let auth_required = status.get("authStatus").and_then(Value::as_str) == Some("notLoggedIn");
        let tools = status
            .get("tools")
            .and_then(Value::as_object)
            .map(|tools| tools.keys().cloned().collect::<BTreeSet<_>>())
            .unwrap_or_default();
        let snapshot =
            servers
                .entry(server_id.to_string())
                .or_insert_with(|| NativeMcpServerSnapshot {
                    key,
                    native_scope: Some("effective".to_string()),
                    native_state: NativeMcpState::Enabled,
                    known_tools: Vec::new(),
                    diagnostic: None,
                });
        snapshot.known_tools.extend(tools);
        snapshot.known_tools.sort();
        snapshot.known_tools.dedup();
        if auth_required && snapshot.native_state.permits_launch() {
            snapshot.native_state = NativeMcpState::AuthRequired;
            snapshot.diagnostic = Some(
                "Codex reports that this MCP server requires native authentication.".to_string(),
            );
        }
    }
    Ok(())
}

struct AppServerSession {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl AppServerSession {
    async fn spawn(mut command: Command) -> Result<Self, String> {
        let mut child = command
            .spawn()
            .map_err(|_| "Failed to start Codex app-server catalog adapter".to_string())?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| "Codex app-server stdin is unavailable".to_string())?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "Codex app-server stdout is unavailable".to_string())?;
        Ok(Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
        })
    }

    async fn notify(&mut self, method: &str, params: Value) -> Result<(), String> {
        self.write_message(&json!({"jsonrpc": "2.0", "method": method, "params": params}))
            .await
    }

    async fn request(&mut self, id: u64, method: &str, params: Value) -> Result<Value, String> {
        self.write_message(
            &json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}),
        )
        .await?;
        loop {
            let mut line = String::new();
            let bytes = self
                .stdout
                .read_line(&mut line)
                .await
                .map_err(|_| "Failed to read Codex app-server response".to_string())?;
            if bytes == 0 {
                return Err("Codex app-server closed before completing the catalog".to_string());
            }
            if bytes > MAX_RESPONSE_LINE_BYTES {
                return Err("Codex app-server response exceeded the size limit".to_string());
            }
            let response: Value = serde_json::from_str(&line)
                .map_err(|_| "Codex app-server returned malformed JSON".to_string())?;
            if response.get("id").and_then(Value::as_u64) != Some(id) {
                continue;
            }
            if response.get("error").is_some() {
                return Err(format!(
                    "Codex app-server method {method} is unsupported or failed"
                ));
            }
            return response
                .get("result")
                .cloned()
                .ok_or_else(|| format!("Codex app-server method {method} omitted a result"));
        }
    }

    async fn write_message(&mut self, message: &Value) -> Result<(), String> {
        let mut encoded = serde_json::to_vec(message)
            .map_err(|_| "Failed to encode Codex app-server request".to_string())?;
        encoded.push(b'\n');
        self.stdin
            .write_all(&encoded)
            .await
            .map_err(|_| "Failed to write Codex app-server request".to_string())?;
        self.stdin
            .flush()
            .await
            .map_err(|_| "Failed to flush Codex app-server request".to_string())
    }

    async fn stop(&mut self) {
        let _ = self.child.kill().await;
        let _ = self.child.wait().await;
    }
}
