use std::sync::Mutex;
use std::time::Duration;

use nmixx_app::{
    DEFAULT_USB_BAUD, DevicePlotCapabilities, DeviceSession, HostSchema, ScopeSession, StreamState,
};
use serde::Serialize;
use tauri::State;

#[derive(Default)]
struct DesktopState {
    session: Option<DeviceSession>,
    schema: Option<HostSchema>,
    capabilities: Option<DevicePlotCapabilities>,
    scope: Option<ScopeSession>,
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

    let result = ConnectionDto {
        port: port.clone(),
        fast_max_channels: capabilities.fast_max_channels,
        normal_max_channels: capabilities.normal_max_channels,
        fast_block_samples: capabilities.fast_block_samples,
        fast_rate_hz: capabilities.fast_rate_hz,
        normal_rate_hz: capabilities.normal_rate_hz,
        channels,
    };

    let mut guard = state.lock().map_err(|_| "desktop state is poisoned".to_owned())?;
    guard.session = Some(session);
    guard.schema = Some(schema);
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
        guard.capabilities = None;
        guard.port = None;
        (scope, session)
    };
    drop(scope);
    drop(session);
    Ok(())
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
    guard
        .scope
        .as_ref()
        .ok_or("Scope is not configured")?
        .live()
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn scope_pause(state: State<'_, Mutex<DesktopState>>) -> Result<(), String> {
    let guard = state.lock().map_err(|_| "desktop state is poisoned".to_owned())?;
    guard
        .scope
        .as_ref()
        .ok_or("Scope is not configured")?
        .pause()
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn scope_clear(state: State<'_, Mutex<DesktopState>>) -> Result<(), String> {
    let guard = state.lock().map_err(|_| "desktop state is poisoned".to_owned())?;
    guard
        .scope
        .as_ref()
        .ok_or("Scope is not configured")?
        .clear()
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn scope_status(state: State<'_, Mutex<DesktopState>>) -> Result<ScopeStatusDto, String> {
    let guard = state.lock().map_err(|_| "desktop state is poisoned".to_owned())?;
    let status = guard
        .scope
        .as_ref()
        .ok_or("Scope is not configured")?
        .status()
        .map_err(|error| error.to_string())?;
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
        let sample = snapshot
            .sample(sample_index)
            .ok_or("snapshot indexing failed")?;
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
