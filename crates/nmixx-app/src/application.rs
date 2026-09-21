use std::path::Path;
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::Duration;

use thiserror::Error;

use crate::{
    ActionHandle, ConfigService, ConfigServiceError, DevicePlotCapabilities, DeviceSession,
    HostSchema, IdentificationKind, IdentificationStart, MixedScopeConfig, MixedScopeError, MixedScopeSession,
    MixedScopeSnapshot, MixedScopeStatus, MotionCapabilities, MotionConfig, MotionMode, MotionPreview,
    MotionService, MotorActionError, MotorActionService, ParameterMetadata, ParameterService,
    ParameterServiceError, ParameterValue, PlotCapabilitiesError, PreflightError, PreflightIssue,
    PreflightService, ScopeRate, ScopeSelection, SessionError, SessionEvent,
};

#[derive(Debug, Error)]
pub enum ApplicationError {
    #[error(transparent)]
    Session(#[from] SessionError),
    #[error(transparent)]
    PlotCapabilities(#[from] PlotCapabilitiesError),
    #[error(transparent)]
    Parameter(#[from] ParameterServiceError),
    #[error(transparent)]
    Scope(#[from] MixedScopeError),
    #[error(transparent)]
    MotorAction(#[from] MotorActionError),
    #[error(transparent)]
    Config(#[from] ConfigServiceError),
    #[error(transparent)]
    Preflight(#[from] PreflightError),
    #[error("application runtime state is poisoned")]
    Poisoned,
    #[error("unknown Action '{0}' in loaded Host schema")]
    UnknownAction(String),
    #[error("Scope is not configured")]
    ScopeNotConfigured,
    #[error("tuning experiment is already active")]
    TuningExperimentBusy,
    #[error("tuning experiment requires motor state ENABLED")]
    TuningExperimentMotorNotEnabled,
    #[error("required tuning Plot channel '{0}' is not available at the requested rate")]
    TuningExperimentChannel(String),
    #[error("{0}")]
    Motion(String),
}


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TuningExperimentState {
    Idle,
    Preparing,
    Running,
    Stopping,
    Completed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TuningExperimentStatus {
    pub state: TuningExperimentState,
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TuningExperimentSnapshot {
    pub status: TuningExperimentStatus,
    pub config: MixedScopeConfig,
    pub snapshot: MixedScopeSnapshot,
}

#[derive(Debug)]
struct TuningExperimentRuntime {
    generation: u64,
    state: TuningExperimentState,
    stop_requested: bool,
    message: Option<String>,
}

impl Default for TuningExperimentRuntime {
    fn default() -> Self {
        Self {
            generation: 0,
            state: TuningExperimentState::Idle,
            stop_requested: false,
            message: None,
        }
    }
}

const TUNING_CAPTURE_BUDGET_BYTES: usize = 128 * 1024 * 1024;
const TUNING_LIVE_WINDOW: Duration = Duration::from_secs(3);
const TUNING_PRE_CAPTURE: Duration = Duration::from_millis(500);
const TUNING_POST_CAPTURE: Duration = Duration::from_millis(750);
const TUNING_POSITION_SETTLE: Duration = Duration::from_millis(500);
const TUNING_POLL: Duration = Duration::from_millis(20);

struct ApplicationInner {
    session: DeviceSession,
    schema: HostSchema,
    parameters: ParameterService,
    plot_capabilities: Mutex<Option<DevicePlotCapabilities>>,
    motion_capabilities: MotionCapabilities,
    motion: MotionService,
    scope: Mutex<Option<MixedScopeSession>>,
    tuning_experiment: Mutex<TuningExperimentRuntime>,
}

#[derive(Clone)]
pub struct ApplicationSession {
    inner: Arc<ApplicationInner>,
}

impl ApplicationSession {
    pub fn open_usb(
        path: impl AsRef<Path>,
        baud_rate: u32,
        schema: HostSchema,
    ) -> Result<Self, ApplicationError> {
        let session = DeviceSession::open_usb(path, baud_rate)?;
        Self::from_session(session, schema)
    }

    pub fn open_usb_with_motion(
        path: impl AsRef<Path>,
        baud_rate: u32,
        schema: HostSchema,
        motion: MotionService,
    ) -> Result<Self, ApplicationError> {
        let session = DeviceSession::open_usb(path, baud_rate)?;
        Self::from_session_with_motion(session, schema, motion)
    }

    pub fn from_session(
        session: DeviceSession,
        schema: HostSchema,
    ) -> Result<Self, ApplicationError> {
        Self::from_session_with_motion(session, schema, MotionService::default())
    }

    pub fn from_session_with_motion(
        session: DeviceSession,
        schema: HostSchema,
        motion: MotionService,
    ) -> Result<Self, ApplicationError> {
        let parameters = ParameterService::new(session.clone(), schema.clone());
        let motion_capabilities = MotionCapabilities::from_schema(&schema);

        Ok(Self {
            inner: Arc::new(ApplicationInner {
                session,
                schema,
                parameters,
                plot_capabilities: Mutex::new(None),
                motion_capabilities,
                motion,
                scope: Mutex::new(None),
                tuning_experiment: Mutex::new(TuningExperimentRuntime::default()),
            }),
        })
    }

    pub fn available_usb_ports() -> Result<Vec<String>, ApplicationError> {
        Ok(DeviceSession::available_usb_ports()?)
    }

    pub fn schema(&self) -> &HostSchema {
        &self.inner.schema
    }

    pub fn plot_capabilities(&self) -> Result<DevicePlotCapabilities, ApplicationError> {
        let mut slot = self
            .inner
            .plot_capabilities
            .lock()
            .map_err(|_| ApplicationError::Poisoned)?;
        if slot.is_none() {
            *slot = Some(DevicePlotCapabilities::discover(&self.inner.session)?);
        }
        Ok(slot.as_ref().expect("Plot capabilities initialized").clone())
    }

    pub fn motion_capabilities(&self) -> &MotionCapabilities {
        &self.inner.motion_capabilities
    }

    pub fn parameter_metadata(&self) -> &[ParameterMetadata] {
        self.inner.parameters.parameters()
    }

    pub fn parameter_read(&self, id: u16) -> Result<ParameterValue, ApplicationError> {
        Ok(self.inner.parameters.read(id)?)
    }

    pub fn parameter_read_many(
        &self,
        ids: &[u16],
    ) -> Result<Vec<(u16, Result<ParameterValue, ParameterServiceError>)>, ApplicationError> {
        Ok(self.inner.parameters.read_many(ids)?)
    }

    pub fn parameter_cached(&self, id: u16) -> Result<Option<ParameterValue>, ApplicationError> {
        Ok(self.inner.parameters.cached(id)?)
    }

    pub fn parameter_refresh_all(
        &self,
    ) -> Result<Vec<(u16, Result<ParameterValue, ParameterServiceError>)>, ApplicationError> {
        Ok(self.inner.parameters.refresh_all()?)
    }

    pub fn parameter_write(
        &self,
        id: u16,
        value: ParameterValue,
    ) -> Result<ParameterValue, ApplicationError> {
        Ok(self.inner.parameters.write_readback(id, value)?)
    }

    pub fn action_start(&self, key: &str) -> Result<ActionHandle, ApplicationError> {
        let action = self
            .inner
            .schema
            .action_by_key(key)
            .ok_or_else(|| ApplicationError::UnknownAction(key.to_owned()))?;
        Ok(self.inner.session.action_start(action.id)?)
    }

    pub fn subscribe(&self) -> Result<mpsc::Receiver<SessionEvent>, ApplicationError> {
        Ok(self.inner.session.subscribe()?)
    }

    pub fn preflight_phase_search(&self) -> Result<Vec<PreflightIssue>, ApplicationError> {
        Ok(PreflightService::new(self.inner.parameters.clone()).check_phase_search()?)
    }

    pub fn preflight_identification(
        &self,
        kind: IdentificationKind,
    ) -> Result<Vec<PreflightIssue>, ApplicationError> {
        Ok(PreflightService::new(self.inner.parameters.clone()).check_identification(kind)?)
    }

    pub fn identification_start(
        &self,
        kind: IdentificationKind,
        allow_enable: bool,
    ) -> Result<IdentificationStart, ApplicationError> {
        Ok(MotorActionService::from_shared(
            self.inner.session.clone(),
            self.inner.schema.clone(),
            self.inner.parameters.clone(),
        )
        .identification_start(kind, allow_enable)?)
    }

    pub fn identification_apply(&self) -> Result<ActionHandle, ApplicationError> {
        Ok(MotorActionService::from_shared(
            self.inner.session.clone(),
            self.inner.schema.clone(),
            self.inner.parameters.clone(),
        )
        .identification_apply()?)
    }

    pub fn motor_enable(&self) -> Result<ActionHandle, ApplicationError> {
        Ok(MotorActionService::from_shared(
            self.inner.session.clone(),
            self.inner.schema.clone(),
            self.inner.parameters.clone(),
        ).enable()?)
    }

    pub fn motor_stop(&self) -> Result<ActionHandle, ApplicationError> {
        Ok(MotorActionService::from_shared(
            self.inner.session.clone(),
            self.inner.schema.clone(),
            self.inner.parameters.clone(),
        ).stop()?)
    }

    pub fn motor_disable(&self) -> Result<ActionHandle, ApplicationError> {
        Ok(MotorActionService::from_shared(
            self.inner.session.clone(),
            self.inner.schema.clone(),
            self.inner.parameters.clone(),
        ).disable()?)
    }

    pub fn phase_search_start(&self) -> Result<ActionHandle, ApplicationError> {
        Ok(MotorActionService::from_shared(
            self.inner.session.clone(),
            self.inner.schema.clone(),
            self.inner.parameters.clone(),
        )
            .phase_search_start()?)
    }

    pub fn config_save_available(&self) -> bool {
        ConfigService::from_shared(
            self.inner.session.clone(),
            self.inner.schema.clone(),
            self.inner.parameters.clone(),
        ).save_available()
    }

    pub fn config_save(&self) -> Result<ActionHandle, ApplicationError> {
        Ok(ConfigService::from_shared(
            self.inner.session.clone(),
            self.inner.schema.clone(),
            self.inner.parameters.clone(),
        ).save()?)
    }

    pub fn motion_get(&self) -> MotionConfig {
        self.inner.motion.get()
    }

    pub fn motion_set(&self, config: MotionConfig) -> Result<MotionConfig, ApplicationError> {
        self.inner.motion.set(config).map_err(ApplicationError::Motion)
    }

    pub fn motion_preview(&self) -> Result<MotionPreview, ApplicationError> {
        let effective_limit = self
            .inner
            .schema
            .parameter_by_key("PARAM_LIMIT_WM_EFFECTIVE")
            .map(|metadata| self.inner.parameters.read(metadata.id))
            .transpose()?
            .and_then(|value| match value {
                ParameterValue::F32(value) => Some(f64::from(value)),
                _ => None,
            });

        self.inner
            .motion
            .preview_with_speed_limit(effective_limit)
            .map_err(ApplicationError::Motion)
    }

    pub fn motion_run(&self) -> Result<ActionHandle, ApplicationError> {
        self.inner
            .motion
            .run(&self.inner.parameters, &self.inner.session)
            .map_err(ApplicationError::Motion)
    }

    pub fn motion_stop(&self) -> Result<ActionHandle, ApplicationError> {
        self.motor_stop()
    }

    pub fn scope_configure(
        &self,
        selections: &[ScopeSelection],
        history: Duration,
        config_id: u8,
    ) -> Result<MixedScopeConfig, ApplicationError> {
        let old_scope = {
            let mut slot = self.inner.scope.lock().map_err(|_| ApplicationError::Poisoned)?;
            if let Some(scope) = slot.as_ref() {
                let current = scope.config()?;
                if current.history == history {
                    return Ok(scope.reconfigure(selections)?);
                }
                let _ = scope.stop();
            }
            slot.take()
        };
        drop(old_scope);

        let plot_capabilities = self.plot_capabilities()?;
        let scope = MixedScopeSession::from_capabilities(
            self.inner.session.clone(),
            &plot_capabilities,
            &self.inner.schema,
            selections,
            history,
            config_id,
        )?;
        let config = scope.config()?;
        let mut slot = self.inner.scope.lock().map_err(|_| ApplicationError::Poisoned)?;
        *slot = Some(scope);
        Ok(config)
    }

    pub fn scope_live(&self) -> Result<(), ApplicationError> {
        self.with_scope(|scope| scope.live())
    }

    pub fn scope_resume(&self) -> Result<(), ApplicationError> {
        self.with_scope(|scope| scope.resume())
    }

    pub fn scope_pause(&self) -> Result<(), ApplicationError> {
        self.with_scope(|scope| scope.pause())
    }

    pub fn scope_stop(&self) -> Result<(), ApplicationError> {
        self.with_scope(|scope| scope.stop())
    }

    pub fn scope_clear(&self) -> Result<(), ApplicationError> {
        self.with_scope(|scope| scope.clear())
    }

    pub fn scope_capture(&self, duration: Duration) -> Result<(), ApplicationError> {
        self.with_scope(|scope| scope.capture(duration))
    }

    pub fn scope_status(&self) -> Result<MixedScopeStatus, ApplicationError> {
        self.with_scope(|scope| scope.status())
    }

    pub fn scope_snapshot_tail(&self, window: Duration) -> Result<MixedScopeSnapshot, ApplicationError> {
        self.with_scope(|scope| scope.snapshot_tail(window))
    }

    pub fn scope_snapshot_window(
        &self,
        window: Duration,
        end_offset: Duration,
    ) -> Result<MixedScopeSnapshot, ApplicationError> {
        self.with_scope(|scope| scope.snapshot_window(window, end_offset))
    }

    pub fn scope_recorded_duration(&self) -> Result<Duration, ApplicationError> {
        self.with_scope(|scope| scope.recorded_duration())
    }

    pub fn scope_config(&self) -> Result<MixedScopeConfig, ApplicationError> {
        self.with_scope(|scope| scope.config())
    }

    pub fn tuning_experiment_default_selections(&self) -> Result<Vec<ScopeSelection>, ApplicationError> {
        let motion = self.inner.motion.get();
        self.tuning_default_selections(motion.mode)
    }

    fn tuning_capture_duration(
        &self,
        selections: &[ScopeSelection],
    ) -> Result<Duration, ApplicationError> {
        let capabilities = self.plot_capabilities()?;
        let mut samples_per_second = 0u64;
        for selection in selections {
            let channel = capabilities
                .channel(selection.id)
                .ok_or_else(|| ApplicationError::TuningExperimentChannel(format!("0x{:04X}", selection.id)))?;
            let rate = match selection.rate {
                ScopeRate::Fast => capabilities.fast_rate_hz,
                ScopeRate::Normal => capabilities.normal_rate_hz,
            };
            let supported = match selection.rate {
                ScopeRate::Fast => channel.supports_fast(),
                ScopeRate::Normal => channel.supports_normal(),
            };
            if !supported {
                return Err(ApplicationError::TuningExperimentChannel(format!("0x{:04X}", selection.id)));
            }
            samples_per_second = samples_per_second.saturating_add(u64::from(rate));
        }

        if samples_per_second == 0 {
            return Err(ApplicationError::TuningExperimentChannel("no capture channels selected".to_owned()));
        }

        let bytes_per_second = samples_per_second
            .saturating_mul(std::mem::size_of::<f32>() as u64);
        let seconds = TUNING_CAPTURE_BUDGET_BYTES as f64 / bytes_per_second as f64;
        Ok(Duration::from_secs_f64(seconds.max(TUNING_PRE_CAPTURE.as_secs_f64() + TUNING_POST_CAPTURE.as_secs_f64())))
    }

    pub fn tuning_experiment_start(
        &self,
        selections: &[ScopeSelection],
    ) -> Result<TuningExperimentStatus, ApplicationError> {
        if self.read_motor_state()? != 1 {
            return Err(ApplicationError::TuningExperimentMotorNotEnabled);
        }

        {
            let runtime = self
                .inner
                .tuning_experiment
                .lock()
                .map_err(|_| ApplicationError::Poisoned)?;
            if matches!(
                runtime.state,
                TuningExperimentState::Preparing
                    | TuningExperimentState::Running
                    | TuningExperimentState::Stopping
            ) {
                return Err(ApplicationError::TuningExperimentBusy);
            }
        }

        let motion = self.inner.motion.get();
        let preview_duration = if motion.mode == MotionMode::Position && !motion.repeat {
            self.motion_preview()?
                .times
                .last()
                .copied()
                .unwrap_or(0.0)
                .max(0.0)
        } else {
            0.0
        };

        let scope_exists = self
            .inner
            .scope
            .lock()
            .map_err(|_| ApplicationError::Poisoned)?
            .is_some();
        if scope_exists {
            self.scope_stop()?;
        }

        let capture_duration = self.tuning_capture_duration(selections)?;
        self.scope_configure(selections, capture_duration, 2)?;
        self.scope_capture(capture_duration)?;

        let generation = {
            let mut runtime = self
                .inner
                .tuning_experiment
                .lock()
                .map_err(|_| ApplicationError::Poisoned)?;
            runtime.generation = runtime.generation.wrapping_add(1);
            runtime.state = TuningExperimentState::Preparing;
            runtime.stop_requested = false;
            runtime.message = None;
            runtime.generation
        };

        let app = self.clone();
        thread::spawn(move || {
            app.run_tuning_experiment(generation, motion, preview_duration, capture_duration);
        });

        self.tuning_experiment_status()
    }

    pub fn tuning_experiment_stop(&self) -> Result<TuningExperimentStatus, ApplicationError> {
        {
            let mut runtime = self
                .inner
                .tuning_experiment
                .lock()
                .map_err(|_| ApplicationError::Poisoned)?;
            if matches!(
                runtime.state,
                TuningExperimentState::Preparing
                    | TuningExperimentState::Running
                    | TuningExperimentState::Stopping
            ) {
                runtime.stop_requested = true;
                if runtime.state == TuningExperimentState::Running {
                    runtime.state = TuningExperimentState::Stopping;
                }
            }
        }

        self.motion_stop()?;
        self.tuning_experiment_status()
    }

    pub fn tuning_experiment_status(&self) -> Result<TuningExperimentStatus, ApplicationError> {
        let runtime = self
            .inner
            .tuning_experiment
            .lock()
            .map_err(|_| ApplicationError::Poisoned)?;
        Ok(TuningExperimentStatus {
            state: runtime.state,
            message: runtime.message.clone(),
        })
    }

    pub fn tuning_experiment_snapshot(&self) -> Result<TuningExperimentSnapshot, ApplicationError> {
        let status = self.tuning_experiment_status()?;
        let config = self.scope_config()?;
        let window = if matches!(
            status.state,
            TuningExperimentState::Preparing
                | TuningExperimentState::Running
                | TuningExperimentState::Stopping
        ) {
            TUNING_LIVE_WINDOW.min(config.history)
        } else {
            config.history
        };
        let snapshot = self.scope_snapshot_tail(window)?;
        Ok(TuningExperimentSnapshot {
            status,
            config,
            snapshot,
        })
    }

    fn tuning_default_selections(
        &self,
        mode: MotionMode,
    ) -> Result<Vec<ScopeSelection>, ApplicationError> {
        let mut selections = vec![
            self.tuning_selection("PARAM_REF_IQ", ScopeRate::Fast)?,
            self.tuning_selection("PARAM_RUN_IQ", ScopeRate::Fast)?,
        ];

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
            MotionMode::Torque | MotionMode::Mit => {}
        }

        Ok(selections)
    }

    fn tuning_selection(
        &self,
        key: &str,
        rate: ScopeRate,
    ) -> Result<ScopeSelection, ApplicationError> {
        let metadata = self
            .inner
            .schema
            .parameter_by_key(key)
            .ok_or_else(|| ApplicationError::TuningExperimentChannel(key.to_owned()))?;
        let capabilities = self.plot_capabilities()?;
        let channel = capabilities
            .channel(metadata.id)
            .ok_or_else(|| ApplicationError::TuningExperimentChannel(metadata.label.clone()))?;
        let supported = match rate {
            ScopeRate::Fast => channel.supports_fast(),
            ScopeRate::Normal => channel.supports_normal(),
        };
        if !supported {
            return Err(ApplicationError::TuningExperimentChannel(metadata.label.clone()));
        }
        Ok(ScopeSelection {
            id: metadata.id,
            rate,
        })
    }

    fn read_motor_state(&self) -> Result<u8, ApplicationError> {
        let metadata = self
            .inner
            .schema
            .parameter_by_key("PARAM_MOTOR_STATE")
            .ok_or_else(|| ApplicationError::Motion("Motor state is not exposed".to_owned()))?;
        match self.inner.parameters.read(metadata.id)? {
            ParameterValue::U8(value) => Ok(value),
            _ => Err(ApplicationError::Motion(
                "Motor state has an unexpected type".to_owned(),
            )),
        }
    }

    fn tuning_stop_requested(&self, generation: u64) -> bool {
        self.inner
            .tuning_experiment
            .lock()
            .map(|runtime| runtime.generation != generation || runtime.stop_requested)
            .unwrap_or(true)
    }

    fn set_tuning_state(&self, generation: u64, state: TuningExperimentState) -> bool {
        let Ok(mut runtime) = self.inner.tuning_experiment.lock() else {
            return false;
        };
        if runtime.generation != generation {
            return false;
        }
        runtime.state = state;
        true
    }

    fn fail_tuning_experiment(&self, generation: u64, error: impl ToString) {
        let _ = self.scope_stop();
        if let Ok(mut runtime) = self.inner.tuning_experiment.lock() {
            if runtime.generation == generation {
                runtime.state = TuningExperimentState::Failed;
                runtime.message = Some(error.to_string());
            }
        }
    }

    fn wait_tuning(
        &self,
        generation: u64,
        duration: Duration,
        stop_sensitive: bool,
    ) -> bool {
        let mut elapsed = Duration::ZERO;
        while elapsed < duration {
            if stop_sensitive && self.tuning_stop_requested(generation) {
                return false;
            }
            let step = TUNING_POLL.min(duration.saturating_sub(elapsed));
            thread::sleep(step);
            elapsed += step;
        }
        true
    }

    fn run_tuning_experiment(
        &self,
        generation: u64,
        motion: MotionConfig,
        preview_duration: f64,
        capture_duration: Duration,
    ) {
        if !self.wait_tuning(generation, TUNING_PRE_CAPTURE, true) {
            let _ = self.scope_stop();
            let _ = self.set_tuning_state(generation, TuningExperimentState::Completed);
            return;
        }

        if let Err(error) = self.motion_run() {
            self.fail_tuning_experiment(generation, error);
            return;
        }
        if !self.set_tuning_state(generation, TuningExperimentState::Running) {
            return;
        }

        let auto_position = motion.mode == MotionMode::Position && !motion.repeat;
        let mut stop_sent = false;
        let reserve_ratio = (TUNING_POST_CAPTURE.as_secs_f64() / capture_duration.as_secs_f64())
            .clamp(0.0, 1.0);

        if auto_position {
            let run_window =
                Duration::from_secs_f64(preview_duration) + TUNING_POSITION_SETTLE;
            let mut elapsed = Duration::ZERO;
            let mut completed_window = true;

            while elapsed < run_window {
                if self.tuning_stop_requested(generation) {
                    completed_window = false;
                    break;
                }
                match self.scope_status() {
                    Ok(status) if status.capacity_samples > 0 => {
                        let used = status.samples as f64 / status.capacity_samples as f64;
                        if used >= 1.0 - reserve_ratio {
                            completed_window = false;
                            if self.read_motor_state().ok() == Some(2) {
                                if let Err(error) = self.motion_stop() {
                                    self.fail_tuning_experiment(generation, error);
                                    return;
                                }
                                stop_sent = true;
                                let _ = self.set_tuning_state(generation, TuningExperimentState::Stopping);
                            }
                            break;
                        }
                    }
                    Ok(_) => {}
                    Err(error) => {
                        self.fail_tuning_experiment(generation, error);
                        return;
                    }
                }
                match self.read_motor_state() {
                    Ok(2) => {}
                    Ok(_) => {
                        completed_window = false;
                        break;
                    }
                    Err(error) => {
                        self.fail_tuning_experiment(generation, error);
                        return;
                    }
                }

                let step = TUNING_POLL.min(run_window.saturating_sub(elapsed));
                thread::sleep(step);
                elapsed += step;
            }

            if completed_window && self.read_motor_state().ok() == Some(2) {
                if let Err(error) = self.motion_stop() {
                    self.fail_tuning_experiment(generation, error);
                    return;
                }
                stop_sent = true;
                let _ = self.set_tuning_state(generation, TuningExperimentState::Stopping);
            }
        }

        loop {
            let stop_requested = self.tuning_stop_requested(generation);
            let motor_state = match self.read_motor_state() {
                Ok(state) => state,
                Err(error) => {
                    self.fail_tuning_experiment(generation, error);
                    return;
                }
            };

            if motor_state != 2 {
                break;
            }

            if stop_requested && !stop_sent {
                if let Err(error) = self.motion_stop() {
                    self.fail_tuning_experiment(generation, error);
                    return;
                }
                stop_sent = true;
                let _ = self.set_tuning_state(generation, TuningExperimentState::Stopping);
            }

            if !stop_sent {
                match self.scope_status() {
                    Ok(status) if status.capacity_samples > 0 => {
                        let used = status.samples as f64 / status.capacity_samples as f64;
                        if used >= 1.0 - reserve_ratio {
                            if let Err(error) = self.motion_stop() {
                                self.fail_tuning_experiment(generation, error);
                                return;
                            }
                            stop_sent = true;
                            let _ = self.set_tuning_state(generation, TuningExperimentState::Stopping);
                            if let Ok(mut runtime) = self.inner.tuning_experiment.lock() {
                                if runtime.generation == generation {
                                    runtime.message = Some("Capture stopped at the 128 MiB recording limit".to_owned());
                                }
                            }
                        }
                    }
                    Ok(_) => {}
                    Err(error) => {
                        self.fail_tuning_experiment(generation, error);
                        return;
                    }
                }
            }

            thread::sleep(TUNING_POLL);
        }

        let _ = self.set_tuning_state(generation, TuningExperimentState::Stopping);
        self.wait_tuning(generation, TUNING_POST_CAPTURE, false);

        if let Err(error) = self.scope_stop() {
            self.fail_tuning_experiment(generation, error);
            return;
        }

        let _ = self.set_tuning_state(generation, TuningExperimentState::Completed);
    }

    fn with_scope<T>(
        &self,
        call: impl FnOnce(&MixedScopeSession) -> Result<T, MixedScopeError>,
    ) -> Result<T, ApplicationError> {
        let slot = self.inner.scope.lock().map_err(|_| ApplicationError::Poisoned)?;
        let scope = slot.as_ref().ok_or(ApplicationError::ScopeNotConfigured)?;
        Ok(call(scope)?)
    }
}
