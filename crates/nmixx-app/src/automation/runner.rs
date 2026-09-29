use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Condvar, Mutex, atomic::{AtomicBool, AtomicU64, Ordering}, mpsc};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const MAX_SCRIPT: usize = 256 * 1024;
const MAX_LINE: usize = 1024 * 1024;
const MAX_REPLY: usize = 4 * 1024 * 1024;
const LOG_BUDGET: usize = 2 * 1024 * 1024;
const MAX_LOG_LINES: usize = 10_000;

pub trait AutomationApi: Send + Sync + 'static {
    fn cancellation(&self) -> Arc<AtomicBool> { Arc::new(AtomicBool::new(false)) }
    fn call(&self, method: &str, params: Value, control: &RunControl) -> Result<Value, String>;
    /// Always called, including script success, timeout, cancellation and panic.
    /// This closes task-owned operations, not unrelated GUI operations.
    fn finish(&self) -> Result<(), String> { Ok(()) }
    fn final_effects(&self) -> Vec<(String, Value)> { Vec::new() }
}

pub struct RunControl { pub(crate) cancelled: Arc<AtomicBool>, pub(crate) deadline: Instant }
impl RunControl {
    pub fn new(cancelled: Arc<AtomicBool>, timeout: Duration) -> Result<Self, String> {
        if timeout.is_zero() { return Err("Run deadline must be positive".into()); }
        let deadline = Instant::now().checked_add(timeout).ok_or("Run deadline overflow")?;
        Ok(Self { cancelled, deadline })
    }
    pub fn check(&self) -> Result<(), String> {
        if Instant::now() >= self.deadline { return Err("Automation deadline exceeded".into()); }
        if self.cancelled.load(Ordering::Acquire) { return Err("Automation was cancelled".into()); }
        Ok(())
    }
    pub fn wait(&self, duration: Duration) -> Result<(), String> {
        let end = Instant::now().checked_add(duration).ok_or("wait duration overflow")?.min(self.deadline);
        while Instant::now() < end {
            self.check()?;
            thread::sleep(Duration::from_millis(20).min(end.saturating_duration_since(Instant::now())));
        }
        self.check()
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptSpec {
    pub name: String,
    pub source: String,
    pub program: String,
    #[serde(default)] pub arguments: Vec<String>,
    pub working_directory: Option<String>,
    pub timeout_seconds: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AutomationState { Idle, Starting, Running, Cancelling, Completed, Cancelled, Failed }
impl AutomationState {
    pub fn active(self) -> bool { matches!(self, Self::Starting | Self::Running | Self::Cancelling) }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogLine { pub sequence: u64, pub elapsed_ms: u64, pub source: String, pub text: String }
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationEffect { pub sequence: u64, pub kind: String, pub payload: Value }
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationSnapshot {
    pub task_id: u64, pub state: AutomationState, pub name: String,
    pub exit_code: Option<i32>, pub message: Option<String>,
    pub first_sequence: u64, pub next_sequence: u64, pub logs: Vec<LogLine>,
    pub effects: Vec<OperationEffect>,
}
struct TaskData {
    state: AutomationState, exit_code: Option<i32>, message: Option<String>,
    logs: VecDeque<LogLine>, log_bytes: usize, next_sequence: u64,
    effects: std::collections::BTreeMap<String, OperationEffect>, next_effect: u64,
}
struct Task {
    id: u64, name: String, started: Instant, cancel: Arc<AtomicBool>,
    child: Mutex<Option<Arc<Mutex<Child>>>>, data: Mutex<TaskData>, changed: Condvar,
}
impl Task {
    fn effect(&self, kind: &str, payload: Value) {
        let mut data = self.data.lock().unwrap_or_else(|e| e.into_inner());
        data.next_effect += 1;
        let sequence = data.next_effect;
        data.effects.insert(kind.into(), OperationEffect { sequence, kind: kind.into(), payload });
    }
    fn kill_child(&self) {
        if let Some(child) = self.child.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            terminate(&mut child.lock().unwrap_or_else(|e| e.into_inner()));
        }
    }
    fn log(&self, source: &str, text: String) {
        let mut data = self.data.lock().unwrap_or_else(|e| e.into_inner());
        let sequence = data.next_sequence;
        data.next_sequence += 1;
        data.log_bytes += text.len();
        data.logs.push_back(LogLine { sequence, elapsed_ms: self.started.elapsed().as_millis() as u64,
            source: source.to_owned(), text });
        while data.log_bytes > LOG_BUDGET || data.logs.len() > MAX_LOG_LINES {
            if let Some(line) = data.logs.pop_front() { data.log_bytes -= line.text.len(); } else { break; }
        }
    }
    fn cancel(&self) {
        // Set the fence before killing: no next RPC may start after cancellation.
        {
            let mut data = self.data.lock().unwrap_or_else(|e| e.into_inner());
            if !data.state.active() { return; }
            self.cancel.store(true, Ordering::Release);
            data.state = AutomationState::Cancelling;
        }
        if let Some(child) = self.child.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            let mut child = child.lock().unwrap_or_else(|e| e.into_inner());
            terminate(&mut child);
        }
    }
    fn finish(&self, state: AutomationState, exit_code: Option<i32>, message: Option<String>) {
        let mut data = self.data.lock().unwrap_or_else(|e| e.into_inner());
        data.state = state; data.exit_code = exit_code; data.message = message;
        self.changed.notify_all();
    }
    fn snapshot(&self, after: u64) -> AutomationSnapshot {
        let data = self.data.lock().unwrap_or_else(|e| e.into_inner());
        AutomationSnapshot { task_id: self.id, state: data.state, name: self.name.clone(),
            exit_code: data.exit_code, message: data.message.clone(),
            first_sequence: data.logs.front().map(|line| line.sequence).unwrap_or(data.next_sequence),
            next_sequence: data.next_sequence,
            logs: data.logs.iter().filter(|line| line.sequence > after).cloned().collect(),
            effects: data.effects.values().cloned().collect() }
    }
}

#[derive(Default)]
struct RuntimeInner { next_id: AtomicU64, current: Mutex<Option<Arc<Task>>> }
impl Drop for RuntimeInner {
    fn drop(&mut self) {
        if let Some(task) = self.current.get_mut().unwrap_or_else(|e| e.into_inner()).as_ref() {
            task.cancel();
        }
    }
}
#[derive(Clone, Default)]
pub struct AutomationRuntime { inner: Arc<RuntimeInner> }
impl AutomationRuntime {
    pub fn start(&self, spec: ScriptSpec, api: Arc<dyn AutomationApi>) -> Result<AutomationSnapshot, String> {
        validate(&spec)?;
        let mut slot = self.inner.current.lock().map_err(|_| "automation lock poisoned")?;
        if slot.as_ref().is_some_and(|task| task.snapshot(u64::MAX).state.active()) {
            return Err("An Automation task is already active".to_owned());
        }
        let task = Arc::new(Task {
            id: self.inner.next_id.fetch_add(1, Ordering::Relaxed) + 1,
            name: spec.name.clone(), started: Instant::now(), cancel: api.cancellation(),
            child: Mutex::new(None), changed: Condvar::new(),
            data: Mutex::new(TaskData { state: AutomationState::Starting, exit_code: None, message: None,
                logs: VecDeque::new(), log_bytes: 0, next_sequence: 1,
                effects: Default::default(), next_effect: 0 }),
        });
        let worker = task.clone();
        thread::Builder::new().name("nmixx-automation".to_owned()).spawn(move || {
            let control = RunControl { cancelled: worker.cancel.clone(),
                deadline: worker.started + Duration::from_secs(spec.timeout_seconds) };
            let done = Arc::new(AtomicBool::new(false));
            let timed_out = Arc::new(AtomicBool::new(false));
            let watcher_done = done.clone();
            let watcher_timeout = timed_out.clone();
            let watcher_task = worker.clone();
            let deadline = control.deadline;
            let watcher = thread::spawn(move || {
                while !watcher_done.load(Ordering::Acquire) {
                    if Instant::now() >= deadline {
                        watcher_timeout.store(true, Ordering::Release);
                        watcher_task.cancel.store(true, Ordering::Release);
                    }
                    if watcher_task.cancel.load(Ordering::Acquire) { watcher_task.cancel(); }
                    thread::sleep(Duration::from_millis(20));
                }
            });
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(&worker, spec, api.as_ref(), &control)));
            done.store(true, Ordering::Release);
            let _ = watcher.join();
            let (code, failure) = match result {
                Ok(Ok(code)) => (code, None),
                Ok(Err(error)) => (None, Some(error)),
                Err(_) => (None, Some("Automation runner panicked".to_owned())),
            };
            worker.kill_child();
            worker.log("system", "Finishing task-owned operations through Application API".into());
            let cleanup = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| api.finish()));
            let cleanup_error = match cleanup { Ok(Ok(())) => None, Ok(Err(e)) => Some(e),
                Err(_) => Some("Application cleanup panicked".into()) };
            for (kind, payload) in api.final_effects() { worker.effect(&kind, payload); }
            // Drop the workflow lease before advertising terminal state, so the next
            // task cannot race the previous task's cleanup or retained connection.
            drop(api);
            let cancelled = worker.cancel.load(Ordering::Acquire);
            let timeout = timed_out.load(Ordering::Acquire);
            let state = if cleanup_error.is_some() || timeout { AutomationState::Failed }
                else if cancelled { AutomationState::Cancelled }
                else if failure.is_some() || code != Some(0) { AutomationState::Failed }
                else { AutomationState::Completed };
            let mut messages = Vec::new();
            if timeout { messages.push("Automation task deadline exceeded".to_owned()); }
            if let Some(e) = failure { messages.push(e); }
            if let Some(e) = cleanup_error { messages.push(format!("Cleanup failed: {e}")); }
            if state == AutomationState::Cancelled { messages.push("Cancelled; owned-operation cleanup completed".into()); }
            if state == AutomationState::Failed && messages.is_empty() { messages.push(format!("Script exit code: {code:?}")); }
            let message = if messages.is_empty() { None } else { Some(messages.join("; ")) };
            if let Some(message) = &message { worker.log("system", message.clone()); }
            worker.finish(state, code, message);
        }).map_err(|error| error.to_string())?;
        *slot = Some(task.clone());
        Ok(task.snapshot(0))
    }
    pub fn snapshot(&self, after: u64) -> Result<AutomationSnapshot, String> {
        let slot = self.inner.current.lock().map_err(|_| "automation lock poisoned")?;
        Ok(match slot.as_ref() {
            Some(task) => task.snapshot(after),
            None => AutomationSnapshot { task_id: 0, state: AutomationState::Idle, name: String::new(),
                exit_code: None, message: None, first_sequence: 1, next_sequence: 1, logs: Vec::new(), effects: Vec::new() },
        })
    }
    pub fn export_log(&self, path: &Path) -> Result<(), String> {
        let snapshot = self.snapshot(0)?;
        if snapshot.task_id == 0 || snapshot.state.active() {
            return Err("Finish or cancel the task before exporting its log".to_owned());
        }
        let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(path).map_err(|e| e.to_string())?;
        writeln!(file, "NMIXX Automation v1 task={} name={} state={:?} exit={:?}",
            snapshot.task_id, snapshot.name, snapshot.state, snapshot.exit_code).map_err(|e| e.to_string())?;
        if snapshot.first_sequence > 1 { writeln!(file, "Older output was truncated by the bounded log buffer.").map_err(|e| e.to_string())?; }
        for line in snapshot.logs {
            writeln!(file, "[{}ms {}] {}", line.elapsed_ms, line.source, line.text).map_err(|e| e.to_string())?;
        }
        if let Some(message) = snapshot.message { writeln!(file, "Result: {message}").map_err(|e| e.to_string())?; }
        file.flush().map_err(|e| e.to_string())
    }
    pub fn cancel(&self, task_id: u64) -> Result<(), String> {
        let slot = self.inner.current.lock().map_err(|_| "automation lock poisoned")?;
        let Some(task) = slot.as_ref() else { return Ok(()); };
        if task.id != task_id { return Err("The Automation task changed".to_owned()); }
        if task.snapshot(u64::MAX).state.active() { task.cancel(); }
        Ok(())
    }
    pub fn request_cancel(&self) -> Result<(), String> {
        let task = self.inner.current.lock().map_err(|_| "automation lock poisoned")?.clone();
        if let Some(task) = task { task.cancel(); }
        Ok(())
    }
    pub fn cancel_and_wait(&self, timeout: Duration) -> Result<(), String> {
        let task = self.inner.current.lock().map_err(|_| "automation lock poisoned")?.clone();
        let Some(task) = task else { return Ok(()); };
        if !task.snapshot(u64::MAX).state.active() { return Ok(()); }
        task.cancel();
        let data = task.data.lock().map_err(|_| "automation task lock poisoned")?;
        let (data, _) = task.changed.wait_timeout_while(data, timeout, |data| data.state.active())
            .map_err(|_| "automation task lock poisoned")?;
        if data.state.active() { Err("Automation is still finishing its call or owned-operation cleanup; connection retained".to_owned()) }
        else { Ok(()) }
    }
}

