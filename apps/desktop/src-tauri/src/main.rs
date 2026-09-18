use std::sync::Mutex;
use std::time::Duration;

use nmixx_app::{
    DEFAULT_USB_BAUD, DevicePlotCapabilities, DeviceSession, HostSchema, MotionCapabilities,
    MotionConfig, MotionPreview, MotionService, ParameterMetadata, ParameterService, ParameterValue,
    PositionValue, RangeMetadata, SchemaNumber, ScopeSession, StreamState,
};
use serde::{Deserialize, Serialize};
use tauri::State;

#[derive(Default)]
struct DesktopState {
    session: Option<DeviceSession>,
    schema: Option<HostSchema>,
    parameters: Option<ParameterService>,
    capabilities: Option<DevicePlotCapabilities>,
    scope: Option<ScopeSession>,
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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ScopeConfigDto {
    sample_rate_hz: u32,
    history_seconds: f64,
    channels: Vec<ScopeChannelDto>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ScopeChannelDto {
    id: u16,
    symbol: String,
    unit: Option<String>,
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
struct ScopeSnapshotDto {
    sample_rate_hz: u32,
    sample_count: usize,
    lost_frames: u64,
    state: &'static str,
    times: Vec<f64>,
    series: Vec<Vec<f32>>,
}

fn stream_state_name(state: StreamState) -> &'static str {
    match state {
        StreamState::Stopped => "STOPPED",
        StreamState::Live => "LIVE",
        StreamState::Capturing => "CAPTURING",
        StreamState::Paused => "PAUSED",
    }
}

fn parameter_service(state: &State<'_, Mutex<DesktopState>>) -> Result<ParameterService, String> {
    state
        .lock()
        .map_err(|_| "desktop state is poisoned".to_owned())?
        .parameters
        .clone()
        .ok_or_else(|| "device is not connected".to_owned())
}

#[tauri::command]
fn device_list() -> Result<Vec<String>, String> {
    DeviceSession::available_usb_ports().map_err(|error| error.to_string())
}

#[tauri::command]
fn device_connect(
    state: State<'_, Mutex<DesktopState>>,
    port: String,
    schema_path: String,
    baud: Option<u32>,
) -> Result<ConnectionDto, String> {
    let (old_scope, old_session) = {
        let mut guard = state.lock().map_err(|_| "desktop state is poisoned".to_owned())?;
        let scope = guard.scope.take();
        let session = guard.session.take();
        guard.schema = None;
        guard.parameters = None;
        guard.capabilities = None;
        guard.port = None;
        (scope, session)
    };
    drop(old_scope);
    drop(old_session);

    let schema = HostSchema::load(&schema_path).map_err(|error| error.to_string())?;
    let session = DeviceSession::open_usb(&port, baud.unwrap_or(DEFAULT_USB_BAUD))
        .map_err(|error| error.to_string())?;
    let capabilities = DevicePlotCapabilities::discover(&session).map_err(|error| error.to_string())?;
    let parameters = ParameterService::new(session.clone(), schema.clone());

    let channels = capabilities
        .with_schema(&schema)
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

    let motion = MotionCapabilities::from_schema(&schema);
    let result = ConnectionDto {
        port: port.clone(),
        fast_max_channels: capabilities.fast_max_channels,
        normal_max_channels: capabilities.normal_max_channels,
        fast_block_samples: capabilities.fast_block_samples,
        fast_rate_hz: capabilities.fast_rate_hz,
        normal_rate_hz: capabilities.normal_rate_hz,
        channels,
        motion,
    };

    let mut guard = state.lock().map_err(|_| "desktop state is poisoned".to_owned())?;
    guard.session = Some(session);
    guard.schema = Some(schema);
    guard.parameters = Some(parameters);
    guard.capabilities = Some(capabilities);
    guard.port = Some(port);
    Ok(result)
}

#[tauri::command]
fn device_disconnect(state: State<'_, Mutex<DesktopState>>) -> Result<(), String> {
    let (scope, session) = {
        let mut guard = state.lock().map_err(|_| "desktop state is poisoned".to_owned())?;
        let scope = guard.scope.take();
        let session = guard.session.take();
        guard.schema = None;
        guard.parameters = None;
        guard.capabilities = None;
        guard.port = None;
        (scope, session)
    };
    drop(scope);
    drop(session);
    Ok(())
}

#[tauri::command]
fn parameter_list(state: State<'_, Mutex<DesktopState>>) -> Result<Vec<ParameterMetadataDto>, String> {
    let service = parameter_service(&state)?;
    Ok(service.parameters().iter().map(Into::into).collect())
}

#[tauri::command]
fn parameter_read(state: State<'_, Mutex<DesktopState>>, id: u16) -> Result<ParameterReadDto, String> {
    let service = parameter_service(&state)?;
    let value = service.read(id).map_err(|error| error.to_string())?;
    Ok(ParameterReadDto { id, value: value.into() })
}

#[tauri::command]
fn parameter_read_many(
    state: State<'_, Mutex<DesktopState>>,
    ids: Vec<u16>,
) -> Result<Vec<ParameterReadResultDto>, String> {
    let service = parameter_service(&state)?;
    let values = service.read_many(&ids).map_err(|error| error.to_string())?;
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
    let service = parameter_service(&state)?;
    service.write(id, value.into()).map_err(|error| error.to_string())
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
fn scope_configure(
    state: State<'_, Mutex<DesktopState>>,
    parameter_ids: Vec<u16>,
    history_seconds: Option<f64>,
) -> Result<ScopeConfigDto, String> {
    let history_seconds = history_seconds.unwrap_or(10.0);
    if !history_seconds.is_finite() || history_seconds <= 0.0 {
        return Err("historySeconds must be positive and finite".to_owned());
    }

    let (old_scope, session, schema, capabilities) = {
        let mut guard = state.lock().map_err(|_| "desktop state is poisoned".to_owned())?;
        let old_scope = guard.scope.take();
        let session = guard.session.clone().ok_or("device is not connected")?;
        let schema = guard.schema.clone().ok_or("HostSchema is not loaded")?;
        let capabilities = guard.capabilities.clone().ok_or("Plot capabilities are not loaded")?;
        (old_scope, session, schema, capabilities)
    };
    drop(old_scope);

    let scope = ScopeSession::from_fast_capabilities(
        session,
        &capabilities,
        &schema,
        &parameter_ids,
        Duration::from_secs_f64(history_seconds),
        1,
    )
    .map_err(|error| error.to_string())?;

    let result = ScopeConfigDto {
        sample_rate_hz: scope.config().sample_rate_hz,
        history_seconds: scope.config().history.as_secs_f64(),
        channels: scope
            .config()
            .channels
            .iter()
            .map(|channel| ScopeChannelDto {
                id: channel.id,
                symbol: channel.symbol.clone(),
                unit: channel.unit.clone(),
            })
            .collect(),
    };

    state
        .lock()
        .map_err(|_| "desktop state is poisoned".to_owned())?
        .scope = Some(scope);
    Ok(result)
}

#[tauri::command]
fn scope_live(state: State<'_, Mutex<DesktopState>>) -> Result<(), String> {
    let guard = state.lock().map_err(|_| "desktop state is poisoned".to_owned())?;
    guard.scope.as_ref().ok_or("Scope is not configured")?.live().map_err(|error| error.to_string())
}

#[tauri::command]
fn scope_pause(state: State<'_, Mutex<DesktopState>>) -> Result<(), String> {
    let guard = state.lock().map_err(|_| "desktop state is poisoned".to_owned())?;
    guard.scope.as_ref().ok_or("Scope is not configured")?.pause().map_err(|error| error.to_string())
}

#[tauri::command]
fn scope_clear(state: State<'_, Mutex<DesktopState>>) -> Result<(), String> {
    let guard = state.lock().map_err(|_| "desktop state is poisoned".to_owned())?;
    guard.scope.as_ref().ok_or("Scope is not configured")?.clear().map_err(|error| error.to_string())
}

#[tauri::command]
fn scope_status(state: State<'_, Mutex<DesktopState>>) -> Result<ScopeStatusDto, String> {
    let guard = state.lock().map_err(|_| "desktop state is poisoned".to_owned())?;
    let status = guard.scope.as_ref().ok_or("Scope is not configured")?.status().map_err(|error| error.to_string())?;
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
    max_points: Option<usize>,
) -> Result<ScopeSnapshotDto, String> {
    let guard = state.lock().map_err(|_| "desktop state is poisoned".to_owned())?;
    let scope = guard.scope.as_ref().ok_or("Scope is not configured")?;
    let status = scope.status().map_err(|error| error.to_string())?;
    let rate = scope.config().sample_rate_hz;
    let window = window_seconds.unwrap_or(0.5).clamp(0.01, scope.config().history.as_secs_f64());
    let wanted = (window * f64::from(rate)).ceil() as usize;
    let snapshot = scope.snapshot_tail(wanted).map_err(|error| error.to_string())?;
    let sample_count = snapshot.sample_count();
    let channel_count = snapshot.config.channel_count;
    let max_points = max_points.unwrap_or(2500).clamp(100, 10_000);
    let stride = sample_count.div_ceil(max_points).max(1);

    let mut times = Vec::with_capacity(sample_count.div_ceil(stride));
    let mut series = (0..channel_count)
        .map(|_| Vec::with_capacity(sample_count.div_ceil(stride)))
        .collect::<Vec<_>>();

    for sample_index in (0..sample_count).step_by(stride) {
        let sample = snapshot.sample(sample_index).ok_or("snapshot indexing failed")?;
        let t = (sample_index as f64 - sample_count.saturating_sub(1) as f64) / f64::from(rate);
        times.push(t);
        for (channel, value) in sample.iter().enumerate() {
            series[channel].push(*value);
        }
    }

    Ok(ScopeSnapshotDto {
        sample_rate_hz: rate,
        sample_count: status.samples,
        lost_frames: status.lost_frames,
        state: stream_state_name(status.state),
        times,
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
            parameter_write,
            motion_get,
            motion_set,
            motion_preview,
            scope_configure,
            scope_live,
            scope_pause,
            scope_clear,
            scope_status,
            scope_snapshot,
        ])
        .run(tauri::generate_context!())
        .expect("error while running NMIXX Motor Studio");
}
