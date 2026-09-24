use std::io::Read;
use std::path::Path;
use std::sync::Arc;
use nmixx_app::{AutomationSnapshot, ScriptSpec, SessionWorkflowApi, DIAGNOSIS_SCRIPT, MOTION_SCRIPT};
use serde::Serialize;
use tauri::State;
use crate::DesktopState;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptDocument { name: String, source: String, working_directory: Option<String> }

#[tauri::command]
pub fn automation_builtin(kind: Option<String>) -> Result<ScriptDocument, String> {
    let (name, source) = match kind.as_deref().unwrap_or("diagnosis") {
        "diagnosis" => ("runtime_diagnosis.py", DIAGNOSIS_SCRIPT),
        "motion" => ("motion_workflow.py", MOTION_SCRIPT),
        _ => return Err("Unknown built-in workflow".into()),
    };
    Ok(ScriptDocument { name: name.into(), source: source.into(), working_directory: None })
}
#[tauri::command]
pub async fn automation_open_script(path: String) -> Result<ScriptDocument, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let path = Path::new(&path).canonicalize().map_err(|e| e.to_string())?;
        let name = path.file_name().and_then(|name| name.to_str()).ok_or("Invalid script filename")?.to_owned();
        let file = std::fs::File::open(&path).map_err(|e| e.to_string())?;
        if !file.metadata().map_err(|e| e.to_string())?.is_file() { return Err("Choose a regular script file".to_owned()); }
        let mut bytes = Vec::new();
        file.take(262145).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
        if bytes.is_empty() || bytes.len() > 262144 { return Err("Script must contain 1..262144 bytes".to_owned()); }
        let source = String::from_utf8(bytes).map_err(|_| "Script must be UTF-8")?;
        Ok(ScriptDocument { name, source, working_directory: path.parent().map(|path| path.to_string_lossy().into_owned()) })
    }).await.map_err(|e| e.to_string())?
}
#[tauri::command]
pub fn automation_start(state: State<'_, std::sync::Mutex<DesktopState>>, spec: ScriptSpec) -> Result<AutomationSnapshot, String> {
    // Hold DesktopState through start so Disconnect cannot miss a just-starting task.
    let guard = state.lock().map_err(|_| "desktop state is poisoned")?;
    if guard.disconnecting { return Err("Device disconnection is in progress".to_owned()); }
    let app = guard.app.clone().ok_or("Connect a device before running a workflow")?;
    if app.is_closing() { return Err("Device session is closing".to_owned()); }
    guard.automation.start(spec, Arc::new(SessionWorkflowApi::new(app)?))
}
#[tauri::command]
pub fn automation_snapshot(state: State<'_, std::sync::Mutex<DesktopState>>, after: u64) -> Result<AutomationSnapshot, String> {
    state.lock().map_err(|_| "desktop state is poisoned")?.automation.snapshot(after)
}
#[tauri::command]
pub fn automation_cancel(state: State<'_, std::sync::Mutex<DesktopState>>, task_id: u64) -> Result<(), String> {
    state.lock().map_err(|_| "desktop state is poisoned")?.automation.cancel(task_id)
}

#[tauri::command]
pub async fn automation_export_log(state: State<'_, std::sync::Mutex<DesktopState>>, path: String) -> Result<(), String> {
    let automation = state.lock().map_err(|_| "desktop state is poisoned")?.automation.clone();
    tauri::async_runtime::spawn_blocking(move || automation.export_log(Path::new(&path)))
        .await.map_err(|e| e.to_string())?
}