fn validate(spec: &ScriptSpec) -> Result<(), String> {
    if spec.source.is_empty() || spec.source.len() > MAX_SCRIPT { return Err("Script must contain 1..262144 UTF-8 bytes".to_owned()); }
    if spec.name.is_empty() || [".", ".."].contains(&spec.name.as_str()) || Path::new(&spec.name).file_name().and_then(|name| name.to_str()) != Some(spec.name.as_str()) {
        return Err("Script name must be a filename, not a path".to_owned());
    }
    if spec.program.trim().is_empty() || spec.program.len() > 4096 || spec.arguments.len() > 16 || spec.arguments.iter().any(|arg| arg.len() > 4096) {
        return Err("Invalid interpreter or argument list".to_owned());
    }
    if !(1..=3600).contains(&spec.timeout_seconds) { return Err("Task timeout must be 1..3600 seconds".to_owned()); }
    Ok(())
}
struct WorkDir(PathBuf);
impl WorkDir {
    fn new() -> Result<Self, String> {
        let root = std::env::temp_dir();
        for attempt in 0..32 {
            let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|e| e.to_string())?.as_nanos();
            let path = root.join(format!("nmixx-script-{}-{stamp}-{attempt}", std::process::id()));
            let mut builder = std::fs::DirBuilder::new();
            #[cfg(unix)] { use std::os::unix::fs::DirBuilderExt; builder.mode(0o700); }
            match builder.create(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error.to_string()),
            }
        }
        Err("Cannot allocate script working directory".to_owned())
    }
}
impl Drop for WorkDir { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); } }

