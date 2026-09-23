use std::path::Path;
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

#[path = "motor_gate.rs"]
mod motor_gate;
use motor_gate::MotorGate;

use thiserror::Error;
use crate::{
    ActionHandle, AxdrStatus, ConfigService, ConfigServiceError, DevicePlotCapabilities, DeviceSession,
    HostSchema, IdentificationKind, IdentificationStart, MixedScopeConfig, MixedScopeError, MixedScopeSession,
    MixedScopeSnapshot, MixedScopeStatus, MotionCapabilities, MotionConfig, MotionMode, MotionPreview,
    PositionCommand, MotionService, MotorActionError, MotorActionService, ParameterMetadata, ParameterService,
    ParameterServiceError, ParameterValue, PlotCapabilitiesError, PreflightError, PreflightIssue,
    PreflightService, ScopeRate, ScopeSelection, SessionError, SessionEvent,
};

#[derive(Debug, Error)]
pub enum ApplicationError {
    #[error(transparent)] Session(#[from] SessionError),
    #[error(transparent)] PlotCapabilities(#[from] PlotCapabilitiesError),
    #[error(transparent)] Parameter(#[from] ParameterServiceError),
    #[error(transparent)] Scope(#[from] MixedScopeError),
    #[error(transparent)] MotorAction(#[from] MotorActionError),
    #[error(transparent)] Config(#[from] ConfigServiceError),
    #[error(transparent)] Preflight(#[from] PreflightError),
    #[error("application runtime state is poisoned")] Poisoned,
    #[error("unknown Action '{0}' in loaded Host schema")] UnknownAction(String),
    #[error("Scope is not configured")] ScopeNotConfigured,
    #[error("tuning experiment is already active")] TuningExperimentBusy,
    #[error("tuning experiment requires motor state ENABLED")] TuningExperimentMotorNotEnabled,
    #[error("required tuning Plot channel '{0}' is not available at the requested rate")] TuningExperimentChannel(String),
    #[error("Parameter synchronization failed: {0}")] ParameterSync(String),
    #[error("{0}")] Motion(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TuningExperimentState { Idle, Preparing, Running, Stopping, Completed, Failed }
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TuningExperimentStatus { pub state: TuningExperimentState, pub message: Option<String> }
#[derive(Debug, Clone, PartialEq)]
pub struct TuningExperimentSnapshot {
    pub status: TuningExperimentStatus,
    pub config: MixedScopeConfig,
    pub snapshot: MixedScopeSnapshot,
}
#[derive(Debug)]
struct TuningExperimentRuntime { generation: u64, state: TuningExperimentState, stop_requested: bool, message: Option<String> }
impl Default for TuningExperimentRuntime {
    fn default() -> Self { Self { generation: 0, state: TuningExperimentState::Idle, stop_requested: false, message: None } }
}
#[derive(Debug)]
struct MotionRepeatRuntime { endpoint_a: Option<f64>, endpoint_b: Option<f64>, next_is_b: bool, pending_target_is_b: Option<bool> }
impl Default for MotionRepeatRuntime {
    fn default() -> Self { Self { endpoint_a: None, endpoint_b: None, next_is_b: true, pending_target_is_b: None } }
}
const TUNING_CAPTURE_BUDGET_BYTES: usize = 128 * 1024 * 1024;
const TUNING_LIVE_WINDOW: Duration = Duration::from_secs(3);
const TUNING_PRE_CAPTURE: Duration = Duration::from_millis(500);
const TUNING_POST_CAPTURE: Duration = Duration::from_millis(750);
const TUNING_POSITION_SETTLE: Duration = Duration::from_millis(500);
const TUNING_POLL: Duration = Duration::from_millis(20);
const MOTOR_STOP_TIMEOUT: Duration = Duration::from_secs(60);

struct ApplicationInner {
    session: DeviceSession,
    schema: HostSchema,
    parameters: ParameterService,
    events: Mutex<Vec<mpsc::Sender<SessionEvent>>>,
    plot_capabilities: Mutex<Option<DevicePlotCapabilities>>,
    motion_capabilities: MotionCapabilities,
    motion: MotionService,
    motion_repeat: Mutex<MotionRepeatRuntime>,
    scope: Mutex<Option<MixedScopeSession>>,
    tuning_experiment: Mutex<TuningExperimentRuntime>,
    motor_gate: MotorGate,
    tuning_worker: Mutex<Option<thread::JoinHandle<()>>>,
}
#[derive(Clone)]
pub struct ApplicationSession { inner: Arc<ApplicationInner> }

impl ApplicationSession {
    pub fn open_usb(path: impl AsRef<Path>, baud_rate: u32, schema: HostSchema) -> Result<Self, ApplicationError> {
        Self::from_session(DeviceSession::open_usb(path, baud_rate)?, schema)
    }
    pub fn open_usb_with_motion(path: impl AsRef<Path>, baud_rate: u32, schema: HostSchema, motion: MotionService) -> Result<Self, ApplicationError> {
        Self::from_session_with_motion(DeviceSession::open_usb(path, baud_rate)?, schema, motion)
    }
    pub fn from_session(session: DeviceSession, schema: HostSchema) -> Result<Self, ApplicationError> {
        Self::from_session_with_motion(session, schema, MotionService::default())
    }
    pub fn from_session_with_motion(session: DeviceSession, schema: HostSchema, motion: MotionService) -> Result<Self, ApplicationError> {
        let events = session.subscribe()?;
        let parameters = ParameterService::new(session.clone(), schema.clone());
        let motion_capabilities = MotionCapabilities::from_schema(&schema);
        let app = Self { inner: Arc::new(ApplicationInner {
            session, schema, parameters, events: Mutex::new(Vec::new()),
            plot_capabilities: Mutex::new(None), motion_capabilities, motion,
            motion_repeat: Mutex::new(MotionRepeatRuntime::default()), scope: Mutex::new(None),
            tuning_experiment: Mutex::new(TuningExperimentRuntime::default()),
            motor_gate: MotorGate::default(), tuning_worker: Mutex::new(None),
        }) };
        let weak = Arc::downgrade(&app.inner);
        thread::spawn(move || {
            while let Ok(event) = events.recv() {
                let Some(inner) = weak.upgrade() else { break; };
                if matches!(&event, SessionEvent::ActionCompleted { .. }) {
                    // Application owns readback, even with no page/CLI subscriber. Failed
                    // reads are retained as cache errors instead of reviving old values.
                    let _ = inner.parameters.refresh_all();
                }
                if let Ok(mut subscribers) = inner.events.lock() {
                    subscribers.retain(|sender| sender.send(event.clone()).is_ok());
                };
            }
        });
        Ok(app)
    }
    pub fn available_usb_ports() -> Result<Vec<String>, ApplicationError> { Ok(DeviceSession::available_usb_ports()?) }
    pub fn schema(&self) -> &HostSchema { &self.inner.schema }
    pub fn plot_capabilities(&self) -> Result<DevicePlotCapabilities, ApplicationError> {
        let mut slot = self.inner.plot_capabilities.lock().map_err(|_| ApplicationError::Poisoned)?;
        if slot.is_none() { *slot = Some(DevicePlotCapabilities::discover(&self.inner.session)?); }
        Ok(slot.as_ref().expect("Plot capabilities initialized").clone())
    }
    pub fn motion_capabilities(&self) -> &MotionCapabilities { &self.inner.motion_capabilities }
    pub fn parameter_metadata(&self) -> &[ParameterMetadata] { self.inner.parameters.parameters() }
    pub fn parameter_read(&self, id: u16) -> Result<ParameterValue, ApplicationError> { Ok(self.inner.parameters.read(id)?) }
    pub fn parameter_read_many(&self, ids: &[u16]) -> Result<Vec<(u16, Result<ParameterValue, ParameterServiceError>)>, ApplicationError> {
        Ok(self.inner.parameters.read_many(ids)?)
    }
    pub fn parameter_cached(&self, id: u16) -> Result<Option<ParameterValue>, ApplicationError> { Ok(self.inner.parameters.cached(id)?) }
    pub fn parameter_cached_many(&self, ids: &[u16]) -> Result<Vec<(u16, Result<ParameterValue, ParameterServiceError>)>, ApplicationError> {
        let snapshot = self.inner.parameters.snapshot()?;
        Ok(ids.iter().copied().map(|id| {
            let result = self.inner.parameters.metadata(id).and_then(|_| match snapshot.get(&id) {
                Some(Ok(value)) => Ok(*value),
                Some(Err(message)) => Err(ParameterServiceError::CachedReadFailed { id, message: message.clone() }),
                None => Err(ParameterServiceError::CacheMissing(id)),
            });
            (id, result)
        }).collect())
    }
    pub fn parameter_subscribe(&self) -> Result<mpsc::Receiver<Vec<u16>>, ApplicationError> { Ok(self.inner.parameters.subscribe()?) }
    pub fn parameter_refresh_all(&self) -> Result<Vec<(u16, Result<ParameterValue, ParameterServiceError>)>, ApplicationError> {
        Ok(self.inner.parameters.refresh_all()?)
    }
    pub fn parameter_write(&self, id: u16, value: ParameterValue) -> Result<ParameterValue, ApplicationError> {
        Ok(self.inner.parameters.write_readback(id, value)?)
    }
    fn require_refresh(&self, results: Vec<(u16, Result<ParameterValue, ParameterServiceError>)>) -> Result<(), ApplicationError> {
        let failures = results.into_iter().filter_map(|(_, result)| result.err().map(|error| error.to_string())).collect::<Vec<_>>();
        if failures.is_empty() { Ok(()) } else { Err(ApplicationError::ParameterSync(failures.join("; "))) }
    }
    fn refresh_after_immediate_action(&self) -> Result<(), ApplicationError> {
        self.require_refresh(self.parameter_refresh_all()?)
    }
    fn refresh_motor_context(&self) -> Result<(), ApplicationError> {
        let ids = self.parameter_metadata().iter().filter(|meta| meta.access.contains('r')
            && (meta.symbol == "PARAM_MOTOR_STATE" || meta.symbol == "PARAM_MOTOR_MODE" || meta.symbol.starts_with("PARAM_TARGET_")))
            .map(|meta| meta.id).collect::<Vec<_>>();
        self.require_refresh(self.parameter_read_many(&ids)?)
    }
    pub fn action_start(&self, key: &str) -> Result<ActionHandle, ApplicationError> {
        let action = self.inner.schema.action_by_key(key).ok_or_else(|| ApplicationError::UnknownAction(key.to_owned()))?;
        match action.symbol.as_str() {
            "ACTION_MOTOR_STOP" => return self.motor_stop(),
            "ACTION_MOTOR_DISABLE" => return self.motor_disable(),
            _ => {}
        }
        let ticket = self.inner.motor_gate.ticket();
        let _command = self.inner.motor_gate.start(ticket)
            .map_err(|message| ApplicationError::Motion(message.to_owned()))?;
        self.ensure_no_tuning()?;
        let handle = self.inner.session.action_start(action.id)?;
        match action.symbol.as_str() {
            "ACTION_IDENT_APPLY" | "ACTION_POSITION_SET_ZERO" | "ACTION_PROTECTION_CLEAR" => self.refresh_after_immediate_action()?,
            "ACTION_MOTOR_ENABLE" | "ACTION_MOTOR_DISABLE" | "ACTION_MOTOR_STOP" => self.refresh_motor_context()?,
            _ => {}
        }
        Ok(handle)
    }
    /// Only use for Actions whose firmware response represents completed execution.
    pub fn action_start_immediate(&self, key: &str) -> Result<ActionHandle, ApplicationError> {
        let action = self.inner.schema.action_by_key(key).ok_or_else(|| ApplicationError::UnknownAction(key.to_owned()))?;
        if action.symbol == "ACTION_MOTOR_STOP" { return self.motor_stop(); }
        if action.symbol == "ACTION_MOTOR_DISABLE" { return self.motor_disable(); }
        let ticket = self.inner.motor_gate.ticket();
        let _command = self.inner.motor_gate.start(ticket)
            .map_err(|message| ApplicationError::Motion(message.to_owned()))?;
        self.ensure_no_tuning()?;
        let handle = self.inner.session.action_start(action.id)?;
        self.refresh_after_immediate_action()?;
        Ok(handle)
    }
    pub fn subscribe(&self) -> Result<mpsc::Receiver<SessionEvent>, ApplicationError> {
        let (sender, receiver) = mpsc::channel();
        self.inner.events.lock().map_err(|_| ApplicationError::Poisoned)?.push(sender);
        Ok(receiver)
    }
    pub fn preflight_phase_search(&self) -> Result<Vec<PreflightIssue>, ApplicationError> {
        Ok(PreflightService::new(self.inner.parameters.clone()).check_phase_search()?)
    }
    pub fn preflight_identification(&self, kind: IdentificationKind) -> Result<Vec<PreflightIssue>, ApplicationError> {
        Ok(PreflightService::new(self.inner.parameters.clone()).check_identification(kind)?)
    }
    fn ensure_no_tuning(&self) -> Result<(), ApplicationError> {
        let runtime = self.inner.tuning_experiment.lock().map_err(|_| ApplicationError::Poisoned)?;
        if matches!(runtime.state, TuningExperimentState::Preparing | TuningExperimentState::Running | TuningExperimentState::Stopping) {
            return Err(ApplicationError::TuningExperimentBusy);
        }
        Ok(())
    }
    fn motor_actions(&self) -> MotorActionService {
        MotorActionService::from_shared(self.inner.session.clone(), self.inner.schema.clone(), self.inner.parameters.clone())
    }
    pub fn identification_start(&self, kind: IdentificationKind, allow_enable: bool) -> Result<IdentificationStart, ApplicationError> {
        let ticket = self.inner.motor_gate.ticket();
        let _command = self.inner.motor_gate.start(ticket)
            .map_err(|message| ApplicationError::Motion(message.to_owned()))?;
        self.ensure_no_tuning()?;
        let result = self.motor_actions().identification_start(kind, allow_enable)?;
        if matches!(&result, IdentificationStart::Started(_)) { self.refresh_motor_context()?; }
        Ok(result)
    }
    pub fn identification_apply(&self) -> Result<ActionHandle, ApplicationError> {
        let handle = self.motor_actions().identification_apply()?;
        self.refresh_after_immediate_action()?;
        Ok(handle)
    }
    pub fn motor_enable(&self) -> Result<ActionHandle, ApplicationError> {
        let ticket = self.inner.motor_gate.ticket();
        let _command = self.inner.motor_gate.start(ticket)
            .map_err(|message| ApplicationError::Motion(message.to_owned()))?;
        self.ensure_no_tuning()?;
        let handle = self.motor_actions().enable()?;
        self.refresh_motor_context()?;
        Ok(handle)
    }
    pub fn motor_stop(&self) -> Result<ActionHandle, ApplicationError> {
        // Cancel delayed Run before sending Stop, including while PREPARING.
        self.inner.motor_gate.cancel();
        let cancelled = self.request_tuning_stop();
        let handle = {
            let _command = self.inner.motor_gate.stop();
            self.motor_actions().stop()?
        };
        cancelled?;
        self.refresh_motor_context()?;
        Ok(handle)
    }
    pub fn motor_disable(&self) -> Result<ActionHandle, ApplicationError> {
        self.inner.motor_gate.cancel();
        let cancelled = self.request_tuning_stop();
        let handle = {
            let _command = self.inner.motor_gate.stop();
            self.motor_actions().disable()?
        };
        cancelled?;
        self.refresh_motor_context()?;
        Ok(handle)
    }
    pub fn phase_search_start(&self) -> Result<ActionHandle, ApplicationError> {
        let ticket = self.inner.motor_gate.ticket();
        let _command = self.inner.motor_gate.start(ticket)
            .map_err(|message| ApplicationError::Motion(message.to_owned()))?;
        self.ensure_no_tuning()?;
        let handle = self.motor_actions().phase_search_start()?;
        self.refresh_motor_context()?;
        Ok(handle)
    }
    pub fn is_same_session(&self, other: &Self) -> bool { Arc::ptr_eq(&self.inner, &other.inner) }

