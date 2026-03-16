use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;

use anyhow::{Context, Result, anyhow};
use serde::Serialize;
use serde_json::Value;

use super::protocol::{RpcRequest, RpcResponseEnvelope};

#[derive(Debug)]
pub enum BackendEvent {
    Response(RpcResponseEnvelope),
    Log(String),
    Exited(Option<i32>),
}

#[derive(Debug, Clone)]
pub struct BackendCommand {
    template: String,
}

impl BackendCommand {
    pub fn new(template: impl Into<String>) -> Self {
        Self {
            template: template.into(),
        }
    }

    pub fn template(&self) -> &str {
        &self.template
    }

    pub fn render(&self, ledger_path: &Path, task_name: Option<&str>) -> String {
        let quoted_ledger = shell_quote(ledger_path.to_string_lossy().as_ref());
        let quoted_task = shell_quote(task_name.unwrap_or(""));
        let mut command = self
            .template
            .replace("{ledger}", &quoted_ledger)
            .replace("{task}", &quoted_task);

        if !self.template.contains("{ledger}") {
            command.push_str(" --ledger ");
            command.push_str(&quoted_ledger);
        }
        if !command.contains("--stdio") {
            command.push_str(" --stdio");
        }
        command
    }
}

pub struct BackendClient {
    stdin: Arc<Mutex<ChildStdin>>,
    child: Arc<Mutex<Child>>,
    next_id: AtomicU64,
    command: String,
}

impl BackendClient {
    pub fn launch(
        command: BackendCommand,
        ledger_path: &Path,
        task_name: Option<&str>,
    ) -> Result<(Self, mpsc::Receiver<BackendEvent>)> {
        let rendered = command.render(ledger_path, task_name);
        let mut child = Command::new("bash")
            .arg("-lc")
            .arg(&rendered)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("failed to launch backend command: {rendered}"))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("backend stdin unavailable"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("backend stdout unavailable"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| anyhow!("backend stderr unavailable"))?;

        let child = Arc::new(Mutex::new(child));
        let stdin = Arc::new(Mutex::new(stdin));
        let (tx, rx) = mpsc::channel();

        {
            let tx = tx.clone();
            thread::spawn(move || {
                let reader = BufReader::new(stdout);
                for line in reader.lines() {
                    match line {
                        Ok(line) if line.trim().is_empty() => continue,
                        Ok(line) => match serde_json::from_str::<RpcResponseEnvelope>(&line) {
                            Ok(response) => {
                                let _ = tx.send(BackendEvent::Response(response));
                            }
                            Err(err) => {
                                let _ = tx.send(BackendEvent::Log(format!(
                                    "invalid backend stdout line: {err}: {line}"
                                )));
                            }
                        },
                        Err(err) => {
                            let _ = tx.send(BackendEvent::Log(format!(
                                "backend stdout read failed: {err}"
                            )));
                            break;
                        }
                    }
                }
            });
        }

        {
            let tx = tx.clone();
            thread::spawn(move || {
                let reader = BufReader::new(stderr);
                for line in reader.lines() {
                    match line {
                        Ok(line) if line.trim().is_empty() => continue,
                        Ok(line) => {
                            let _ = tx.send(BackendEvent::Log(line));
                        }
                        Err(err) => {
                            let _ = tx.send(BackendEvent::Log(format!(
                                "backend stderr read failed: {err}"
                            )));
                            break;
                        }
                    }
                }
            });
        }

        {
            let tx = tx.clone();
            let child = Arc::clone(&child);
            thread::spawn(move || {
                let status = child
                    .lock()
                    .expect("child lock poisoned")
                    .wait()
                    .ok()
                    .and_then(|status| status.code());
                let _ = tx.send(BackendEvent::Exited(status));
            });
        }

        Ok((
            Self {
                stdin,
                child,
                next_id: AtomicU64::new(1),
                command: rendered,
            },
            rx,
        ))
    }

