use std::sync::Mutex;
use std::time::Duration;

use nmixx_app::{
    ActionHandle, ActionMetadata, ApplicationSession, AxdrStatus, DEFAULT_USB_BAUD, HostSchema,
    IdentificationKind, MotionCapabilities, MotionConfig, MotionPreview, MotionService,
    ParameterMetadata, ParameterValue, PositionValue, PreflightDomain, RangeMetadata,
    SchemaNumber, ScopeRate, ScopeSelection, StreamState,
};
use serde::{Deserialize, Serialize};
use tauri::{Emitter, State};

#[derive(Default)]
struct DesktopState {
    app: Option<ApplicationSession>,
    motion: MotionService,
    port: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PlotChannelDto {
    id: u16,
    symbol: String,
    unit: Option<String>,
    supports_fast: bool,
    supports_normal: bool,
    fast_scale: Option<f32>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ConnectionDto {
    port: String,
    fast_max_channels: u8,
    normal_max_channels: u8,
    fast_block_samples: u8,
    fast_rate_hz: u32,
    normal_rate_hz: u32,
    channels: Vec<PlotChannelDto>,
    motion: MotionCapabilities,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(untagged)]
enum SchemaNumberDto {
    Integer(i64),
    Float(f64),
}

impl From<SchemaNumber> for SchemaNumberDto {
    fn from(value: SchemaNumber) -> Self {
        match value {
            SchemaNumber::Integer(value) => Self::Integer(value),
            SchemaNumber::Float(value) => Self::Float(value),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ParameterRangeDto {
    min: Option<SchemaNumberDto>,
    max: Option<SchemaNumberDto>,
    exclusive_min: bool,
    exclusive_max: bool,
    max_symbol: Option<String>,
    max_binding: Option<String>,
    max_bindings: Vec<String>,
}

impl From<&RangeMetadata> for ParameterRangeDto {
    fn from(value: &RangeMetadata) -> Self {
        Self {
            min: value.min.map(Into::into),
            max: value.max.map(Into::into),
            exclusive_min: value.exclusive_min,
            exclusive_max: value.exclusive_max,
            max_symbol: value.max_symbol.clone(),
            max_binding: value.max_binding.clone(),
            max_bindings: value.max_bindings.clone(),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ParameterMetadataDto {
    id: u16,
    symbol: String,
    name: Option<String>,
    type_name: String,
    access: String,
    unit: Option<String>,
    description: String,
    write_state: Option<String>,
    range: Option<ParameterRangeDto>,
    allowed: Vec<SchemaNumberDto>,
    allowed_symbols: Vec<String>,
}

impl From<&ParameterMetadata> for ParameterMetadataDto {
    fn from(value: &ParameterMetadata) -> Self {
        Self {
            id: value.id,
            symbol: value.symbol.clone(),
            name: value.name.clone(),
            type_name: value.type_name.clone(),
            access: value.access.clone(),
            unit: value.unit.clone(),
            description: value.description.clone(),
            write_state: value.write_state.clone(),
            range: value.range.as_ref().map(Into::into),
            allowed: value.allowed.iter().copied().map(Into::into).collect(),
            allowed_symbols: value.allowed_symbols.clone(),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ActionMetadataDto {
    id: u16,
    symbol: String,
    name: Option<String>,
    description: String,
}

impl From<&ActionMetadata> for ActionMetadataDto {
    fn from(value: &ActionMetadata) -> Self {
        Self {
            id: value.id,
            symbol: value.symbol.clone(),
            name: value.name.clone(),
            description: value.description.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ActionHandleDto {
    txn: u8,
    action_id: u16,
    symbol: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ActionCompletionDto {
    txn: u8,
    action_id: u16,
    symbol: String,
    status: String,
    ok: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PreflightIssueDto {
    parameter_id: Option<u16>,
    reason: String,
    suggested_domain: &'static str,
}

fn preflight_domain_name(domain: PreflightDomain) -> &'static str {
    match domain {
        PreflightDomain::LimitsSafety => "limits",
        PreflightDomain::Motor => "motor",
        PreflightDomain::Encoder => "encoder",
        PreflightDomain::Identification => "identification",
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PositionValueDto {
    turns: i32,
    theta: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
enum ParameterValueDto {
    U8(u8),
    I8(i8),
    F32(f32),
    I32(i32),
    U32(u32),
    Position(PositionValueDto),
}

impl From<ParameterValue> for ParameterValueDto {
    fn from(value: ParameterValue) -> Self {
        match value {
            ParameterValue::U8(value) => Self::U8(value),
            ParameterValue::I8(value) => Self::I8(value),
            ParameterValue::F32(value) => Self::F32(value),
            ParameterValue::I32(value) => Self::I32(value),
            ParameterValue::U32(value) => Self::U32(value),
            ParameterValue::Position(value) => Self::Position(PositionValueDto {
                turns: value.turns,
                theta: value.theta,
            }),
        }
    }
}

impl From<ParameterValueDto> for ParameterValue {
    fn from(value: ParameterValueDto) -> Self {
        match value {
            ParameterValueDto::U8(value) => Self::U8(value),
            ParameterValueDto::I8(value) => Self::I8(value),
            ParameterValueDto::F32(value) => Self::F32(value),
            ParameterValueDto::I32(value) => Self::I32(value),
            ParameterValueDto::U32(value) => Self::U32(value),
            ParameterValueDto::Position(value) => Self::Position(PositionValue {
                turns: value.turns,
                theta: value.theta,
            }),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ParameterReadDto {
    id: u16,
    value: ParameterValueDto,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ParameterReadResultDto {
    id: u16,
    value: Option<ParameterValueDto>,
    error: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScopeSelectionDto {
    id: u16,
    rate: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ScopeConfigDto {
    history_seconds: f64,
    channels: Vec<ScopeChannelDto>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ScopeChannelDto {
    id: u16,
    symbol: String,
    unit: Option<String>,
    rate: &'static str,
    sample_rate_hz: u32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ScopeStatusDto {
    state: &'static str,
    samples: usize,
    capacity_samples: usize,
    lost_frames: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ScopeSeriesDto {
    id: u16,
    sample_rate_hz: u32,
    times: Vec<f64>,
    values: Vec<f32>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ScopeSnapshotDto {
    sample_count: usize,
    lost_frames: u64,
    state: &'static str,
    series: Vec<ScopeSeriesDto>,
}

fn stream_state_name(state: StreamState) -> &'static str {
    match state {
        StreamState::Stopped => "STOPPED",
        StreamState::Live => "LIVE",
        StreamState::Capturing => "CAPTURING",
        StreamState::Paused => "PAUSED",
    }
}

fn application(state: &State<'_, Mutex<DesktopState>>) -> Result<ApplicationSession, String> {
    state
        .lock()
        .map_err(|_| "desktop state is poisoned".to_owned())?
        .app
        .clone()
        .ok_or_else(|| "device is not connected".to_owned())
}

fn parameter_result(id: u16, result: Result<ParameterValue, impl ToString>) -> ParameterReadResultDto {
    match result {
        Ok(value) => ParameterReadResultDto {
            id,
            value: Some(value.into()),
            error: None,
        },
        Err(error) => ParameterReadResultDto {
            id,
            value: None,
            error: Some(error.to_string()),
        },
    }
}

fn spawn_action_completion(
    app: tauri::AppHandle,
    events: std::sync::mpsc::Receiver<nmixx_app::SessionEvent>,
    handle: ActionHandle,
    symbol: String,
) {
    std::thread::spawn(move || {
        while let Ok(event) = events.recv() {
            let nmixx_app::SessionEvent::ActionCompleted {
                handle: completed,
                status,
            } = event
            else {
                continue;
            };
            if completed != handle {
                continue;
            }

            let payload = ActionCompletionDto {
                txn: completed.txn.get(),
                action_id: completed.action_id,
                symbol,
                status: format!("{status:?}"),
                ok: status == AxdrStatus::Ok,
            };
            let _ = app.emit("action-completed", payload);
            break;
        }
    });
}

fn action_handle_dto(handle: ActionHandle, symbol: String) -> ActionHandleDto {
    ActionHandleDto {
        txn: handle.txn.get(),
        action_id: handle.action_id,
        symbol,
    }
}

#[tauri::command]
fn device_list() -> Result<Vec<String>, String> {
    ApplicationSession::available_usb_ports().map_err(|error| error.to_string())
}

#[tauri::command]
fn device_connect(
    state: State<'_, Mutex<DesktopState>>,
    port: String,
    schema_path: String,
    baud: Option<u32>,
) -> Result<ConnectionDto, String> {
    let old_app = {
        let mut guard = state.lock().map_err(|_| "desktop state is poisoned".to_owned())?;
        guard.port = None;
        guard.app.take()
    };
    drop(old_app);

    let schema = HostSchema::load(&schema_path).map_err(|error| error.to_string())?;
    let motion = state
        .lock()
        .map_err(|_| "desktop state is poisoned".to_owned())?
        .motion
        .clone();
    let app = ApplicationSession::open_usb_with_motion(
        &port,
        baud.unwrap_or(DEFAULT_USB_BAUD),
        schema,
        motion,
    )
    .map_err(|error| error.to_string())?;

    let _ = app.parameter_refresh_all().map_err(|error| error.to_string())?;
    let capabilities = app.plot_capabilities().map_err(|error| error.to_string())?;
    let channels = capabilities
        .with_schema(app.schema())
        .into_iter()
        .map(|channel| PlotChannelDto {
            id: channel.id,
            symbol: channel
                .name
                .or(channel.symbol)
                .unwrap_or_else(|| format!("0x{:04X}", channel.id)),
            unit: channel.unit,
            supports_fast: channel.supports_fast,
            supports_normal: channel.supports_normal,
            fast_scale: channel.fast_scale,
        })
        .collect();

    let result = ConnectionDto {
        port: port.clone(),
        fast_max_channels: capabilities.fast_max_channels,
        normal_max_channels: capabilities.normal_max_channels,
        fast_block_samples: capabilities.fast_block_samples,
        fast_rate_hz: capabilities.fast_rate_hz,
        normal_rate_hz: capabilities.normal_rate_hz,
        channels,
        motion: app.motion_capabilities().clone(),
    };

    let mut guard = state.lock().map_err(|_| "desktop state is poisoned".to_owned())?;
    guard.app = Some(app);
    guard.port = Some(port);
    Ok(result)
}

#[tauri::command]
fn device_disconnect(state: State<'_, Mutex<DesktopState>>) -> Result<(), String> {
    let app = {
        let mut guard = state.lock().map_err(|_| "desktop state is poisoned".to_owned())?;
        guard.port = None;
        guard.app.take()
    };
    drop(app);
    Ok(())
}

#[tauri::command]
fn parameter_list(state: State<'_, Mutex<DesktopState>>) -> Result<Vec<ParameterMetadataDto>, String> {
    let app = application(&state)?;
    Ok(app.parameter_metadata().iter().map(Into::into).collect())
}

#[tauri::command]
fn parameter_read(state: State<'_, Mutex<DesktopState>>, id: u16) -> Result<ParameterReadDto, String> {
    let app = application(&state)?;
    let value = app.parameter_read(id).map_err(|error| error.to_string())?;
    Ok(ParameterReadDto { id, value: value.into() })
}

#[tauri::command]
fn parameter_read_many(
    state: State<'_, Mutex<DesktopState>>,
    ids: Vec<u16>,
) -> Result<Vec<ParameterReadResultDto>, String> {
    let app = application(&state)?;
    let values = app.parameter_read_many(&ids).map_err(|error| error.to_string())?;
    Ok(values
        .into_iter()
        .map(|(id, result)| match result {
            Ok(value) => ParameterReadResultDto {
                id,
                value: Some(value.into()),
                error: None,
            },
            Err(error) => ParameterReadResultDto {
                id,
                value: None,
                error: Some(error.to_string()),
            },
        })
        .collect())
}

#[tauri::command]
fn parameter_write(
    state: State<'_, Mutex<DesktopState>>,
    id: u16,
    value: ParameterValueDto,
) -> Result<(), String> {
    application(&state)?
        .parameter_write(id, value.into())
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn parameter_cached_many(
    state: State<'_, Mutex<DesktopState>>,
    ids: Vec<u16>,
) -> Result<Vec<ParameterReadResultDto>, String> {
    let app = application(&state)?;
    Ok(ids
        .into_iter()
        .map(|id| match app.parameter_cached(id) {
            Ok(Some(value)) => ParameterReadResultDto {
                id,
                value: Some(value.into()),
                error: None,
            },
            Ok(None) => ParameterReadResultDto {
                id,
                value: None,
                error: Some("parameter has not been read into the shared cache".to_owned()),
            },
            Err(error) => ParameterReadResultDto {
                id,
                value: None,
                error: Some(error.to_string()),
            },
        })
        .collect())
}

#[tauri::command]
fn parameter_refresh_all(
    app_handle: tauri::AppHandle,
    state: State<'_, Mutex<DesktopState>>,
) -> Result<Vec<ParameterReadResultDto>, String> {
    let app = application(&state)?;
    let values = app.parameter_refresh_all().map_err(|error| error.to_string())?;
    let result = values
        .into_iter()
        .map(|(id, result)| parameter_result(id, result))
        .collect::<Vec<_>>();
    let _ = app_handle.emit("parameters-refreshed", ());
    Ok(result)
}

#[tauri::command]
fn phase_search_preflight(
    state: State<'_, Mutex<DesktopState>>,
) -> Result<Vec<PreflightIssueDto>, String> {
    let issues = application(&state)?
        .preflight_phase_search()
        .map_err(|error| error.to_string())?;
    Ok(issues
        .into_iter()
        .map(|issue| PreflightIssueDto {
            parameter_id: issue.parameter_id,
            reason: issue.reason,
            suggested_domain: preflight_domain_name(issue.suggested_domain),
        })
        .collect())
}

#[tauri::command]
fn identification_preflight(
    state: State<'_, Mutex<DesktopState>>,
    kind: String,
) -> Result<Vec<PreflightIssueDto>, String> {
    let kind = match kind.as_str() {
        "rs_ls" => IdentificationKind::RsLs,
        "flux" => IdentificationKind::Flux,
        "jb" => IdentificationKind::Jb,
        other => return Err(format!("unknown identification preflight kind '{other}'")),
    };
    let issues = application(&state)?
        .preflight_identification(kind)
        .map_err(|error| error.to_string())?;
    Ok(issues
        .into_iter()
        .map(|issue| PreflightIssueDto {
            parameter_id: issue.parameter_id,
            reason: issue.reason,
            suggested_domain: preflight_domain_name(issue.suggested_domain),
        })
        .collect())
}

#[tauri::command]
fn action_list(state: State<'_, Mutex<DesktopState>>) -> Result<Vec<ActionMetadataDto>, String> {
    let app = application(&state)?;
    Ok(app.schema().actions.iter().map(Into::into).collect())
}

#[tauri::command]
fn action_start(
    app_handle: tauri::AppHandle,
    state: State<'_, Mutex<DesktopState>>,
    key: String,
) -> Result<ActionHandleDto, String> {
    let app = application(&state)?;
    let symbol = app
        .schema()
        .action_by_key(&key)
        .ok_or_else(|| format!("Action '{key}' is not exposed by the HostSchema"))?
        .symbol
        .clone();
    let events = app.subscribe().map_err(|error| error.to_string())?;
    let handle = app.action_start(&key).map_err(|error| error.to_string())?;
    let result = action_handle_dto(handle, symbol.clone());
    spawn_action_completion(app_handle, events, handle, symbol);
    Ok(result)
}

fn start_semantic_action(
    app_handle: tauri::AppHandle,
    app: ApplicationSession,
    symbol: &str,
    start: impl FnOnce(&ApplicationSession) -> Result<ActionHandle, nmixx_app::ApplicationError>,
) -> Result<ActionHandleDto, String> {
    let events = app.subscribe().map_err(|error| error.to_string())?;
    let handle = start(&app).map_err(|error| error.to_string())?;
    let symbol = symbol.to_owned();
    let result = action_handle_dto(handle, symbol.clone());
    spawn_action_completion(app_handle, events, handle, symbol);
    Ok(result)
}

#[tauri::command]
fn motor_enable(
    app_handle: tauri::AppHandle,
    state: State<'_, Mutex<DesktopState>>,
) -> Result<ActionHandleDto, String> {
    start_semantic_action(app_handle, application(&state)?, "ACTION_MOTOR_ENABLE", |app| app.motor_enable())
}

#[tauri::command]
fn motor_stop(
    app_handle: tauri::AppHandle,
    state: State<'_, Mutex<DesktopState>>,
) -> Result<ActionHandleDto, String> {
    start_semantic_action(app_handle, application(&state)?, "ACTION_MOTOR_STOP", |app| app.motor_stop())
}

#[tauri::command]
fn motor_disable(
    app_handle: tauri::AppHandle,
    state: State<'_, Mutex<DesktopState>>,
) -> Result<ActionHandleDto, String> {
    start_semantic_action(app_handle, application(&state)?, "ACTION_MOTOR_DISABLE", |app| app.motor_disable())
}

#[tauri::command]
fn config_save_available(state: State<'_, Mutex<DesktopState>>) -> Result<bool, String> {
    Ok(application(&state)?.config_save_available())
}

#[tauri::command]
fn config_save(
    app_handle: tauri::AppHandle,
    state: State<'_, Mutex<DesktopState>>,
) -> Result<ActionHandleDto, String> {
    start_semantic_action(app_handle, application(&state)?, "ACTION_PARAMETER_SAVE", |app| app.config_save())
}

#[tauri::command]
fn phase_search_start(
    app_handle: tauri::AppHandle,
    state: State<'_, Mutex<DesktopState>>,
) -> Result<ActionHandleDto, String> {
    start_semantic_action(app_handle, application(&state)?, "ACTION_PHASE_SEARCH_START", |app| app.phase_search_start())
}

#[tauri::command]
fn motion_get(state: State<'_, Mutex<DesktopState>>) -> Result<MotionConfig, String> {
    let guard = state.lock().map_err(|_| "desktop state is poisoned".to_owned())?;
    Ok(guard.motion.get())
}

#[tauri::command]
fn motion_set(
    state: State<'_, Mutex<DesktopState>>,
    config: MotionConfig,
) -> Result<MotionConfig, String> {
    let guard = state.lock().map_err(|_| "desktop state is poisoned".to_owned())?;
    guard.motion.set(config)
}

#[tauri::command]
fn motion_preview(state: State<'_, Mutex<DesktopState>>) -> Result<MotionPreview, String> {
    let guard = state.lock().map_err(|_| "desktop state is poisoned".to_owned())?;
    guard.motion.preview()
}

#[tauri::command]
fn motion_run(state: State<'_, Mutex<DesktopState>>) -> Result<(), String> {
    application(&state)?
        .motion_run()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn motion_stop(state: State<'_, Mutex<DesktopState>>) -> Result<(), String> {
    application(&state)?
        .motion_stop()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn scope_configure(
    state: State<'_, Mutex<DesktopState>>,
    selections: Vec<ScopeSelectionDto>,
    history_seconds: Option<f64>,
) -> Result<ScopeConfigDto, String> {
    let history_seconds = history_seconds.unwrap_or(10.0);
    if !history_seconds.is_finite() || history_seconds <= 0.0 {
        return Err("historySeconds must be positive and finite".to_owned());
    }

    let selections = selections
        .into_iter()
        .map(|selection| {
            let rate = match selection.rate.as_str() {
                "fast" => Ok(ScopeRate::Fast),
                "normal" => Ok(ScopeRate::Normal),
                other => Err(format!("unknown Scope rate '{other}'")),
            }?;
            Ok(ScopeSelection {
                id: selection.id,
                rate,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;

    let app = application(&state)?;
    let config = app
        .scope_configure(
            &selections,
            Duration::from_secs_f64(history_seconds),
            1,
        )
        .map_err(|error| error.to_string())?;

    Ok(ScopeConfigDto {
        history_seconds: config.history.as_secs_f64(),
        channels: config
            .channels
            .iter()
            .map(|channel| ScopeChannelDto {
                id: channel.id,
                symbol: channel.symbol.clone(),
                unit: channel.unit.clone(),
                rate: match channel.rate {
                    ScopeRate::Fast => "fast",
                    ScopeRate::Normal => "normal",
                },
                sample_rate_hz: channel.sample_rate_hz,
            })
            .collect(),
    })
}

#[tauri::command]
fn scope_live(state: State<'_, Mutex<DesktopState>>) -> Result<(), String> {
    application(&state)?.scope_live().map_err(|error| error.to_string())
}

#[tauri::command]
fn scope_pause(state: State<'_, Mutex<DesktopState>>) -> Result<(), String> {
    application(&state)?.scope_pause().map_err(|error| error.to_string())
}

#[tauri::command]
fn scope_stop(state: State<'_, Mutex<DesktopState>>) -> Result<(), String> {
    application(&state)?.scope_stop().map_err(|error| error.to_string())
}

#[tauri::command]
fn scope_clear(state: State<'_, Mutex<DesktopState>>) -> Result<(), String> {
    application(&state)?.scope_clear().map_err(|error| error.to_string())
}

#[tauri::command]
fn scope_status(state: State<'_, Mutex<DesktopState>>) -> Result<ScopeStatusDto, String> {
    let status = application(&state)?.scope_status().map_err(|error| error.to_string())?;
    Ok(ScopeStatusDto {
        state: stream_state_name(status.state),
        samples: status.samples,
        capacity_samples: status.capacity_samples,
        lost_frames: status.lost_frames,
    })
}

#[tauri::command]
fn scope_snapshot(
    state: State<'_, Mutex<DesktopState>>,
    window_seconds: Option<f64>,
    end_offset_seconds: Option<f64>,
    max_points: Option<usize>,
) -> Result<ScopeSnapshotDto, String> {
    let app = application(&state)?;
    let status = app.scope_status().map_err(|error| error.to_string())?;
    let config = app.scope_config().map_err(|error| error.to_string())?;
    let window = window_seconds.unwrap_or(0.5).clamp(0.0005, config.history.as_secs_f64());
    let max_offset = (config.history.as_secs_f64() - window).max(0.0);
    let end_offset = end_offset_seconds.unwrap_or(0.0).clamp(0.0, max_offset);
    let snapshot = app
        .scope_snapshot_window(
            Duration::from_secs_f64(window),
            Duration::from_secs_f64(end_offset),
        )
        .map_err(|error| error.to_string())?;
    let max_points = max_points.unwrap_or(2500).clamp(100, 10_000);

    let series = snapshot
        .series
        .into_iter()
        .map(|series| {
            let sample_count = series.values.len();
            let stride = sample_count.div_ceil(max_points).max(1);
            let mut times = Vec::with_capacity(sample_count.div_ceil(stride));
            let mut values = Vec::with_capacity(sample_count.div_ceil(stride));

            for index in (0..sample_count).step_by(stride) {
                let t = (index as f64 - sample_count.saturating_sub(1) as f64)
                    / f64::from(series.sample_rate_hz)
                    - end_offset;
                times.push(t);
                values.push(series.values[index]);
            }

            ScopeSeriesDto {
                id: series.id,
                sample_rate_hz: series.sample_rate_hz,
                times,
                values,
            }
        })
        .collect();

    Ok(ScopeSnapshotDto {
        sample_count: status.samples,
        lost_frames: snapshot.lost_frames,
        state: stream_state_name(snapshot.state),
        series,
    })
}

fn main() {
    tauri::Builder::default()
        .manage(Mutex::new(DesktopState::default()))
        .invoke_handler(tauri::generate_handler![
            device_list,
            device_connect,
            device_disconnect,
            parameter_list,
            parameter_read,
            parameter_read_many,
            parameter_cached_many,
            parameter_refresh_all,
            parameter_write,
            phase_search_preflight,
            identification_preflight,
            action_list,
            action_start,
            motor_enable,
            motor_stop,
            motor_disable,
            config_save_available,
            config_save,
            phase_search_start,
            motion_get,
            motion_set,
            motion_preview,
            motion_run,
            motion_stop,
            scope_configure,
            scope_live,
            scope_pause,
            scope_stop,
            scope_clear,
            scope_status,
            scope_snapshot,
        ])
        .run(tauri::generate_context!())
        .expect("error while running NMIXX Motor Studio");
}