    pub fn disconnect(&self) -> Result<(), ApplicationError> {
        let deadline = Instant::now() + MOTOR_STOP_TIMEOUT;
        self.inner.motor_gate.close();
        let mut failures = Vec::new();
        if let Err(error) = self.request_tuning_stop() { failures.push(error.to_string()); }
        {
            let _command = self.inner.motor_gate.stop();
            if let Err(error) = self.motor_actions().stop() { failures.push(error.to_string()); }
        }
        let worker = self.inner.tuning_worker.lock().map_err(|_| ApplicationError::Poisoned)?.take();
        if let Some(worker) = worker {
            if worker.join().is_err() { failures.push("tuning worker panicked".to_owned()); }
        }
        if let Err(error) = self.wait_motor_stopped_until(deadline) { failures.push(error.to_string()); }
        let has_scope = self.inner.scope.lock().map_err(|_| ApplicationError::Poisoned)?.is_some();
        if has_scope {
            if let Err(error) = self.scope_stop() { failures.push(error.to_string()); }
        }
        if failures.is_empty() { Ok(()) }
        else { Err(ApplicationError::Motion(format!("Disconnect incomplete; motor/transport status must be checked: {}", failures.join("; ")))) }
    }

    fn wait_motor_stopped(&self) -> Result<(), ApplicationError> {
        self.wait_motor_stopped_until(Instant::now() + MOTOR_STOP_TIMEOUT)
    }
    fn wait_motor_stopped_until(&self, deadline: Instant) -> Result<(), ApplicationError> {
        loop {
            match self.read_motor_state()? {
                0 | 1 => return Ok(()),
                2 => {}
                state => return Err(ApplicationError::Motion(format!("Unknown motor state {state} while stopping"))),
            }
            if Instant::now() >= deadline {
                return Err(ApplicationError::Motion("Motor is still RUN after the 60 s controlled-stop timeout; use Disable to turn off the drive".to_owned()));
            }
            thread::sleep(TUNING_POLL);
        }
    }