// All child handles are reaped, including early protocol errors and panic unwinds.
struct ChildGuard { child: Arc<Mutex<Child>>, task: Arc<Task> }
impl Drop for ChildGuard {
    fn drop(&mut self) {
        // Remove the cancellation handle before reaping (including panic unwinds).
        *self.task.child.lock().unwrap_or_else(|e| e.into_inner()) = None;
        let mut child = self.child.lock().unwrap_or_else(|e| e.into_inner());
        if matches!(child.try_wait(), Ok(None)) { terminate(&mut child); }
        let _ = child.wait();
    }
}
fn terminate(child: &mut Child) {
    #[cfg(unix)] {
        // This process group was created by CommandExt::process_group(0).
        // Only our child and its non-detached descendants are targeted.
        unsafe { libc::kill(-(child.id() as i32), libc::SIGKILL); }
    }
    #[cfg(windows)] {
        let _ = Command::new("taskkill").args(["/PID", &child.id().to_string(), "/T", "/F"])
            .stdout(Stdio::null()).stderr(Stdio::null()).status();
    }
    let _ = child.kill();
}
enum Output { Line(&'static str, String), End, Error(String) }
fn read_pipe(pipe: impl Read, kind: &'static str, tx: mpsc::SyncSender<Output>) {
    let mut reader = BufReader::new(pipe);
    loop {
        let mut buffer = Vec::new();
        match (&mut reader).take(MAX_LINE as u64 + 1).read_until(b'\n', &mut buffer) {
            Ok(0) => break,
            Ok(_) if buffer.len() > MAX_LINE => { let _ = tx.send(Output::Error("Script output line exceeds 1 MiB".to_owned())); return; }
            Ok(_) => {
                let line = String::from_utf8_lossy(&buffer).trim_end_matches(['\n', '\r']).to_owned();
                if tx.send(Output::Line(kind, line)).is_err() { return; }
            }
            Err(error) => { let _ = tx.send(Output::Error(error.to_string())); return; }
        }
    }
    let _ = tx.send(Output::End);
}
fn reply(value: &Value, api: &dyn AutomationApi, control: &RunControl, task: &Task) -> Value {
    let id = value.get("id").cloned().unwrap_or(Value::Null);
    if value.get("jsonrpc").and_then(Value::as_str) != Some("2.0")
        || !id.is_u64() || value.get("method").and_then(Value::as_str).is_none()
        || !value.get("params").is_none_or(Value::is_object) {
        return json!({"jsonrpc":"2.0", "id":id, "error":{"code":-32600,"message":"Invalid JSON-RPC request"}});
    }
    let method = value["method"].as_str().unwrap();
    task.log("api", method.to_owned());
    match control.check().and_then(|_| api.call(method, value.get("params").cloned().unwrap_or_else(|| json!({})), control)) {
        Ok(result) => {
            if method == "config.save" {
                if let Some(values) = result.get("savedParameters") { task.effect("config.saved", values.clone()); }
            } else if ["scope.configure", "scope.live", "scope.stop", "scope.clear", "scope.capture"].contains(&method) {
                task.effect("scope.changed", result.clone());
            } else if method == "motion.set" { task.effect("motion.changed", result.clone()); }
            task.log("api", format!("{method}: returned"));
            json!({"jsonrpc":"2.0", "id":id, "result":result})
        },
        Err(message) => json!({"jsonrpc":"2.0", "id":id, "error":{"code":-32000,"message":message}}),
    }
}
fn run(task: &Arc<Task>, spec: ScriptSpec, api: &dyn AutomationApi, control: &RunControl) -> Result<Option<i32>, String> {
    let work = WorkDir::new()?;
    let script = work.0.join(format!("entry-{}", spec.name));
    std::fs::write(&script, spec.source).map_err(|e| e.to_string())?;
    std::fs::write(work.0.join("nmixx.py"), include_str!("assets/nmixx.py")).map_err(|e| e.to_string())?;
    let mut python_paths = vec![work.0.clone()];
    let mut command = Command::new(&spec.program);
    command.args(&spec.arguments).arg(&script)
        .env("NMIXX_AUTOMATION_PROTOCOL", "stdio-jsonrpc-v1").env("PYTHONUNBUFFERED", "1")
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    if let Some(directory) = &spec.working_directory {
        let directory = Path::new(directory).canonicalize().map_err(|e| e.to_string())?;
        if !directory.is_dir() { return Err("Working directory does not exist".to_owned()); }
        python_paths.push(directory.clone()); command.current_dir(directory);
    }
    if let Some(previous) = std::env::var_os("PYTHONPATH") { python_paths.extend(std::env::split_paths(&previous)); }
    command.env("PYTHONPATH", std::env::join_paths(python_paths).map_err(|e| e.to_string())?);
    #[cfg(unix)] { use std::os::unix::process::CommandExt; command.process_group(0); }
    if task.cancel.load(Ordering::Acquire) { return Ok(None); }
    let mut child = command.spawn().map_err(|e| format!("Cannot start '{}': {e}", spec.program))?;
    let mut input = child.stdin.take().ok_or("Missing child stdin")?;
    let stdout = child.stdout.take().ok_or("Missing child stdout")?;
    let stderr = child.stderr.take().ok_or("Missing child stderr")?;
    let child = Arc::new(Mutex::new(child));
    let guard = ChildGuard { child: child.clone(), task: task.clone() };
    *task.child.lock().map_err(|_| "child lock poisoned")? = Some(child.clone());
    if task.cancel.load(Ordering::Acquire) { task.cancel(); }
    {
        let mut data = task.data.lock().map_err(|_| "task lock poisoned")?;
        if !task.cancel.load(Ordering::Acquire) { data.state = AutomationState::Running; }
    }
    task.log("system", "Shared Application API; current connection reused. No CLI or second transport is opened.".to_owned());
    let (tx, rx) = mpsc::sync_channel(64);
    let stdout_tx = tx.clone();
    let stdout_thread = thread::spawn(move || read_pipe(stdout, "stdout", stdout_tx));
    let stderr_thread = thread::spawn(move || read_pipe(stderr, "stderr", tx));
    let deadline = task.started + Duration::from_secs(spec.timeout_seconds);
    let mut ends = 0;
    let result = (|| -> Result<Option<i32>, String> {
        loop {
            if task.cancel.load(Ordering::Acquire) { return Ok(None); }
            if Instant::now() >= deadline { return Err(format!("Task timed out after {} seconds", spec.timeout_seconds)); }
            // Only reap after pipe readers reached EOF. Cancellation can still
            // kill the child's group while descendants keep a pipe open.
            if ends == 2 {
                // With both pipes closed, no blocking pipe I/O remains. Remove the
                // kill handle before reaping so Cancel cannot signal a recycled PID.
                *task.child.lock().map_err(|_| "child lock poisoned")? = None;
                if let Some(status) = child.lock().map_err(|_| "child lock poisoned")?.try_wait().map_err(|e| e.to_string())? {
                    return Ok(status.code());
                }
            }
            match rx.recv_timeout(Duration::from_millis(20)) {
                Ok(Output::Line("stdout", line)) => {
                    let rpc = serde_json::from_str::<Value>(&line).ok().filter(|value| value.get("jsonrpc").is_some());
                    if let Some(value) = rpc {
                        if task.cancel.load(Ordering::Acquire) { continue; }
                        let response = reply(&value, api, control, task);
                        if task.cancel.load(Ordering::Acquire) { continue; }
                        let mut bytes = serde_json::to_vec(&response).map_err(|e| e.to_string())?;
                        if bytes.len() > MAX_REPLY { return Err("Application response exceeds 4 MiB".to_owned()); }
                        bytes.push(b'\n');
                        input.write_all(&bytes).and_then(|_| input.flush()).map_err(|e| e.to_string())?;
                    } else { task.log("stdout", line); }
                }
                Ok(Output::Line(kind, line)) => task.log(kind, line),
                Ok(Output::End) => ends += 1,
                Ok(Output::Error(error)) => return Err(error),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    if ends < 2 { return Err("Script output reader closed unexpectedly".to_owned()); }
                    thread::sleep(Duration::from_millis(20));
                }
            }
        }
    })();
    if result.is_err() || task.cancel.load(Ordering::Acquire) {
        // The leader has not been reaped on these paths unless both pipes closed.
        // Killing the group also releases pipe readers held by child subprocesses.
        if ends < 2 { terminate(&mut child.lock().unwrap_or_else(|e| e.into_inner())); }
    }
    drop(input);
    // Revoke/terminate before unblocking readers; no detached task may remain.
    drop(guard);
    *task.child.lock().unwrap_or_else(|e| e.into_inner()) = None;
    drop(rx);
    let _ = stdout_thread.join(); let _ = stderr_thread.join();
    result
}