    pub fn command_line(&self) -> &str {
        &self.command
    }

    pub fn request<T>(&self, method: &str, params: T) -> Result<String>
    where
        T: Serialize,
    {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst).to_string();
        let request = RpcRequest::new(id.clone(), method, params);
        let line = serde_json::to_string(&request).context("failed to serialize request")?;

        let mut stdin = self.stdin.lock().expect("stdin lock poisoned");
        stdin
            .write_all(line.as_bytes())
            .context("failed to write rpc request")?;
        stdin
            .write_all(b"\n")
            .context("failed to write rpc newline")?;
        stdin.flush().context("failed to flush rpc request")?;
        Ok(id)
    }

    pub fn notify(&self, method: &str, params: Value) -> Result<()> {
        let payload = serde_json::json!({
            "jsonrpc": super::protocol::JSONRPC_VERSION,
            "method": method,
            "params": params,
        });
        let line =
            serde_json::to_string(&payload).context("failed to serialize rpc notification")?;
        let mut stdin = self.stdin.lock().expect("stdin lock poisoned");
        stdin
            .write_all(line.as_bytes())
            .context("failed to write rpc notification")?;
        stdin
            .write_all(b"\n")
            .context("failed to write rpc newline")?;
        stdin.flush().context("failed to flush rpc notification")?;
        Ok(())
    }

    pub fn shutdown(&self) -> Result<String> {
        self.request("system.shutdown", serde_json::json!({}))
    }

    pub fn kill(&self) -> Result<()> {
        self.child
            .lock()
            .expect("child lock poisoned")
            .kill()
            .context("failed to terminate backend")
    }
}

fn shell_quote(value: &str) -> String {
    if value.is_empty() {
        return "''".to_string();
    }
    let escaped = value.replace('"', "\\\"");
    format!("\"{escaped}\"")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn renders_command_with_defaults() {
        let command = BackendCommand::new("respkit-ledger-service");
        let rendered = command.render(Path::new("/tmp/test ledger.sqlite"), Some("demo"));
        assert!(rendered.contains("--ledger \"/tmp/test ledger.sqlite\""));
        assert!(rendered.contains("--stdio"));
    }

    #[test]
    fn supports_placeholders_without_duplicate_flags() {
        let command = BackendCommand::new("python -m demo --ledger {ledger} --task {task} --stdio");
        let rendered = command.render(Path::new("/tmp/demo.sqlite"), Some("sample-task"));
        assert_eq!(rendered.matches("--ledger").count(), 1);
        assert!(rendered.contains("--task \"sample-task\""));
    }

    #[test]
    fn exchanges_requests_with_fake_backend() {
        let temp = tempdir().expect("tempdir should work");
        let ledger = temp.path().join("ledger.sqlite");
        let script = "python3 -u -c 'import json,sys; \nfor line in sys.stdin: \n req=json.loads(line); \n print(json.dumps({\"jsonrpc\":\"2.0\",\"id\":req.get(\"id\"),\"result\":{\"status\":\"ok\",\"echo\":req.get(\"method\")}}), flush=True); \n break' --ledger {ledger} --stdio";
        let (client, rx) = BackendClient::launch(BackendCommand::new(script), &ledger, None)
            .expect("backend launch should succeed");
        let request_id = client
            .request("ledger.health", serde_json::json!({}))
            .expect("request should write");
        let mut matched = false;
        for _ in 0..3 {
            match rx.recv().expect("event should arrive") {
                BackendEvent::Response(response) if response.id_string() == request_id => {
                    let payload: Value = response.into_result().expect("result should parse");
                    assert_eq!(payload["status"], "ok");
                    matched = true;
                    break;
                }
                BackendEvent::Exited(_) | BackendEvent::Log(_) | BackendEvent::Response(_) => {}
            }
        }
        assert!(matched, "expected rpc response from fake backend");
    }
}
