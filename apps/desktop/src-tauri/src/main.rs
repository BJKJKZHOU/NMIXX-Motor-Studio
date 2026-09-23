use std::sync::Mutex;
use std::time::Duration;
use nmixx_app::{
    ActionHandle, ActionMetadata, ApplicationSession, AxdrStatus, DEFAULT_USB_BAUD, HostSchema,
    IdentificationKind, IdentificationStart, MixedScopeSeries, MotionCapabilities, MotionConfig, MotionPreview,
    ParameterMetadata, ParameterValue, PositionValue, PreflightDomain, RangeMetadata,
    SchemaNumber, ScopeRate, ScopeSelection, StreamState, TuningExperimentState,
};
use serde::{Deserialize, Serialize};
use tauri::{Emitter, State};

#[derive(Default)]
struct DesktopState { app: Option<ApplicationSession>, port: Option<String> }
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PlotChannelDto { id: u16, label: String, unit: Option<String>, supports_fast: bool, supports_normal: bool, fast_scale: Option<f32> }
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ConnectionDto {
    port: String, fast_max_channels: u8, normal_max_channels: u8, fast_block_samples: u8,
    fast_rate_hz: u32, normal_rate_hz: u32, channels: Vec<PlotChannelDto>, motion: MotionCapabilities,
    runtime_channel_ids: Vec<u16>,
}
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(untagged)]
enum SchemaNumberDto { Integer(i64), Float(f64) }
impl From<SchemaNumber> for SchemaNumberDto {
    fn from(value: SchemaNumber) -> Self { match value { SchemaNumber::Integer(value) => Self::Integer(value), SchemaNumber::Float(value) => Self::Float(value) } }
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ParameterRangeDto {
    min: Option<SchemaNumberDto>, max: Option<SchemaNumberDto>, exclusive_min: bool, exclusive_max: bool,
    max_symbol: Option<String>, min_binding: Option<String>, max_binding: Option<String>, max_bindings: Vec<String>,
}
impl From<&RangeMetadata> for ParameterRangeDto {
    fn from(value: &RangeMetadata) -> Self {
        Self { min: value.min.map(Into::into), max: value.max.map(Into::into), exclusive_min: value.exclusive_min,
            exclusive_max: value.exclusive_max, max_symbol: value.max_symbol.clone(), min_binding: value.min_binding.clone(),
            max_binding: value.max_binding.clone(), max_bindings: value.max_bindings.clone() }
    }
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ParameterMetadataDto {
    id: u16, symbol: String, label: String, type_name: String, access: String, unit: Option<String>, description: String,
    write_state: Option<String>, range: Option<ParameterRangeDto>, allowed: Vec<SchemaNumberDto>, allowed_symbols: Vec<String>,
}
impl From<&ParameterMetadata> for ParameterMetadataDto {
    fn from(value: &ParameterMetadata) -> Self {
        Self { id: value.id, symbol: value.symbol.clone(), label: value.label.clone(), type_name: value.type_name.clone(),
            access: value.access.clone(), unit: value.unit.clone(), description: value.description.clone(),
            write_state: value.write_state.clone(), range: value.range.as_ref().map(Into::into),
            allowed: value.allowed.iter().copied().map(Into::into).collect(), allowed_symbols: value.allowed_symbols.clone() }
    }
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ActionMetadataDto { id: u16, symbol: String, label: String, description: String }
impl From<&ActionMetadata> for ActionMetadataDto {
    fn from(value: &ActionMetadata) -> Self { Self { id: value.id, symbol: value.symbol.clone(), label: value.label.clone(), description: value.description.clone() } }
}
#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum IdentificationStartDto { Blocked { issues: Vec<PreflightIssueDto> }, RequiresEnable, Started { handle: ActionHandleDto } }
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ActionHandleDto { txn: u8, action_id: u16, symbol: String }
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ActionCompletionDto { txn: u8, action_id: u16, symbol: String, status: String, ok: bool }
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PreflightIssueDto { parameter_id: Option<u16>, reason: String, suggested_domain: &'static str }
fn preflight_domain_name(domain: PreflightDomain) -> &'static str {
    match domain { PreflightDomain::LimitsSafety => "limits", PreflightDomain::Motor => "motor", PreflightDomain::Encoder => "encoder", PreflightDomain::Identification => "identification" }
}
fn preflight_issue_dto(issue: nmixx_app::PreflightIssue) -> PreflightIssueDto {
    PreflightIssueDto { parameter_id: issue.parameter_id, reason: issue.reason, suggested_domain: preflight_domain_name(issue.suggested_domain) }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PositionValueDto { turns: i32, theta: f32 }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
enum ParameterValueDto { U8(u8), I8(i8), F32(f32), I32(i32), U32(u32), Position(PositionValueDto) }
impl From<ParameterValue> for ParameterValueDto {
    fn from(value: ParameterValue) -> Self {
        match value {
            ParameterValue::U8(value) => Self::U8(value), ParameterValue::I8(value) => Self::I8(value),
            ParameterValue::F32(value) => Self::F32(value), ParameterValue::I32(value) => Self::I32(value),
            ParameterValue::U32(value) => Self::U32(value),
            ParameterValue::Position(value) => Self::Position(PositionValueDto { turns: value.turns, theta: value.theta }),
        }
    }
}
impl From<ParameterValueDto> for ParameterValue {
    fn from(value: ParameterValueDto) -> Self {
        match value {
            ParameterValueDto::U8(value) => Self::U8(value), ParameterValueDto::I8(value) => Self::I8(value),
            ParameterValueDto::F32(value) => Self::F32(value), ParameterValueDto::I32(value) => Self::I32(value),
            ParameterValueDto::U32(value) => Self::U32(value),
            ParameterValueDto::Position(value) => Self::Position(PositionValue { turns: value.turns, theta: value.theta }),
        }
    }
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ParameterReadDto { id: u16, value: ParameterValueDto }
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ParameterReadResultDto { id: u16, value: Option<ParameterValueDto>, error: Option<String> }
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ScopeSelectionDto { id: u16, rate: String }
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ScopeConfigDto { history_seconds: f64, channels: Vec<ScopeChannelDto> }
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ScopeChannelDto { id: u16, label: String, unit: Option<String>, rate: &'static str, sample_rate_hz: u32 }
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ScopeStatusDto { state: &'static str, samples: usize, capacity_samples: usize, lost_frames: u64 }
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ScopeSeriesDto {
    id: u16, sample_rate_hz: u32, times: Vec<f64>, values: Vec<f32>,
    #[serde(skip_serializing_if = "Option::is_none")] envelope_min: Option<Vec<f32>>,
    #[serde(skip_serializing_if = "Option::is_none")] envelope_max: Option<Vec<f32>>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ScopeSnapshotDto { sample_count: usize, recorded_seconds: f64, lost_frames: u64, state: &'static str, series: Vec<ScopeSeriesDto> }

fn scope_series_dto(series: MixedScopeSeries, end_offset_seconds: f64, max_points: usize) -> ScopeSeriesDto {
    let sample_count = series.values.len();
    let rate = f64::from(series.sample_rate_hz);
    if sample_count <= max_points {
        let times = (0..sample_count).map(|index| (index as f64 - sample_count.saturating_sub(1) as f64) / rate - end_offset_seconds).collect();
        return ScopeSeriesDto { id: series.id, sample_rate_hz: series.sample_rate_hz, times, values: series.values, envelope_min: None, envelope_max: None };
    }
    // Min/max downsampling preserves narrow spikes instead of periodically
    // sampling one arbitrary point from each bucket.
    let target_buckets = (max_points / 2).max(1);
    let bucket_size = sample_count.div_ceil(target_buckets).max(1);
    let mut times = Vec::with_capacity(target_buckets * 2);
    let mut values = Vec::with_capacity(target_buckets * 2);
    for start in (0..sample_count).step_by(bucket_size) {
        let end = (start + bucket_size).min(sample_count);
        let bucket = &series.values[start..end];
        let mut min_index = start;
        let mut max_index = start;
        let mut min_value = f32::INFINITY;
        let mut max_value = f32::NEG_INFINITY;
        for (offset, &value) in bucket.iter().enumerate() {
            let index = start + offset;
            if value < min_value { min_value = value; min_index = index; }
            if value > max_value { max_value = value; max_index = index; }
        }
        let mut extrema = [(min_index, min_value), (max_index, max_value)];
        extrema.sort_by_key(|(index, _)| *index);
        for (index, value) in extrema {
            if times.len() >= max_points { break; }
            times.push((index as f64 - sample_count.saturating_sub(1) as f64) / rate - end_offset_seconds);
            values.push(value);
        }
    }
    ScopeSeriesDto { id: series.id, sample_rate_hz: series.sample_rate_hz, times, values, envelope_min: None, envelope_max: None }
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TuningExperimentStatusDto { state: &'static str, message: Option<String> }
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TuningExperimentSnapshotDto {
    status: TuningExperimentStatusDto, config: ScopeConfigDto, snapshot: ScopeSnapshotDto,
    recorded_seconds: f64, window_seconds: f64, end_offset_seconds: f64,
}
fn stream_state_name(state: StreamState) -> &'static str {
    match state { StreamState::Stopped => "STOPPED", StreamState::Live => "LIVE", StreamState::Capturing => "CAPTURING", StreamState::Paused => "PAUSED" }
}
fn tuning_experiment_state_name(state: TuningExperimentState) -> &'static str {
    match state { TuningExperimentState::Idle => "IDLE", TuningExperimentState::Preparing => "PREPARING",
        TuningExperimentState::Running => "RUNNING", TuningExperimentState::Stopping => "STOPPING",
        TuningExperimentState::Completed => "COMPLETED", TuningExperimentState::Failed => "FAILED" }
}
fn application(state: &State<'_, Mutex<DesktopState>>) -> Result<ApplicationSession, String> {
    state.lock().map_err(|_| "desktop state is poisoned".to_owned())?.app.clone().ok_or_else(|| "device is not connected".to_owned())
}
fn parameter_result(id: u16, result: Result<ParameterValue, impl ToString>) -> ParameterReadResultDto {
    match result {
        Ok(value) => ParameterReadResultDto { id, value: Some(value.into()), error: None },
        Err(error) => ParameterReadResultDto { id, value: None, error: Some(error.to_string()) },
    }
}
fn spawn_parameter_change_bridge(app_handle: tauri::AppHandle, changes: std::sync::mpsc::Receiver<Vec<u16>>) {
    std::thread::spawn(move || {
        while let Ok(ids) = changes.recv() { if !ids.is_empty() { let _ = app_handle.emit("parameters-changed", ids); } }
    });
}
fn spawn_action_completion(app_handle: tauri::AppHandle, events: std::sync::mpsc::Receiver<nmixx_app::SessionEvent>, handle: ActionHandle, symbol: String) {
    std::thread::spawn(move || {
        while let Ok(event) = events.recv() {
            let nmixx_app::SessionEvent::ActionCompleted { handle: completed, status } = event else { continue; };
            if completed != handle { continue; }
            // Application already updated the Parameter cache before forwarding this event.
            let payload = ActionCompletionDto { txn: completed.txn.get(), action_id: completed.action_id, symbol,
                status: format!("{status:?}"), ok: status == AxdrStatus::Ok };
            let _ = app_handle.emit("action-completed", payload);
            break;
        }
    });
}
fn action_handle_dto(handle: ActionHandle, symbol: String) -> ActionHandleDto { ActionHandleDto { txn: handle.txn.get(), action_id: handle.action_id, symbol } }

#[tauri::command]
fn device_list() -> Result<Vec<String>, String> { ApplicationSession::available_usb_ports().map_err(|error| error.to_string()) }
#[tauri::command]
async fn device_connect(app_handle: tauri::AppHandle, state: State<'_, Mutex<DesktopState>>, port: String, schema_path: String, baud: Option<u32>) -> Result<ConnectionDto, String> {
    disconnect_application(&state)?;
    let schema = HostSchema::load(&schema_path).map_err(|error| error.to_string())?;
    let app = ApplicationSession::open_usb(&port, baud.unwrap_or(DEFAULT_USB_BAUD), schema).map_err(|error| error.to_string())?;
    for (id, result) in app.parameter_refresh_all().map_err(|error| error.to_string())? {
        let label = app.parameter_metadata().iter().find(|meta| meta.id == id).map(|meta| meta.label.as_str()).unwrap_or("Parameter");
        result.map_err(|error| format!("Initial read of {label} failed: {error}"))?;
    }
    let parameter_changes = app.parameter_subscribe().map_err(|error| error.to_string())?;
    spawn_parameter_change_bridge(app_handle, parameter_changes);
    let capabilities = app.plot_capabilities().map_err(|error| error.to_string())?;
    let channels = capabilities.with_schema(app.schema()).into_iter().map(|channel| PlotChannelDto {
        id: channel.id, label: channel.label.unwrap_or_else(|| format!("0x{:04X}", channel.id)), unit: channel.unit,
        supports_fast: channel.supports_fast, supports_normal: channel.supports_normal, fast_scale: channel.fast_scale,
    }).collect();
    let runtime_channel_ids = app.runtime_start().map_err(|error| error.to_string())?;
    let result = ConnectionDto { port: port.clone(), fast_max_channels: capabilities.fast_max_channels,
        normal_max_channels: capabilities.normal_max_channels, fast_block_samples: capabilities.fast_block_samples,
        fast_rate_hz: capabilities.fast_rate_hz, normal_rate_hz: capabilities.normal_rate_hz, channels, motion: app.motion_capabilities().clone(), runtime_channel_ids };
    let mut guard = state.lock().map_err(|_| "desktop state is poisoned".to_owned())?;
    guard.app = Some(app); guard.port = Some(port);
    Ok(result)
}
#[tauri::command]
async fn device_disconnect(state: State<'_, Mutex<DesktopState>>) -> Result<(), String> {
    disconnect_application(&state)
}
fn disconnect_application(state: &State<'_, Mutex<DesktopState>>) -> Result<(), String> {
    let app = state.lock().map_err(|_| "desktop state is poisoned".to_owned())?.app.clone();
    let Some(app) = app else { return Ok(()); };
    app.disconnect().map_err(|error| error.to_string())?;
    let removed = {
        let mut guard = state.lock().map_err(|_| "desktop state is poisoned".to_owned())?;
        if guard.app.as_ref().is_some_and(|current| current.is_same_session(&app)) {
            guard.port = None;
            guard.app.take()
        } else { None }
    };
    drop(removed);
    Ok(())
}
#[tauri::command]
fn parameter_list(state: State<'_, Mutex<DesktopState>>) -> Result<Vec<ParameterMetadataDto>, String> { Ok(application(&state)?.parameter_metadata().iter().map(Into::into).collect()) }
#[tauri::command]
async fn parameter_read(state: State<'_, Mutex<DesktopState>>, id: u16) -> Result<ParameterReadDto, String> {
    let value = application(&state)?.parameter_read(id).map_err(|error| error.to_string())?;
    Ok(ParameterReadDto { id, value: value.into() })
}
#[tauri::command]
async fn parameter_read_many(state: State<'_, Mutex<DesktopState>>, ids: Vec<u16>) -> Result<Vec<ParameterReadResultDto>, String> {
    Ok(application(&state)?.parameter_read_many(&ids).map_err(|error| error.to_string())?.into_iter().map(|(id, result)| parameter_result(id, result)).collect())
}
#[tauri::command]
async fn parameter_write(state: State<'_, Mutex<DesktopState>>, id: u16, value: ParameterValueDto) -> Result<ParameterReadDto, String> {
    let value = application(&state)?.parameter_write(id, value.into()).map_err(|error| error.to_string())?;
    Ok(ParameterReadDto { id, value: value.into() })
}
#[tauri::command]
fn parameter_cached_many(state: State<'_, Mutex<DesktopState>>, ids: Vec<u16>) -> Result<Vec<ParameterReadResultDto>, String> {
    Ok(application(&state)?.parameter_cached_many(&ids).map_err(|error| error.to_string())?.into_iter().map(|(id, result)| parameter_result(id, result)).collect())
}
#[tauri::command]
async fn parameter_refresh_all(state: State<'_, Mutex<DesktopState>>) -> Result<Vec<ParameterReadResultDto>, String> {
    Ok(application(&state)?.parameter_refresh_all().map_err(|error| error.to_string())?.into_iter().map(|(id, result)| parameter_result(id, result)).collect())
}
#[tauri::command]
async fn phase_search_preflight(state: State<'_, Mutex<DesktopState>>) -> Result<Vec<PreflightIssueDto>, String> {
    Ok(application(&state)?.preflight_phase_search().map_err(|error| error.to_string())?.into_iter().map(preflight_issue_dto).collect())
}
fn identification_kind(kind: &str) -> Result<IdentificationKind, String> {
    match kind { "rs_ls" => Ok(IdentificationKind::RsLs), "flux" => Ok(IdentificationKind::Flux), "jb" => Ok(IdentificationKind::Jb), other => Err(format!("unknown identification kind '{other}'")) }
}
#[tauri::command]
async fn identification_preflight(state: State<'_, Mutex<DesktopState>>, kind: String) -> Result<Vec<PreflightIssueDto>, String> {
    Ok(application(&state)?.preflight_identification(identification_kind(&kind)?).map_err(|error| error.to_string())?.into_iter().map(preflight_issue_dto).collect())
}
#[tauri::command]
async fn identification_start(app_handle: tauri::AppHandle, state: State<'_, Mutex<DesktopState>>, kind: String, allow_enable: bool) -> Result<IdentificationStartDto, String> {
    let kind_value = identification_kind(&kind)?;
    let symbol = match kind_value { IdentificationKind::RsLs => "ACTION_IDENT_RS_LS_START", IdentificationKind::Flux => "ACTION_IDENT_FLUX_START", IdentificationKind::Jb => "ACTION_IDENT_JB_START" };
    let app = application(&state)?;
    let events = app.subscribe().map_err(|error| error.to_string())?;
    match app.identification_start(kind_value, allow_enable).map_err(|error| error.to_string())? {
        IdentificationStart::Blocked(issues) => Ok(IdentificationStartDto::Blocked { issues: issues.into_iter().map(preflight_issue_dto).collect() }),
        IdentificationStart::RequiresEnable => Ok(IdentificationStartDto::RequiresEnable),
        IdentificationStart::Started(handle) => {
            let dto = action_handle_dto(handle, symbol.to_owned());
            spawn_action_completion(app_handle, events, handle, symbol.to_owned());
            Ok(IdentificationStartDto::Started { handle: dto })
        }
    }
}
#[tauri::command]
async fn identification_apply(state: State<'_, Mutex<DesktopState>>) -> Result<ActionHandleDto, String> {
    Ok(action_handle_dto(application(&state)?.identification_apply().map_err(|error| error.to_string())?, "ACTION_IDENT_APPLY".to_owned()))
}
#[tauri::command]
fn action_list(state: State<'_, Mutex<DesktopState>>) -> Result<Vec<ActionMetadataDto>, String> { Ok(application(&state)?.schema().actions.iter().map(Into::into).collect()) }
#[tauri::command]
async fn action_start(app_handle: tauri::AppHandle, state: State<'_, Mutex<DesktopState>>, key: String) -> Result<ActionHandleDto, String> {
    let app = application(&state)?;
    let symbol = app.schema().action_by_key(&key).ok_or_else(|| format!("Action '{key}' is not exposed by the HostSchema"))?.symbol.clone();
    let events = app.subscribe().map_err(|error| error.to_string())?;
    let handle = app.action_start(&key).map_err(|error| error.to_string())?;
    let result = action_handle_dto(handle, symbol.clone());
    spawn_action_completion(app_handle, events, handle, symbol);
    Ok(result)
}
#[tauri::command]
async fn action_start_immediate(state: State<'_, Mutex<DesktopState>>, key: String) -> Result<ActionHandleDto, String> {
    let app = application(&state)?;
    let symbol = app.schema().action_by_key(&key).ok_or_else(|| format!("Action '{key}' is not exposed by the HostSchema"))?.symbol.clone();
    let handle = app.action_start_immediate(&key).map_err(|error| error.to_string())?;
    Ok(action_handle_dto(handle, symbol))
}
fn start_async_semantic_action(app_handle: tauri::AppHandle, app: ApplicationSession, symbol: &str,
    start: impl FnOnce(&ApplicationSession) -> Result<ActionHandle, nmixx_app::ApplicationError>) -> Result<ActionHandleDto, String> {
    let events = app.subscribe().map_err(|error| error.to_string())?;
    let handle = start(&app).map_err(|error| error.to_string())?;
    let result = action_handle_dto(handle, symbol.to_owned());
    spawn_action_completion(app_handle, events, handle, symbol.to_owned());
    Ok(result)
}
#[tauri::command]
async fn motor_enable(state: State<'_, Mutex<DesktopState>>) -> Result<ActionHandleDto, String> {
    Ok(action_handle_dto(application(&state)?.motor_enable().map_err(|error| error.to_string())?, "ACTION_MOTOR_ENABLE".to_owned()))
}
#[tauri::command]
async fn motor_stop(app_handle: tauri::AppHandle, state: State<'_, Mutex<DesktopState>>) -> Result<ActionHandleDto, String> {
    let handle = application(&state)?.motor_stop().map_err(|error| error.to_string())?;
    let _ = app_handle.emit("motor-stop-issued", ());
    Ok(action_handle_dto(handle, "ACTION_MOTOR_STOP".to_owned()))
}
#[tauri::command]
async fn motor_disable(state: State<'_, Mutex<DesktopState>>) -> Result<ActionHandleDto, String> {
    Ok(action_handle_dto(application(&state)?.motor_disable().map_err(|error| error.to_string())?, "ACTION_MOTOR_DISABLE".to_owned()))
}
#[tauri::command]
fn config_save_available(state: State<'_, Mutex<DesktopState>>) -> Result<bool, String> { Ok(application(&state)?.config_save_available()) }
#[tauri::command]
async fn config_save(state: State<'_, Mutex<DesktopState>>) -> Result<ActionHandleDto, String> {
    Ok(action_handle_dto(application(&state)?.config_save().map_err(|error| error.to_string())?, "ACTION_PARAMETER_SAVE".to_owned()))
}
#[tauri::command]
async fn phase_search_start(app_handle: tauri::AppHandle, state: State<'_, Mutex<DesktopState>>) -> Result<ActionHandleDto, String> {
    start_async_semantic_action(app_handle, application(&state)?, "ACTION_PHASE_SEARCH_START", |app| app.phase_search_start())
}
#[tauri::command]
fn motion_get(state: State<'_, Mutex<DesktopState>>) -> Result<MotionConfig, String> { Ok(application(&state)?.motion_get()) }
#[tauri::command]
fn motion_set(state: State<'_, Mutex<DesktopState>>, config: MotionConfig) -> Result<MotionConfig, String> { application(&state)?.motion_set(config).map_err(|error| error.to_string()) }
#[tauri::command]
async fn motion_preview(state: State<'_, Mutex<DesktopState>>) -> Result<MotionPreview, String> { application(&state)?.motion_preview().map_err(|error| error.to_string()) }
#[tauri::command]
async fn motion_run(state: State<'_, Mutex<DesktopState>>) -> Result<(), String> { application(&state)?.motion_run().map(|_| ()).map_err(|error| error.to_string()) }
#[tauri::command]
async fn motion_stop(app_handle: tauri::AppHandle, state: State<'_, Mutex<DesktopState>>) -> Result<(), String> {
    application(&state)?.motion_stop().map_err(|error| error.to_string())?;
    let _ = app_handle.emit("motor-stop-issued", ()); Ok(())
}
fn scope_selection_from_dto(selection: ScopeSelectionDto) -> Result<ScopeSelection, String> {
    let rate = match selection.rate.as_str() { "fast" => ScopeRate::Fast, "normal" => ScopeRate::Normal, other => return Err(format!("unknown Scope rate '{other}'")) };
    Ok(ScopeSelection { id: selection.id, rate })
}
fn scope_config_dto(config: &nmixx_app::MixedScopeConfig) -> ScopeConfigDto {
    ScopeConfigDto { history_seconds: config.history.as_secs_f64(), channels: config.channels.iter().map(|channel| ScopeChannelDto {
        id: channel.id, label: channel.label.clone(), unit: channel.unit.clone(),
        rate: match channel.rate { ScopeRate::Fast => "fast", ScopeRate::Normal => "normal" }, sample_rate_hz: channel.sample_rate_hz,
    }).collect() }
}
fn tuning_status_dto(status: nmixx_app::TuningExperimentStatus) -> TuningExperimentStatusDto {
    TuningExperimentStatusDto { state: tuning_experiment_state_name(status.state), message: status.message }
}
#[tauri::command]
fn tuning_experiment_defaults(state: State<'_, Mutex<DesktopState>>) -> Result<Vec<ScopeSelectionDto>, String> {
    Ok(application(&state)?.tuning_experiment_default_selections().map_err(|error| error.to_string())?.into_iter().map(|selection| ScopeSelectionDto {
        id: selection.id, rate: match selection.rate { ScopeRate::Fast => "fast".to_owned(), ScopeRate::Normal => "normal".to_owned() },
    }).collect())
}
#[tauri::command]
async fn tuning_experiment_start(state: State<'_, Mutex<DesktopState>>, selections: Vec<ScopeSelectionDto>) -> Result<TuningExperimentStatusDto, String> {
    let selections = selections.into_iter().map(scope_selection_from_dto).collect::<Result<Vec<_>, _>>()?;
    Ok(tuning_status_dto(application(&state)?.tuning_experiment_start(&selections).map_err(|error| error.to_string())?))
}
#[tauri::command]
async fn tuning_experiment_stop(app_handle: tauri::AppHandle, state: State<'_, Mutex<DesktopState>>) -> Result<TuningExperimentStatusDto, String> {
    let status = application(&state)?.tuning_experiment_stop().map_err(|error| error.to_string())?;
    let _ = app_handle.emit("motor-stop-issued", ()); Ok(tuning_status_dto(status))
}
#[tauri::command]
fn tuning_experiment_status(state: State<'_, Mutex<DesktopState>>) -> Result<TuningExperimentStatusDto, String> {
    Ok(tuning_status_dto(application(&state)?.tuning_experiment_status().map_err(|error| error.to_string())?))
}
#[tauri::command]
fn tuning_experiment_snapshot(state: State<'_, Mutex<DesktopState>>, window_seconds: Option<f64>, end_offset_seconds: Option<f64>, max_points: Option<usize>) -> Result<TuningExperimentSnapshotDto, String> {
    let app = application(&state)?;
    let status = app.tuning_experiment_status().map_err(|error| error.to_string())?;
    let config = app.tuning_record_config().map_err(|error| error.to_string())?;
    let scope_status = app.tuning_record_status().map_err(|error| error.to_string())?;
    let recorded_seconds = app.tuning_record_duration().map_err(|error| error.to_string())?.as_secs_f64();
    let default_window = if matches!(status.state, TuningExperimentState::Preparing | TuningExperimentState::Running | TuningExperimentState::Stopping) { 3.0 } else { recorded_seconds.max(0.0005) };
    let window_seconds = window_seconds.unwrap_or(default_window).clamp(0.0005, recorded_seconds.max(0.0005));
    let max_offset = (recorded_seconds - window_seconds).max(0.0);
    let end_offset_seconds = end_offset_seconds.unwrap_or(0.0).clamp(0.0, max_offset);
    let snapshot = app.tuning_record_window(Duration::from_secs_f64(window_seconds), Duration::from_secs_f64(end_offset_seconds)).map_err(|error| error.to_string())?;
    let max_points = max_points.unwrap_or(3000).clamp(200, 20_000);
    let series = snapshot.series.into_iter().map(|series| scope_series_dto(series, end_offset_seconds, max_points)).collect();
    Ok(TuningExperimentSnapshotDto {
        status: tuning_status_dto(status), config: scope_config_dto(&config),
        snapshot: ScopeSnapshotDto { sample_count: scope_status.samples, recorded_seconds, lost_frames: snapshot.lost_frames, state: stream_state_name(snapshot.state), series },
        recorded_seconds, window_seconds, end_offset_seconds,
    })
}
#[tauri::command]
async fn scope_configure(state: State<'_, Mutex<DesktopState>>, selections: Vec<ScopeSelectionDto>, history_seconds: Option<f64>) -> Result<ScopeConfigDto, String> {
    let history_seconds = history_seconds.unwrap_or(10.0);
    if !history_seconds.is_finite() || history_seconds <= 0.0 { return Err("historySeconds must be positive and finite".to_owned()); }
    let selections = selections.into_iter().map(scope_selection_from_dto).collect::<Result<Vec<_>, _>>()?;
    let config = application(&state)?.scope_configure(&selections, Duration::from_secs_f64(history_seconds), 1).map_err(|error| error.to_string())?;
    Ok(scope_config_dto(&config))
}
#[tauri::command]
async fn scope_live(state: State<'_, Mutex<DesktopState>>) -> Result<(), String> { application(&state)?.scope_live().map_err(|error| error.to_string()) }
#[tauri::command]
async fn scope_pause(state: State<'_, Mutex<DesktopState>>) -> Result<(), String> { application(&state)?.scope_pause().map_err(|error| error.to_string()) }
#[tauri::command]
async fn scope_stop(state: State<'_, Mutex<DesktopState>>) -> Result<(), String> { application(&state)?.scope_stop().map_err(|error| error.to_string()) }
#[tauri::command]
fn scope_clear(state: State<'_, Mutex<DesktopState>>) -> Result<(), String> { application(&state)?.scope_clear().map_err(|error| error.to_string()) }
#[tauri::command]
fn scope_status(state: State<'_, Mutex<DesktopState>>) -> Result<ScopeStatusDto, String> {
    let status = application(&state)?.scope_status().map_err(|error| error.to_string())?;
    Ok(ScopeStatusDto { state: stream_state_name(status.state), samples: status.samples, capacity_samples: status.capacity_samples, lost_frames: status.lost_frames })
}
#[tauri::command]
fn scope_snapshot(state: State<'_, Mutex<DesktopState>>, window_seconds: Option<f64>, end_offset_seconds: Option<f64>, max_points: Option<usize>) -> Result<ScopeSnapshotDto, String> {
    let app = application(&state)?;
    let recorded_seconds = app.scope_recorded_duration().map_err(|error| error.to_string())?.as_secs_f64();
    let window = window_seconds.unwrap_or(0.5).clamp(0.0005, recorded_seconds.max(0.0005));
    let max_offset = (recorded_seconds - window).max(0.0);
    let end_offset = end_offset_seconds.unwrap_or(0.0).clamp(0.0, max_offset);
    let snapshot = app.scope_snapshot_window(Duration::from_secs_f64(window), Duration::from_secs_f64(end_offset)).map_err(|error| error.to_string())?;
    let max_points = max_points.unwrap_or(2500).clamp(100, 10_000);
    let sample_count = snapshot.series.iter().map(|series| series.values.len()).max().unwrap_or(0);
    let series = snapshot.series.into_iter().map(|series| scope_series_dto(series, end_offset, max_points)).collect();
    Ok(ScopeSnapshotDto { sample_count, recorded_seconds, lost_frames: snapshot.lost_frames, state: stream_state_name(snapshot.state), series })
}
fn main() {
    tauri::Builder::default().manage(Mutex::new(DesktopState::default()))
        .invoke_handler(tauri::generate_handler![
            device_list, device_connect, device_disconnect, parameter_list, parameter_read, parameter_read_many,
            parameter_cached_many, parameter_refresh_all, parameter_write, phase_search_preflight,
            identification_preflight, identification_start, identification_apply, action_list, action_start,
            action_start_immediate, motor_enable, motor_stop, motor_disable, config_save_available, config_save,
            phase_search_start, motion_get, motion_set, motion_preview, motion_run, motion_stop,
            tuning_experiment_defaults, tuning_experiment_start, tuning_experiment_stop, tuning_experiment_status,
            tuning_experiment_snapshot, scope_configure, scope_live, scope_pause, scope_stop, scope_clear, scope_status, scope_snapshot,
        ])
        .run(tauri::generate_context!())
        .expect("error while running NMIXX Motor Studio");
}
