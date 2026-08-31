use std::{
    collections::HashMap,
    io::{BufRead, BufReader, BufWriter, Write},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    time::Duration,
};

use serde_json::{Value, json};

use super::RemoteError;

type Reply = std::result::Result<Value, RemoteError>;

#[derive(Clone)]
pub(super) struct Transport {
    inner: Arc<Inner>,
}

struct Inner {
    input: Mutex<BufWriter<ChildStdin>>,
    child: Mutex<Child>,
    pending: Mutex<HashMap<u64, mpsc::Sender<Reply>>>,
    stderr: Arc<Mutex<String>>,
    next_id: AtomicU64,
    connected: AtomicBool,
    timeout: Duration,
}

impl Transport {
    pub(super) fn from_command(command: Command, timeout: Duration) -> Result<Self, RemoteError> {
        // Kept separate from JSON-RPC payloads so `Command` stays injectable in tests.
        let mut command = command;
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command
            .spawn()
            .map_err(|e| RemoteError::Connect(e.to_string()))?;
        let input = child
            .stdin
            .take()
            .ok_or_else(|| RemoteError::Connect("stdin unavailable".into()))?;
        let output = child
            .stdout
            .take()
            .ok_or_else(|| RemoteError::Connect("stdout unavailable".into()))?;
        let stderr_pipe = child
            .stderr
            .take()
            .ok_or_else(|| RemoteError::Connect("stderr unavailable".into()))?;
        let stderr = Arc::new(Mutex::new(String::new()));
        let inner = Arc::new(Inner {
            input: Mutex::new(BufWriter::new(input)),
            child: Mutex::new(child),
            pending: Mutex::new(HashMap::new()),
            stderr: stderr.clone(),
            next_id: AtomicU64::new(1),
            connected: AtomicBool::new(false),
            timeout,
        });
        spawn_stderr_reader(stderr_pipe, stderr);
        spawn_response_reader(output, inner.clone());
        Ok(Self { inner })
    }

    pub(super) fn request(&self, method: &str, params: Value) -> Reply {
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel();
        self.inner.pending.lock().unwrap().insert(id, tx);
        let request = json!({"jsonrpc":"2.0", "id":id, "method":method, "params":params});
        let written = self
            .inner
            .input
            .lock()
            .map_err(|_| "stdin lock poisoned".to_string())
            .and_then(|mut input| {
                serde_json::to_writer(&mut *input, &request).map_err(|e| e.to_string())?;
                input
                    .write_all(b"\n")
                    .and_then(|_| input.flush())
                    .map_err(|e| e.to_string())
            });
        if let Err(message) = written {
            self.inner.pending.lock().unwrap().remove(&id);
            return Err(self.failure(message));
        }
        match rx.recv_timeout(self.inner.timeout) {
            Ok(reply) => reply,
            Err(_) => {
                self.inner.pending.lock().unwrap().remove(&id);
                if self.inner.connected.load(Ordering::Acquire) {
                    Err(RemoteError::Timeout(method.into()))
                } else {
                    if let Ok(mut child) = self.inner.child.lock() {
                        let _ = child.kill();
                    }
                    Err(RemoteError::startup(
                        &self.stderr(),
                        format!("{method} timed out"),
                    ))
                }
            }
        }
    }

    pub(super) fn mark_connected(&self) {
        self.inner.connected.store(true, Ordering::Release);
    }

    pub(super) fn terminate(&self) {
        if let Ok(mut child) = self.inner.child.lock() {
            let _ = child.kill();
        }
    }

    fn stderr(&self) -> String {
        self.inner
            .stderr
            .lock()
            .map(|s| s.clone())
            .unwrap_or_default()
    }

    fn failure(&self, fallback: String) -> RemoteError {
        if self.inner.connected.load(Ordering::Acquire) {
            RemoteError::Dropped(if self.stderr().trim().is_empty() {
                fallback
            } else {
                self.stderr()
            })
        } else {
            RemoteError::startup(&self.stderr(), fallback)
        }
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        if let Ok(child) = self.child.get_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn spawn_stderr_reader(stderr: impl std::io::Read + Send + 'static, shared: Arc<Mutex<String>>) {
    std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            if let Ok(mut captured) = shared.lock() {
                if !captured.is_empty() {
                    captured.push('\n');
                }
                captured.push_str(&line);
            }
        }
    });
}

fn spawn_response_reader(output: impl std::io::Read + Send + 'static, inner: Arc<Inner>) {
    std::thread::spawn(move || {
        for line in BufReader::new(output).lines() {
            let Ok(line) = line else { break };
            let Ok(value) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            let Some(id) = value.get("id").and_then(Value::as_u64) else {
                continue;
            };
            if let Some(sender) = inner.pending.lock().unwrap().remove(&id) {
                let reply = if let Some(error) = value.get("error") {
                    Err(RemoteError::Protocol(error.to_string()))
                } else {
                    Ok(value.get("result").cloned().unwrap_or(Value::Null))
                };
                let _ = sender.send(reply);
            }
        }
        let exit = inner
            .child
            .lock()
            .ok()
            .and_then(|mut child| child.try_wait().ok().flatten())
            .map(|status| format!("remote process exited with {status}"))
            .unwrap_or_else(|| "remote process exited".into());
        let stderr = inner
            .stderr
            .lock()
            .map(|value| value.clone())
            .unwrap_or_default();
        let detail = if stderr.trim().is_empty() {
            exit
        } else {
            stderr
        };
        let error = if inner.connected.load(Ordering::Acquire) {
            RemoteError::Dropped(detail)
        } else {
            RemoteError::startup(&detail, "remote process exited")
        };
        for (_, sender) in inner.pending.lock().unwrap().drain() {
            let _ = sender.send(Err(error.clone()));
        }
    });
}
