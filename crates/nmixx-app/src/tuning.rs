use std::thread;
use std::time::{Duration, Instant};

use crate::{
    MixedScopeConfig, MixedScopeSnapshot, MixedScopeStatus, MotionMode, ScopeRate, ScopeSelection,
    StreamState,
};
use super::{
    ApplicationError, ApplicationSession, MotorState, TuningExperimentSnapshot,
    TuningExperimentState, TuningExperimentStatus, View, MOTOR_STOP_TIMEOUT,
};

#[derive(Debug)]
pub(super) struct TuningExperimentRuntime {
    pub(super) generation: u64,
    pub(super) state: TuningExperimentState,
    pub(super) stop_requested: bool,
    pub(super) message: Option<String>,
}
impl Default for TuningExperimentRuntime {
    fn default() -> Self {
        Self { generation: 0, state: TuningExperimentState::Idle, stop_requested: false, message: None }
    }
}

const TUNING_CAPTURE_BUDGET_BYTES: usize = 128 * 1024 * 1024;
const TUNING_LIVE_WINDOW: Duration = Duration::from_secs(3);
const TUNING_PRE_CAPTURE: Duration = Duration::from_millis(500);
const TUNING_POST_CAPTURE: Duration = Duration::from_millis(750);
const TUNING_POSITION_SETTLE: Duration = Duration::from_millis(500);
const TUNING_POLL: Duration = Duration::from_millis(20);

impl ApplicationSession {
    pub fn tuning_record_config(&self) -> Result<MixedScopeConfig, ApplicationError> { self.with_scope(|source| source.config(View::Tuning)) }
    pub fn tuning_record_status(&self) -> Result<MixedScopeStatus, ApplicationError> { self.with_scope(|source| source.status(View::Tuning)) }
    pub fn tuning_record_duration(&self) -> Result<Duration, ApplicationError> { self.with_scope(|source| source.recorded_duration(View::Tuning)) }
    pub fn tuning_record_window(&self, window: Duration, end_offset: Duration) -> Result<MixedScopeSnapshot, ApplicationError> {
        self.with_scope(|source| source.snapshot(View::Tuning, window, end_offset))
    }
    fn tuning_record_stop(&self) -> Result<(), ApplicationError> {
        self.with_scope(|source| source.stop(View::Tuning, StreamState::Stopped))
    }
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
        let bytes = TUNING_CAPTURE_BUDGET_BYTES.saturating_sub(selections.len() * 8);
        let seconds = bytes as f64 / bytes_per_second as f64;
        Ok(Duration::from_secs_f64(seconds.max(TUNING_PRE_CAPTURE.as_secs_f64() + TUNING_POST_CAPTURE.as_secs_f64())))
    }
    pub fn tuning_experiment_start(&self, selections: &[ScopeSelection]) -> Result<TuningExperimentStatus, ApplicationError> {
        let _workflow = self.workflow_permit()?;
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
            if self.read_motor_state()? != MotorState::Enabled { return Err(ApplicationError::TuningExperimentMotorNotEnabled); }
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
            self.runtime_start()?;
            self.with_scope(|source| source.configure(View::Tuning, selections, capture_duration))?;
            capture_owned = true;
            self.with_scope(|source| source.capture(View::Tuning, capture_duration))?;
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
            let cleanup = if capture_owned { self.tuning_record_stop() } else { Ok(()) };
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
    pub(super) fn request_tuning_stop(&self) -> Result<(), ApplicationError> {
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
        let config = self.tuning_record_config()?;
        let window = if matches!(status.state, TuningExperimentState::Preparing | TuningExperimentState::Running | TuningExperimentState::Stopping) {
            TUNING_LIVE_WINDOW.min(config.history)
        } else { config.history };
        let snapshot = self.tuning_record_window(window, Duration::ZERO)?;
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
        if let Err(error) = self.tuning_record_stop() { failures.push(format!("Scope Stop: {error}")); }
        if let Ok(mut runtime) = self.inner.tuning_experiment.lock() {
            if runtime.generation == generation {
                runtime.state = TuningExperimentState::Failed;
                runtime.message = Some(failures.join("; "));
            }
        }
    }
    fn finish_cancelled_pre_capture(&self, generation: u64) {
        if let Err(error) = self.tuning_record_stop() {
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
                match self.tuning_record_status() {
                    Ok(status) if status.capacity_samples > 0 => {
                        let used = status.samples as f64 / status.capacity_samples as f64;
                        if used >= 1.0 - reserve_ratio {
                            completed_window = false;
                            if self.read_motor_state().ok() == Some(MotorState::Run) {
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
                    Ok(MotorState::Run) => {}
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
            if completed_window && self.read_motor_state().ok() == Some(MotorState::Run) {
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
            if motor_state != MotorState::Run {
                if !stop_requested && !stop_sent {
                    self.fail_tuning_experiment(generation, format!("Motor left RUN unexpectedly (state {motor_state}); check firmware faults"));
                    return;
                }
                break;
            }
            if stop_sent && stop_deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                let mut message = "Motor is still RUN after the 60 s controlled-stop timeout; use Disable to turn off the drive".to_owned();
                if let Err(error) = self.tuning_record_stop() { message.push_str(&format!("; Scope Stop: {error}")); }
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
                match self.tuning_record_status() {
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
        if let Err(error) = self.tuning_record_stop() { self.fail_tuning_experiment(generation, error); return; }
        let _ = self.set_tuning_state(generation, TuningExperimentState::Completed);
    }
}
