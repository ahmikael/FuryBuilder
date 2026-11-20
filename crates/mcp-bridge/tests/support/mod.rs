#![allow(dead_code)]

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;

pub const TOKEN: &str = "test-token-0123456789abcdef0123456789abcdef";

/// One stdio MCP bridge process. Requests are sequential; notifications are ignored.
pub struct BridgeProcess {
    child: Child,
    stdin: ChildStdin,
    responses: mpsc::UnboundedReceiver<Value>,
    next_id: u64,
}

impl BridgeProcess {
    pub fn spawn(runtime_path: &Path) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_furybuilder-mcp"))
            .env("FURYBUILDER_MCP_RUNTIME", runtime_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn MCP bridge");
        let stdin = child.stdin.take().expect("bridge stdin");
        let stdout = child.stdout.take().expect("bridge stdout");
        let (sender, responses) = mpsc::unbounded_channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                let Ok(message) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                if message.get("id").is_some() {
                    let _ = sender.send(message);
                }
            }
        });
        Self {
            child,
            stdin,
            responses,
            next_id: 1,
        }
    }

    pub async fn request(&mut self, method: &str, params: Value) -> Value {
        self.request_with_meta(method, params, None).await
    }

    pub async fn request_with_meta(
        &mut self,
        method: &str,
        params: Value,
        meta: Option<Value>,
    ) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        let mut message = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        if let Some(meta) = meta {
            message["params"]["_meta"] = meta;
        }
        writeln!(self.stdin, "{message}").expect("write bridge request");
        self.stdin.flush().expect("flush bridge request");
        let response = tokio::time::timeout(Duration::from_secs(20), self.responses.recv())
            .await
            .expect("bridge response timed out")
            .expect("bridge closed stdout");
        assert_eq!(response["id"], id, "unexpected response: {response}");
        response
    }

    pub fn notify(&mut self, method: &str, params: Value) {
        let message = json!({ "jsonrpc": "2.0", "method": method, "params": params });
        writeln!(self.stdin, "{message}").expect("write bridge notification");
        self.stdin.flush().expect("flush bridge notification");
    }

    pub async fn initialize_legacy(&mut self) -> Value {
        let response = self
            .request(
                "initialize",
                json!({
                    "protocolVersion": "2025-06-18",
                    "capabilities": {},
                    "clientInfo": { "name": "legacy-test", "version": "1.0.0" }
                }),
            )
            .await;
        assert!(
            response.get("result").is_some(),
            "initialize failed: {response}"
        );
        response
    }

    pub fn modern_meta() -> Value {
        json!({
            "io.modelcontextprotocol/protocolVersion": "2026-07-28",
            "io.modelcontextprotocol/clientCapabilities": {},
        })
    }
}

impl Drop for BridgeProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub fn result_text(response: &Value) -> &str {
    response["result"]["content"]
        .as_array()
        .and_then(|content| {
            content
                .iter()
                .find(|block| block["type"] == "text")
                .and_then(|block| block["text"].as_str())
        })
        .expect("tool result text")
}

pub fn tool_json(response: &Value) -> Value {
    serde_json::from_str(result_text(response)).expect("tool result JSON")
}

pub fn ok(request: &Value, result: Value) -> Value {
    json!({
        "protocol": 1,
        "id": request["id"],
        "ok": true,
        "result": result,
    })
}

pub fn failed(request: &Value, code: &str, message: &str) -> Value {
    json!({
        "protocol": 1,
        "id": request["id"],
        "ok": false,
        "error": { "code": code, "message": message },
    })
}

pub type Handler = Arc<dyn Fn(Value) -> Value + Send + Sync>;