    pub fn config_save_available(&self) -> bool {
        ConfigService::from_shared(self.inner.session.clone(), self.inner.schema.clone(), self.inner.parameters.clone()).save_available()
    }
    pub fn config_save(&self) -> Result<ActionHandle, ApplicationError> {
        // AxDr executes NVS_Storage_Save_All before replying. There is no later
        // ACTION_COMPLETE for Save; a rejected/failed response is propagated.
        Ok(ConfigService::from_shared(self.inner.session.clone(), self.inner.schema.clone(), self.inner.parameters.clone()).save()?)
    }
    pub fn motion_get(&self) -> MotionConfig { self.inner.motion.get() }
    pub fn motion_set(&self, config: MotionConfig) -> Result<MotionConfig, ApplicationError> {
        let current = self.inner.motion.get();
        let reset_repeat = current.repeat != config.repeat || current.position_command != config.position_command
            || current.incremental_delta_turn != config.incremental_delta_turn;
        let canonical = self.inner.motion.set(config).map_err(ApplicationError::Motion)?;
        if reset_repeat { self.reset_motion_repeat()?; }
        Ok(canonical)
    }
    pub fn motion_preview(&self) -> Result<MotionPreview, ApplicationError> {
        self.inner.motion.preview_with_parameters(&self.inner.parameters, None).map_err(ApplicationError::Motion)
    }
    pub fn motion_run(&self) -> Result<ActionHandle, ApplicationError> {
        self.motion_run_at(self.inner.motor_gate.ticket(), None)
    }
    fn motion_run_at(&self, ticket: u64, experiment: Option<u64>) -> Result<ActionHandle, ApplicationError> {
        let _command = self.inner.motor_gate.start(ticket)
            .map_err(|message| ApplicationError::Motion(message.to_owned()))?;
        {
            let runtime = self.inner.tuning_experiment.lock().map_err(|_| ApplicationError::Poisoned)?;
            if let Some(generation) = experiment {
                if runtime.generation != generation || runtime.stop_requested {
                    return Err(ApplicationError::Motion("tuning Run was cancelled".to_owned()));
                }
            } else if matches!(runtime.state, TuningExperimentState::Preparing | TuningExperimentState::Running | TuningExperimentState::Stopping) {
                return Err(ApplicationError::TuningExperimentBusy);
            }
        }
        let events = self.subscribe()?;
        let config = self.inner.motion.get();
        let mode = self.read_motion_mode()?;
        let mut repeat_target: Option<(f64, bool)> = None;
        if mode == MotionMode::Position && config.repeat {
            let needs_init = {
                let runtime = self.inner.motion_repeat.lock().map_err(|_| ApplicationError::Poisoned)?;
                runtime.endpoint_a.is_none() || runtime.endpoint_b.is_none()
            };
            if needs_init {
                let current = self.read_position_turns("PARAM_RUN_POSITION")?;
                let target = match config.position_command {
                    PositionCommand::Absolute => self.read_position_turns("PARAM_TARGET_POSITION")?,
                    PositionCommand::Incremental => current + config.incremental_delta_turn,
                };
                if (target - current).abs() <= 1e-9 { return Err(ApplicationError::Motion("Repeat position endpoints must be different".to_owned())); }
                let mut runtime = self.inner.motion_repeat.lock().map_err(|_| ApplicationError::Poisoned)?;
                runtime.endpoint_a = Some(current); runtime.endpoint_b = Some(target);
                runtime.next_is_b = true; runtime.pending_target_is_b = None;
            }
            let runtime = self.inner.motion_repeat.lock().map_err(|_| ApplicationError::Poisoned)?;
            let target_is_b = runtime.next_is_b;
            let target = (if target_is_b { runtime.endpoint_b } else { runtime.endpoint_a })
                .ok_or_else(|| ApplicationError::Motion("Repeat position endpoints are not initialized".to_owned()))?;
            repeat_target = Some((target, target_is_b));
        }
        let handle = self.inner.motion.run(&self.inner.parameters, &self.inner.session, repeat_target.map(|(target, _)| target))
            .map_err(ApplicationError::Motion)?;
        if let Some((_, target_is_b)) = repeat_target {
            self.inner.motion_repeat.lock().map_err(|_| ApplicationError::Poisoned)?.pending_target_is_b = Some(target_is_b);
            // Do not keep the whole connection alive while waiting for an Action.
            let weak = Arc::downgrade(&self.inner);
            thread::spawn(move || {
                while let Ok(event) = events.recv() {
                    let SessionEvent::ActionCompleted { handle: completed, status } = event else { continue; };
                    if completed != handle { continue; }
                    if let Some(inner) = weak.upgrade() { let _ = (ApplicationSession { inner }).motion_run_completed(status); }
                    break;
                }
            });
        }
        Ok(handle)
    }
    pub fn motion_run_completed(&self, status: AxdrStatus) -> Result<(), ApplicationError> {
        let mut runtime = self.inner.motion_repeat.lock().map_err(|_| ApplicationError::Poisoned)?;
        if let Some(target_is_b) = runtime.pending_target_is_b.take() {
            if status == AxdrStatus::Ok { runtime.next_is_b = !target_is_b; }
        }
        Ok(())
    }
    pub fn motion_stop(&self) -> Result<ActionHandle, ApplicationError> { self.cancel_motion_repeat_leg()?; self.motor_stop() }
    pub fn scope_configure(&self, selections: &[ScopeSelection], history: Duration, config_id: u8) -> Result<MixedScopeConfig, ApplicationError> {
        let old_scope = {
            let mut slot = self.inner.scope.lock().map_err(|_| ApplicationError::Poisoned)?;
            if let Some(scope) = slot.as_ref() {
                if scope.config()?.history == history { return Ok(scope.reconfigure(selections)?); }
                let _ = scope.stop();
            }
            slot.take()
        };
        drop(old_scope);
        let plot_capabilities = self.plot_capabilities()?;
        let scope = MixedScopeSession::from_capabilities(self.inner.session.clone(), &plot_capabilities,
            &self.inner.schema, selections, history, config_id)?;
        let config = scope.config()?;
        *self.inner.scope.lock().map_err(|_| ApplicationError::Poisoned)? = Some(scope);
        Ok(config)
    }
    pub fn scope_live(&self) -> Result<(), ApplicationError> { self.with_scope(|scope| scope.live()) }
    pub fn scope_resume(&self) -> Result<(), ApplicationError> { self.with_scope(|scope| scope.resume()) }
    pub fn scope_pause(&self) -> Result<(), ApplicationError> { self.with_scope(|scope| scope.pause()) }
    pub fn scope_stop(&self) -> Result<(), ApplicationError> { self.with_scope(|scope| scope.stop()) }
    pub fn scope_clear(&self) -> Result<(), ApplicationError> { self.with_scope(|scope| scope.clear()) }
    pub fn scope_capture(&self, duration: Duration) -> Result<(), ApplicationError> { self.with_scope(|scope| scope.capture(duration)) }
    pub fn scope_status(&self) -> Result<MixedScopeStatus, ApplicationError> { self.with_scope(|scope| scope.status()) }
    pub fn scope_snapshot_tail(&self, window: Duration) -> Result<MixedScopeSnapshot, ApplicationError> { self.with_scope(|scope| scope.snapshot_tail(window)) }
    pub fn scope_snapshot_window(&self, window: Duration, end_offset: Duration) -> Result<MixedScopeSnapshot, ApplicationError> {
        self.with_scope(|scope| scope.snapshot_window(window, end_offset))
    }
    pub fn scope_recorded_duration(&self) -> Result<Duration, ApplicationError> { self.with_scope(|scope| scope.recorded_duration()) }
    pub fn scope_config(&self) -> Result<MixedScopeConfig, ApplicationError> { self.with_scope(|scope| scope.config()) }
    pub fn tuning_experiment_default_selections(&self) -> Result<Vec<ScopeSelection>, ApplicationError> {
        self.tuning_default_selections(self.read_motion_mode()?)
    }
    fn tuning_capture_duration(&self, selections: &[ScopeSelection]) -> Result<Duration, ApplicationError> {
        let capabilities = self.plot_capabilities()?;
        let mut samples_per_second = 0u64;
        for selection in selections {
            let channel = capabilities.channel(selection.id)
                .ok_or_else(|| ApplicationError::TuningExperimentChannel(format!("0x{:04X}", selection.id)))?;
            let rate = match selection.rate { ScopeRate::Fast => capabilities.fast_rate_hz, ScopeRate::Normal => capabilities.normal_rate_hz };
            let supported = match selection.rate { ScopeRate::Fast => channel.supports_fast(), ScopeRate::Normal => channel.supports_normal() };
            if !supported { return Err(ApplicationError::TuningExperimentChannel(format!("0x{:04X}", selection.id))); }
            samples_per_second = samples_per_second.saturating_add(u64::from(rate));
        }
        if samples_per_second == 0 { return Err(ApplicationError::TuningExperimentChannel("no capture channels selected".to_owned())); }
        let bytes_per_second = samples_per_second.saturating_mul(std::mem::size_of::<f32>() as u64);
        let seconds = TUNING_CAPTURE_BUDGET_BYTES as f64 / bytes_per_second as f64;
        Ok(Duration::from_secs_f64(seconds.max(TUNING_PRE_CAPTURE.as_secs_f64() + TUNING_POST_CAPTURE.as_secs_f64())))
    }
    pub fn tuning_experiment_start(&self, selections: &[ScopeSelection]) -> Result<TuningExperimentStatus, ApplicationError> {
        let ticket = self.inner.motor_gate.ticket();
        let generation = {
            let _command = self.inner.motor_gate.start(ticket)
                .map_err(|message| ApplicationError::Motion(message.to_owned()))?;
            let mut runtime = self.inner.tuning_experiment.lock().map_err(|_| ApplicationError::Poisoned)?;
            if matches!(runtime.state, TuningExperimentState::Preparing | TuningExperimentState::Running | TuningExperimentState::Stopping) {
                return Err(ApplicationError::TuningExperimentBusy);
            }
            // Publish PREPARING before any blocking I/O. Stop must be able to
            // cancel this operation even before the pre-capture worker exists.
            runtime.generation = runtime.generation.wrapping_add(1);
            runtime.state = TuningExperimentState::Preparing;
            runtime.stop_requested = false;
            runtime.message = None;
            runtime.generation
        };
        let mut capture_owned = false;
        let prepared = (|| -> Result<(), ApplicationError> {
            if self.read_motor_state()? != 1 { return Err(ApplicationError::TuningExperimentMotorNotEnabled); }
            let ids = self.parameter_metadata().iter().filter(|meta| meta.access.contains('r') && (
                meta.symbol.starts_with("PARAM_CTRL_") || meta.symbol.starts_with("PARAM_MOTION_")
                || meta.symbol.starts_with("PARAM_TARGET_") || meta.symbol == "PARAM_RUN_POSITION"
                || meta.symbol == "PARAM_LIMIT_WM_EFFECTIVE"
            )).map(|meta| meta.id).collect::<Vec<_>>();
            self.require_refresh(self.parameter_read_many(&ids)?)?;
            let mode = self.read_motion_mode()?;
            let preview_duration = if mode == MotionMode::Position {
                self.motion_preview()?.times.last().copied().unwrap_or(0.0).max(0.0)
            } else { 0.0 };
            let capture_duration = self.tuning_capture_duration(selections)?;
            // The check and command sequence are ordered against Stop/Disconnect.
            // A stale preparation must not reconfigure/start capture after close.
            let _command = self.inner.motor_gate.start(ticket)
                .map_err(|message| ApplicationError::Motion(message.to_owned()))?;
            let scope_exists = self.inner.scope.lock().map_err(|_| ApplicationError::Poisoned)?.is_some();
            if scope_exists { self.scope_stop()?; }
            self.scope_configure(selections, capture_duration, 2)?;
            capture_owned = true;
            self.scope_capture(capture_duration)?;
            let mut worker = self.inner.tuning_worker.lock().map_err(|_| ApplicationError::Poisoned)?;
            // A previous worker reaches a terminal state only after its cleanup.
            if let Some(old) = worker.take() {
                old.join().map_err(|_| ApplicationError::Motion("previous tuning worker panicked".to_owned()))?;
            }
            let app = self.clone();
            *worker = Some(thread::spawn(move || {
                app.run_tuning_experiment(generation, ticket, mode, preview_duration, capture_duration);
            }));
            Ok(())
        })();
        if let Err(error) = prepared {
            let cleanup = if capture_owned { self.scope_stop() } else { Ok(()) };
            let cancelled = (self.tuning_stop_requested(generation)
                || self.inner.motor_gate.ticket() != ticket) && cleanup.is_ok();
            let error = if let Err(cleanup) = cleanup {
                ApplicationError::Motion(format!("{error}; Scope cleanup: {cleanup}"))
            } else { error };
            if let Ok(mut runtime) = self.inner.tuning_experiment.lock() {
                if runtime.generation == generation {
                    runtime.state = if cancelled { TuningExperimentState::Completed } else { TuningExperimentState::Failed };
                    runtime.message = Some(if cancelled { "Cancelled before Run".to_owned() } else { error.to_string() });
                }
            }
            // No motor Run was sent by this preparation. Do not stop an unrelated
            // motor operation when start validation itself failed.
            if !cancelled { return Err(error); }
        }
        self.tuning_experiment_status()
    }
    fn request_tuning_stop(&self) -> Result<(), ApplicationError> {
        let mut runtime = self.inner.tuning_experiment.lock().map_err(|_| ApplicationError::Poisoned)?;
        if matches!(runtime.state, TuningExperimentState::Preparing | TuningExperimentState::Running | TuningExperimentState::Stopping) {
            runtime.stop_requested = true;
            if runtime.state == TuningExperimentState::Running { runtime.state = TuningExperimentState::Stopping; }
        }
        Ok(())
    }
    pub fn tuning_experiment_stop(&self) -> Result<TuningExperimentStatus, ApplicationError> {
        self.request_tuning_stop()?;
        self.motion_stop()?;
        self.tuning_experiment_status()
    }
    pub fn tuning_experiment_status(&self) -> Result<TuningExperimentStatus, ApplicationError> {
        let runtime = self.inner.tuning_experiment.lock().map_err(|_| ApplicationError::Poisoned)?;
        Ok(TuningExperimentStatus { state: runtime.state, message: runtime.message.clone() })
    }
    pub fn tuning_experiment_snapshot(&self) -> Result<TuningExperimentSnapshot, ApplicationError> {
        let status = self.tuning_experiment_status()?;
        let config = self.scope_config()?;
        let window = if matches!(status.state, TuningExperimentState::Preparing | TuningExperimentState::Running | TuningExperimentState::Stopping) {
            TUNING_LIVE_WINDOW.min(config.history)
        } else { config.history };
        let snapshot = self.scope_snapshot_tail(window)?;
        Ok(TuningExperimentSnapshot { status, config, snapshot })
    }
    fn tuning_default_selections(&self, mode: MotionMode) -> Result<Vec<ScopeSelection>, ApplicationError> {
        let mut selections = vec![self.tuning_selection("PARAM_REF_IQ", ScopeRate::Fast)?, self.tuning_selection("PARAM_RUN_IQ", ScopeRate::Fast)?];
        match mode {
            MotionMode::Position => {
                selections.push(self.tuning_selection("PARAM_REF_POSITION", ScopeRate::Normal)?);
                selections.push(self.tuning_selection("PARAM_RUN_POSITION", ScopeRate::Normal)?);
                selections.push(self.tuning_selection("PARAM_REF_WM", ScopeRate::Normal)?);
                selections.push(self.tuning_selection("PARAM_RUN_WM", ScopeRate::Normal)?);
            }
            MotionMode::Speed | MotionMode::SensorlessSpeed => {
                selections.push(self.tuning_selection("PARAM_REF_WM", ScopeRate::Normal)?);
                selections.push(self.tuning_selection("PARAM_RUN_WM", ScopeRate::Normal)?);
            }
            MotionMode::Torque => {}
        }
        Ok(selections)
    }
    fn tuning_selection(&self, key: &str, rate: ScopeRate) -> Result<ScopeSelection, ApplicationError> {
        let metadata = self.inner.schema.parameter_by_key(key).ok_or_else(|| ApplicationError::TuningExperimentChannel(key.to_owned()))?;
        let capabilities = self.plot_capabilities()?;
        let channel = capabilities.channel(metadata.id).ok_or_else(|| ApplicationError::TuningExperimentChannel(metadata.label.clone()))?;
        let supported = match rate { ScopeRate::Fast => channel.supports_fast(), ScopeRate::Normal => channel.supports_normal() };
        if !supported { return Err(ApplicationError::TuningExperimentChannel(metadata.label.clone())); }
        Ok(ScopeSelection { id: metadata.id, rate })
    }
    fn read_motion_mode(&self) -> Result<MotionMode, ApplicationError> {
        let metadata = self.inner.schema.parameter_by_key("PARAM_MOTOR_MODE").ok_or_else(|| ApplicationError::Motion("Motor mode is not exposed".to_owned()))?;
        match self.inner.parameters.read(metadata.id)? {
            ParameterValue::U8(value) => crate::motion::mode_from_wire_value(value).map_err(ApplicationError::Motion),
            _ => Err(ApplicationError::Motion("Motor mode has an unexpected type".to_owned())),
        }
    }
    fn read_position_turns(&self, key: &str) -> Result<f64, ApplicationError> {
        let metadata = self.inner.schema.parameter_by_key(key).ok_or_else(|| ApplicationError::Motion(format!("{key} is not exposed")))?;
        match self.inner.parameters.read(metadata.id)? {
            ParameterValue::Position(value) => Ok(f64::from(value.turns) + f64::from(value.theta) / std::f64::consts::TAU),
            _ => Err(ApplicationError::Motion(format!("{key} has an unexpected type"))),
        }
    }
    fn reset_motion_repeat(&self) -> Result<(), ApplicationError> {
        *self.inner.motion_repeat.lock().map_err(|_| ApplicationError::Poisoned)? = MotionRepeatRuntime::default(); Ok(())
    }
    fn cancel_motion_repeat_leg(&self) -> Result<(), ApplicationError> {
        self.inner.motion_repeat.lock().map_err(|_| ApplicationError::Poisoned)?.pending_target_is_b = None; Ok(())
    }
    fn read_motor_state(&self) -> Result<u8, ApplicationError> {
        let metadata = self.inner.schema.parameter_by_key("PARAM_MOTOR_STATE").ok_or_else(|| ApplicationError::Motion("Motor state is not exposed".to_owned()))?;
        match self.inner.parameters.read(metadata.id)? {
            ParameterValue::U8(value) => Ok(value),
            _ => Err(ApplicationError::Motion("Motor state has an unexpected type".to_owned())),
        }
    }
    fn tuning_stop_requested(&self, generation: u64) -> bool {
        self.inner.motor_gate.is_closed()
            || self.inner.tuning_experiment.lock().map(|runtime| runtime.generation != generation || runtime.stop_requested).unwrap_or(true)
    }
    fn set_tuning_state(&self, generation: u64, state: TuningExperimentState) -> bool {
        let Ok(mut runtime) = self.inner.tuning_experiment.lock() else { return false; };
        if runtime.generation != generation { return false; }
        runtime.state = state; true
    }
    fn fail_tuning_experiment(&self, generation: u64, error: impl ToString) {
        {
            let Ok(mut runtime) = self.inner.tuning_experiment.lock() else { return; };
            if runtime.generation != generation { return; }
            runtime.state = TuningExperimentState::Stopping;
        }
        let mut failures = vec![error.to_string()];
        // A plot error ends the experiment, not just its recording. Motor Stop
        // is separate from Scope Stop; retain every cleanup error for the user.
        if let Err(error) = self.motor_stop() { failures.push(format!("Motor Stop: {error}")); }
        if let Err(error) = self.wait_motor_stopped() { failures.push(format!("Motor state: {error}")); }
        if let Err(error) = self.scope_stop() { failures.push(format!("Scope Stop: {error}")); }
        if let Ok(mut runtime) = self.inner.tuning_experiment.lock() {
            if runtime.generation == generation {
                runtime.state = TuningExperimentState::Failed;
                runtime.message = Some(failures.join("; "));
            }
        }
    }
    fn finish_cancelled_pre_capture(&self, generation: u64) {
        if let Err(error) = self.scope_stop() {
            self.fail_tuning_experiment(generation, error);
            return;
        }
        if let Ok(mut runtime) = self.inner.tuning_experiment.lock() {
            if runtime.generation == generation {
                runtime.state = TuningExperimentState::Completed;
                runtime.message = Some("Cancelled before Run".to_owned());
            }
        }
    }
    fn wait_tuning(&self, generation: u64, duration: Duration, stop_sensitive: bool) -> bool {
        let mut elapsed = Duration::ZERO;
        while elapsed < duration {
            if stop_sensitive && self.tuning_stop_requested(generation) { return false; }
            let step = TUNING_POLL.min(duration.saturating_sub(elapsed));
            thread::sleep(step); elapsed += step;
        }
        true
    }
    fn run_tuning_experiment(&self, generation: u64, ticket: u64, mode: MotionMode, preview_duration: f64, capture_duration: Duration) {
        if !self.wait_tuning(generation, TUNING_PRE_CAPTURE, true) {
            self.finish_cancelled_pre_capture(generation);
            return;
        }
        if let Err(error) = self.motion_run_at(ticket, Some(generation)) {
            if self.tuning_stop_requested(generation) || self.inner.motor_gate.ticket() != ticket {
                self.finish_cancelled_pre_capture(generation);
            } else {
                self.fail_tuning_experiment(generation, error);
            }
            return;
        }
        if !self.set_tuning_state(generation, TuningExperimentState::Running) { return; }
        let auto_position = mode == MotionMode::Position;
        let mut stop_sent = false;
        let reserve_ratio = (TUNING_POST_CAPTURE.as_secs_f64() / capture_duration.as_secs_f64()).clamp(0.0, 1.0);
        if auto_position {
            let run_window = Duration::from_secs_f64(preview_duration) + TUNING_POSITION_SETTLE;
            let mut elapsed = Duration::ZERO;
            let mut completed_window = true;
            while elapsed < run_window {
                if self.tuning_stop_requested(generation) { completed_window = false; break; }
                match self.scope_status() {
                    Ok(status) if status.capacity_samples > 0 => {
                        let used = status.samples as f64 / status.capacity_samples as f64;
                        if used >= 1.0 - reserve_ratio {
                            completed_window = false;
                            if self.read_motor_state().ok() == Some(2) {
                                if let Err(error) = self.motion_stop() { self.fail_tuning_experiment(generation, error); return; }
                                stop_sent = true;
                                let _ = self.set_tuning_state(generation, TuningExperimentState::Stopping);
                            }
                            break;
                        }
                    }
                    Ok(_) => {}
                    Err(error) => { self.fail_tuning_experiment(generation, error); return; }
                }
                match self.read_motor_state() {
                    Ok(2) => {}
                    Ok(state) => {
                        if !self.tuning_stop_requested(generation) && !stop_sent {
                            self.fail_tuning_experiment(generation, format!("Motor left RUN unexpectedly (state {state}); check firmware faults"));
                            return;
                        }
                        completed_window = false;
                        break;
                    }
                    Err(error) => { self.fail_tuning_experiment(generation, error); return; }
                }
                let step = TUNING_POLL.min(run_window.saturating_sub(elapsed));
                thread::sleep(step); elapsed += step;
            }
            if completed_window && self.read_motor_state().ok() == Some(2) {
                if let Err(error) = self.motion_stop() { self.fail_tuning_experiment(generation, error); return; }
                stop_sent = true;
                let _ = self.set_tuning_state(generation, TuningExperimentState::Stopping);
            }
        }
        let mut stop_deadline = if stop_sent { Some(Instant::now() + MOTOR_STOP_TIMEOUT) } else { None };
        loop {
            let stop_requested = self.tuning_stop_requested(generation);
            let motor_state = match self.read_motor_state() {
                Ok(state) => state,
                Err(error) => { self.fail_tuning_experiment(generation, error); return; }
            };
            if motor_state != 2 {
                if !stop_requested && !stop_sent {
                    self.fail_tuning_experiment(generation, format!("Motor left RUN unexpectedly (state {motor_state}); check firmware faults"));
                    return;
                }
                break;
            }
            if stop_sent && stop_deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                let mut message = "Motor is still RUN after the 60 s controlled-stop timeout; use Disable to turn off the drive".to_owned();
                if let Err(error) = self.scope_stop() { message.push_str(&format!("; Scope Stop: {error}")); }
                if let Ok(mut runtime) = self.inner.tuning_experiment.lock() {
                    if runtime.generation == generation {
                        runtime.state = TuningExperimentState::Failed;
                        runtime.message = Some(message);
                    }
                }
                return;
            }
            if stop_requested && !stop_sent {
                if let Err(error) = self.motion_stop() { self.fail_tuning_experiment(generation, error); return; }
                stop_sent = true;
                let _ = self.set_tuning_state(generation, TuningExperimentState::Stopping);
            }
            if !stop_sent {
                match self.scope_status() {
                    Ok(status) if status.capacity_samples > 0 => {
                        let used = status.samples as f64 / status.capacity_samples as f64;
                        if used >= 1.0 - reserve_ratio {
                            if let Err(error) = self.motion_stop() { self.fail_tuning_experiment(generation, error); return; }
                            stop_sent = true;
                            let _ = self.set_tuning_state(generation, TuningExperimentState::Stopping);
                            if let Ok(mut runtime) = self.inner.tuning_experiment.lock() {
                                if runtime.generation == generation { runtime.message = Some("Capture stopped at the 128 MiB recording limit".to_owned()); }
                            }
                        }
                    }
                    Ok(_) => {}
                    Err(error) => { self.fail_tuning_experiment(generation, error); return; }
                }
            }
            if stop_sent && stop_deadline.is_none() { stop_deadline = Some(Instant::now() + MOTOR_STOP_TIMEOUT); }
            thread::sleep(TUNING_POLL);
        }
        let _ = self.set_tuning_state(generation, TuningExperimentState::Stopping);
        self.wait_tuning(generation, TUNING_POST_CAPTURE, false);
        if let Err(error) = self.scope_stop() { self.fail_tuning_experiment(generation, error); return; }
        let _ = self.set_tuning_state(generation, TuningExperimentState::Completed);
    }
    fn with_scope<T>(&self, call: impl FnOnce(&MixedScopeSession) -> Result<T, MixedScopeError>) -> Result<T, ApplicationError> {
        let slot = self.inner.scope.lock().map_err(|_| ApplicationError::Poisoned)?;
        let scope = slot.as_ref().ok_or(ApplicationError::ScopeNotConfigured)?;
        Ok(call(scope)?)
    }
}