pub struct MockDesktop {
    pub runtime_path: PathBuf,
    pub seen: Arc<Mutex<Vec<Value>>>,
    directory: PathBuf,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for MockDesktop {
    fn drop(&mut self) {
        self.task.abort();
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

impl MockDesktop {
    pub async fn start(handler: Handler) -> Self {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_in_loop = seen.clone();
        let unique = uuid::Uuid::new_v4().simple().to_string();
        let directory = std::env::temp_dir().join(format!("furybuilder-mcp-test-{unique}"));
        std::fs::create_dir_all(&directory).expect("create test directory");
        let runtime_path = directory.join("runtime.json");

        #[cfg(windows)]
        let (endpoint, task) = {
            let pipe_name = format!(
                r"\\.\pipe\furybuilder-mcp-test-{}-{}",
                std::process::id(),
                &unique[..12]
            );
            let first = tokio::net::windows::named_pipe::ServerOptions::new()
                .first_pipe_instance(true)
                .max_instances(8)
                .create(&pipe_name)
                .expect("create named pipe");
            let task_pipe_name = pipe_name.clone();
            let task = tokio::spawn(async move {
                let mut server = first;
                loop {
                    if server.connect().await.is_err() {
                        break;
                    }
                    let mut connection = server;
                    let request = match read_line(&mut connection).await {
                        Ok(line) => match serde_json::from_slice::<Value>(&line) {
                            Ok(request) => request,
                            Err(_) => break,
                        },
                        Err(_) => break,
                    };
                    seen_in_loop
                        .lock()
                        .expect("seen lock")
                        .push(request.clone());
                    let response = handler(request);
                    // Prepare the next listening instance before publishing this response,
                    // so the sequential client never observes a missing pipe.
                    let mut options = tokio::net::windows::named_pipe::ServerOptions::new();
                    options.max_instances(8);
                    let next = match options.create(&task_pipe_name) {
                        Ok(next) => next,
                        Err(_) => break,
                    };
                    if connection
                        .write_all(format!("{response}\n").as_bytes())
                        .await
                        .is_err()
                    {
                        break;
                    }
                    let _ = connection.flush().await;
                    drop(connection);
                    server = next;
                }
            });
            (pipe_name, task)
        };

        #[cfg(unix)]
        let (endpoint, task) = {
            let socket_path = directory.join("desktop.sock");
            let listener = tokio::net::UnixListener::bind(&socket_path).expect("bind socket");
            let endpoint = socket_path.to_string_lossy().into_owned();
            let task = tokio::spawn(async move {
                loop {
                    let Ok((mut connection, _)) = listener.accept().await else {
                        break;
                    };
                    let request = match read_line(&mut connection).await {
                        Ok(line) => match serde_json::from_slice::<Value>(&line) {
                            Ok(request) => request,
                            Err(_) => continue,
                        },
                        Err(_) => continue,
                    };
                    seen_in_loop
                        .lock()
                        .expect("seen lock")
                        .push(request.clone());
                    let response = handler(request);
                    let _ = connection
                        .write_all(format!("{response}\n").as_bytes())
                        .await;
                    let _ = connection.flush().await;
                }
            });
            (endpoint, task)
        };

        let runtime = json!({
            "schemaVersion": 1,
            "transport": "local_socket",
            "endpoint": endpoint,
            "token": TOKEN,
            "pid": std::process::id(),
            "protocolVersion": 1,
            "appVersion": "0.1.0",
            "startedAtMs": 1,
        });
        std::fs::write(
            &runtime_path,
            serde_json::to_vec_pretty(&runtime).expect("runtime json"),
        )
        .expect("write runtime file");

        Self {
            runtime_path,
            seen,
            directory,
            task,
        }
    }

    pub fn requests(&self) -> Vec<Value> {
        self.seen.lock().expect("seen lock").clone()
    }
}

async fn read_line(reader: &mut (impl AsyncRead + Unpin)) -> std::io::Result<Vec<u8>> {
    let mut line = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let read = reader.read(&mut chunk).await?;
        if read == 0 {
            break;
        }
        line.extend_from_slice(&chunk[..read]);
        if let Some(newline) = line.iter().position(|byte| *byte == b'\n') {
            line.truncate(newline);
            break;
        }
    }
    Ok(line)
}
