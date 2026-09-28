use std::path::Path;
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

#[path = "motor_gate.rs"]
mod motor_gate;
use motor_gate::MotorGate;
use crate::action_completion::ActionCompletionWaiter;
use crate::automation::access::{WorkflowAccess, Permit};
use crate::config_service::ConfigService;
use crate::motor_actions::MotorActionService;
use crate::parameter_service::ParameterService;
use crate::preflight::PreflightService;
use crate::problems::ProblemService;
#[path = "acquisition.rs"]
mod acquisition;
use acquisition::{SharedAcquisition, View};
#[path = "tuning.rs"]
mod tuning;
use tuning::TuningExperimentRuntime;

use thiserror::Error;
use crate::{
    ActionHandle, AxdrStatus, CommissioningCapabilities, ConfigServiceError, DevicePlotCapabilities, DeviceSession,
    HostSchema, IdentificationKind, IdentificationStart, MixedScopeConfig, MixedScopeError,
    MixedScopeSnapshot, MixedScopeStatus, MotionCapabilities, MotionConfig, MotionMode, MotionPreview,
    MotorState, PositionCommand, MotionService, MotorActionError, ParameterMetadata,
    ParameterServiceError, ParameterValue, PlotCapabilitiesError, PreflightError, PreflightIssue,
    ProblemSnapshot, ProtectionEventFrame, ScopeRate, ScopeSelection, SessionError, SessionEvent, StreamState,
    parse_protection_event,
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
const MOTOR_STATE_POLL: Duration = Duration::from_millis(20);
const MOTOR_STOP_TIMEOUT: Duration = Duration::from_secs(60);

struct ApplicationInner {
    session: DeviceSession,
    schema: HostSchema,
    parameters: ParameterService,
    events: Mutex<Vec<mpsc::Sender<SessionEvent>>>,
    plot_capabilities: Mutex<Option<DevicePlotCapabilities>>,
    motion_capabilities: MotionCapabilities,
    commissioning_capabilities: CommissioningCapabilities,
    problems: ProblemService,
    motion: MotionService,
    scope: Mutex<Option<SharedAcquisition>>,
    tuning_experiment: Mutex<TuningExperimentRuntime>,
    motor_gate: MotorGate,
    workflow_access: WorkflowAccess,
    tuning_worker: Mutex<Option<thread::JoinHandle<()>>>,
}
#[derive(Clone)]
pub struct ApplicationSession { inner: Arc<ApplicationInner>, workflow_owner: Option<u64>, workflow_cleanup: bool }

impl ApplicationSession {
    pub fn open_usb(path: impl AsRef<Path>, baud_rate: u32, schema: HostSchema) -> Result<Self, ApplicationError> {
        Self::from_session(DeviceSession::open_usb(path, baud_rate)?, schema)
    }
    pub fn open_usb_with_motion(path: impl AsRef<Path>, baud_rate: u32, schema: HostSchema, motion: MotionService) -> Result<Self, ApplicationError> {
        Self::from_session_with_motion(DeviceSession::open_usb(path, baud_rate)?, schema, motion)
    }
    pub(crate) fn from_session(session: DeviceSession, schema: HostSchema) -> Result<Self, ApplicationError> {
        Self::from_session_with_motion(session, schema, MotionService::default())
    }
    pub(crate) fn from_session_with_motion(session: DeviceSession, schema: HostSchema, motion: MotionService) -> Result<Self, ApplicationError> {
        motion.reset_runtime().map_err(ApplicationError::Motion)?;
        let events = session.subscribe()?;
        let parameters = ParameterService::new(session.clone(), schema.clone());
        let motion_capabilities = MotionCapabilities::from_schema(&schema);
        let commissioning_capabilities = CommissioningCapabilities::from_schema(&schema);
        let problems = ProblemService::default();
        let app = Self { inner: Arc::new(ApplicationInner {
            session, schema, parameters, events: Mutex::new(Vec::new()),
            plot_capabilities: Mutex::new(None), motion_capabilities, commissioning_capabilities, problems, motion,
            scope: Mutex::new(None),
            tuning_experiment: Mutex::new(TuningExperimentRuntime::default()),
            motor_gate: MotorGate::default(), workflow_access: WorkflowAccess::default(), tuning_worker: Mutex::new(None),
        }), workflow_owner: None, workflow_cleanup: false };
        let weak = Arc::downgrade(&app.inner);
        thread::spawn(move || {
            while let Ok(event) = events.recv() {
                let Some(inner) = weak.upgrade() else { break; };
                match &event {
                    SessionEvent::ActionCompleted { .. } => {
                        // Application owns readback, even with no page/CLI subscriber. Failed
                        // reads are retained as cache errors instead of reviving old values.
                        let _ = inner.parameters.refresh_all();
                        inner.problems.sync_from_parameters(&inner.parameters, &inner.schema);
                    }
                    SessionEvent::DeviceEvent(frame) => {
                        if let Ok(Some(status)) = parse_protection_event(frame) {
                            let values = protection_parameter_values(&inner.schema, status);
                            let _ = inner.parameters.ingest_external_values(&values);
                            inner.problems.update_protection(status);
                        }
                    }
                    _ => {}
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
    pub fn is_closing(&self) -> bool { self.inner.motor_gate.is_closed() }
    pub fn runtime_stream_progress(&self) -> Result<crate::RuntimeStreamProgress, ApplicationError> {
        Ok(self.inner.parameters.runtime_stream_progress()?)
    }
    pub(crate) fn automation_claim(&self) -> Result<(Self, Arc<std::sync::atomic::AtomicBool>), ApplicationError> {
        let (id, cancelled) = self.inner.workflow_access.claim().map_err(ApplicationError::Motion)?;
        if let Err(error) = self.ensure_no_tuning() {
            self.inner.workflow_access.release(id);
            return Err(error);
        }
        Ok((Self { inner: self.inner.clone(), workflow_owner: Some(id), workflow_cleanup: false }, cancelled))
    }
    fn workflow_checkpoint(&self, ticket: u64) -> Result<(), String> {
        if self.inner.motor_gate.is_closed() || self.inner.motor_gate.ticket() != ticket {
            return Err("The pending operation was cancelled".into());
        }
        let permit = self.workflow_permit().map_err(|error| error.to_string())?;
        drop(permit);
        Ok(())
    }
    fn motor_actions_at(&self, ticket: u64) -> MotorActionService {
        let app = self.clone();
        self.motor_actions().with_checkpoint(Arc::new(move || app.workflow_checkpoint(ticket).is_ok()))
    }
    pub(crate) fn automation_release(&self) {
        if let Some(id) = self.workflow_owner { self.inner.workflow_access.release(id); }
    }
    fn workflow_permit(&self) -> Result<Permit<'_>, ApplicationError> {
        self.inner.workflow_access.enter(self.workflow_owner, self.workflow_cleanup).map_err(ApplicationError::Motion)
    }
    pub(crate) fn automation_cleanup_handle(&self) -> Self {
        Self { inner: self.inner.clone(), workflow_owner: self.workflow_owner, workflow_cleanup: true }
    }
    pub(crate) fn automation_wait_stopped(&self) -> Result<(), ApplicationError> { self.wait_motor_stopped() }
    pub(crate) fn automation_join_tuning(&self) -> Result<(), ApplicationError> {
        let worker = self.inner.tuning_worker.lock().map_err(|_| ApplicationError::Poisoned)?.take();
        if let Some(worker) = worker { worker.join().map_err(|_| ApplicationError::Motion("tuning worker panicked".into()))?; }
        Ok(())
    }
    pub fn plot_capabilities(&self) -> Result<DevicePlotCapabilities, ApplicationError> {
        let mut slot = self.inner.plot_capabilities.lock().map_err(|_| ApplicationError::Poisoned)?;
        if slot.is_none() { *slot = Some(DevicePlotCapabilities::discover(&self.inner.session)?); }
        Ok(slot.as_ref().expect("Plot capabilities initialized").clone())
    }
    pub fn motion_capabilities(&self) -> &MotionCapabilities { &self.inner.motion_capabilities }
    pub fn commissioning_capabilities(&self) -> &CommissioningCapabilities { &self.inner.commissioning_capabilities }
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
        let results = self.inner.parameters.refresh_all()?;
        self.inner.problems.sync_from_parameters(&self.inner.parameters, &self.inner.schema);
        Ok(results)
    }

    pub fn problems_snapshot(&self) -> ProblemSnapshot {
        self.inner.problems.snapshot()
    }

    pub fn problems_recheck(&self) -> Result<ProblemSnapshot, ApplicationError> {
        let ids = protection_parameter_ids(&self.inner.schema);
        if !ids.is_empty() {
            let results = self.inner.parameters.read_many(&ids)?;
            self.require_refresh(results)?;
            self.inner.problems.sync_from_parameters(&self.inner.parameters, &self.inner.schema);
        }
        Ok(self.inner.problems.snapshot())
    }

    pub fn problems_clear_history(&self) -> ProblemSnapshot {
        self.inner.problems.clear_history();
        self.inner.problems.snapshot()
    }
    pub fn parameter_write(&self, id: u16, value: ParameterValue) -> Result<ParameterValue, ApplicationError> {
        let _workflow = self.workflow_permit()?;
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
    fn guarded_action_start(&self, key: &str) -> Result<ActionHandle, ApplicationError> {
        let action = self.inner.schema.action_by_key(key).ok_or_else(|| ApplicationError::UnknownAction(key.to_owned()))?;
        let _workflow = self.workflow_permit()?;
        let ticket = self.inner.motor_gate.ticket();
        let _command = self.inner.motor_gate.start(ticket)
            .map_err(|message| ApplicationError::Motion(message.to_owned()))?;
        self.ensure_no_tuning()?;
        Ok(self.inner.session.action_start(action.id)?)
    }
    /// Expert compatibility entry for nmixxctl action start.
    ///
    /// Normal product workflows use semantic methods. This preserves the legacy
    /// CLI Action behavior without exposing raw transport/session ownership.
    pub fn expert_action_start(&self, key: &str) -> Result<ActionHandle, ApplicationError> {
        let action = self.inner.schema.action_by_key(key).ok_or_else(|| ApplicationError::UnknownAction(key.to_owned()))?;
        match action.symbol.as_str() {
            "ACTION_MOTOR_STOP" => return self.motor_stop(),
            "ACTION_MOTOR_DISABLE" => return self.motor_disable(),
            _ => {}
        }

        let handle = self.guarded_action_start(key)?;
        match action.symbol.as_str() {
            "ACTION_IDENT_APPLY" | "ACTION_POSITION_SET_ZERO" | "ACTION_PROTECTION_CLEAR" => {
                self.refresh_after_immediate_action()?;
            }
            "ACTION_MOTOR_ENABLE" => {
                self.refresh_motor_context()?;
            }
            _ => {}
        }
        Ok(handle)
    }
    pub fn action_completion_waiter(&self) -> Result<ActionCompletionWaiter, ApplicationError> {
        let (sender, receiver) = mpsc::channel();
        self.inner.events.lock().map_err(|_| ApplicationError::Poisoned)?.push(sender);
        Ok(ActionCompletionWaiter::new(receiver))
    }
    pub fn encoder_set_zero(&self) -> Result<ActionHandle, ApplicationError> {
        let handle = self.guarded_action_start("ACTION_POSITION_SET_ZERO")?;
        self.refresh_after_immediate_action()?;
        Ok(handle)
    }
    pub fn homing_start(&self) -> Result<ActionHandle, ApplicationError> {
        self.guarded_action_start("ACTION_HOME_START")
    }
    pub fn protection_clear(&self) -> Result<ActionHandle, ApplicationError> {
        let handle = self.guarded_action_start("ACTION_PROTECTION_CLEAR")?;
        self.refresh_after_immediate_action()?;
        Ok(handle)
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
        let _workflow = self.workflow_permit()?;
        let ticket = self.inner.motor_gate.ticket();
        let _command = self.inner.motor_gate.start(ticket)
            .map_err(|message| ApplicationError::Motion(message.to_owned()))?;
        self.ensure_no_tuning()?;
        let result = self.motor_actions_at(ticket).identification_start(kind, allow_enable)?;
        if matches!(&result, IdentificationStart::Started(_)) { self.refresh_motor_context()?; }
        Ok(result)
    }
    pub fn identification_apply(&self) -> Result<ActionHandle, ApplicationError> {
        let _workflow = self.workflow_permit()?;
        let handle = self.motor_actions().identification_apply()?;
        self.refresh_after_immediate_action()?;
        Ok(handle)
    }
    pub fn motor_enable(&self) -> Result<ActionHandle, ApplicationError> {
        let _workflow = self.workflow_permit()?;
        let ticket = self.inner.motor_gate.ticket();
        let _command = self.inner.motor_gate.start(ticket)
            .map_err(|message| ApplicationError::Motion(message.to_owned()))?;
        self.ensure_no_tuning()?;
        let handle = self.motor_actions_at(ticket).enable()?;
        self.refresh_motor_context()?;
        Ok(handle)
    }
    pub fn motor_stop(&self) -> Result<ActionHandle, ApplicationError> {
        if self.workflow_owner.is_none() { self.inner.workflow_access.revoke(); }
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
        if self.workflow_owner.is_none() { self.inner.workflow_access.revoke(); }
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
        let _workflow = self.workflow_permit()?;
        let ticket = self.inner.motor_gate.ticket();
        let _command = self.inner.motor_gate.start(ticket)
            .map_err(|message| ApplicationError::Motion(message.to_owned()))?;
        self.ensure_no_tuning()?;
        let handle = self.motor_actions_at(ticket).phase_search_start()?;
        self.refresh_motor_context()?;
        Ok(handle)
    }
    pub fn is_same_session(&self, other: &Self) -> bool { Arc::ptr_eq(&self.inner, &other.inner) }

    pub fn disconnect(&self) -> Result<(), ApplicationError> {
        self.inner.workflow_access.close();
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
            if let Err(error) = self.with_scope(|source| source.shutdown()) { failures.push(error.to_string()); }
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
                MotorState::Disabled | MotorState::Enabled => return Ok(()),
                MotorState::Run => {}
            }
            if Instant::now() >= deadline {
                return Err(ApplicationError::Motion("Motor is still RUN after the 60 s controlled-stop timeout; use Disable to turn off the drive".to_owned()));
            }
            thread::sleep(MOTOR_STATE_POLL);
        }
    }

    pub fn config_save_available(&self) -> bool {
        ConfigService::from_shared(self.inner.session.clone(), self.inner.schema.clone(), self.inner.parameters.clone()).save_available()
    }
    pub fn config_save(&self) -> Result<ActionHandle, ApplicationError> {
        let _workflow = self.workflow_permit()?;
        // AxDr executes NVS_Storage_Save_All before replying. There is no later
        // ACTION_COMPLETE for Save; a rejected/failed response is propagated.
        Ok(ConfigService::from_shared(self.inner.session.clone(), self.inner.schema.clone(), self.inner.parameters.clone()).save()?)
    }
    pub fn motion_get(&self) -> MotionConfig { self.inner.motion.get() }
    pub fn motion_set(&self, config: MotionConfig) -> Result<MotionConfig, ApplicationError> {
        let _workflow = self.workflow_permit()?;
        self.inner.motion.set(config).map_err(ApplicationError::Motion)
    }
    pub fn motion_preview(&self) -> Result<MotionPreview, ApplicationError> {
        self.inner.motion.preview_with_parameters(&self.inner.parameters, None).map_err(ApplicationError::Motion)
    }
    pub fn motion_run(&self) -> Result<ActionHandle, ApplicationError> {
        self.motion_run_at(self.inner.motor_gate.ticket(), None)
    }
    fn motion_run_at(&self, ticket: u64, experiment: Option<u64>) -> Result<ActionHandle, ApplicationError> {
        let _workflow = self.workflow_permit()?;
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
        let completion = self.action_completion_waiter()?;
        let config = self.inner.motion.get();
        let mode = self.read_motion_mode()?;
        let repeat_target = if mode == MotionMode::Position && config.repeat {
            let current = self.read_position_turns("PARAM_RUN_POSITION")?;
            let absolute_target = if config.position_command == PositionCommand::Absolute {
                Some(self.read_position_turns("PARAM_TARGET_POSITION")?)
            } else {
                None
            };
            self.inner.motion.repeat_target(current, absolute_target).map_err(ApplicationError::Motion)?
        } else {
            None
        };
        let handle = self.inner.motion.run_checked(&self.inner.parameters, &self.inner.session, repeat_target.map(|(target, _)| target), || self.workflow_checkpoint(ticket))
            .map_err(ApplicationError::Motion)?;
        if let Some((_, target_is_b)) = repeat_target {
            self.inner.motion.repeat_mark_started(target_is_b).map_err(ApplicationError::Motion)?;
            // Do not keep the whole connection alive while waiting for an Action.
            let weak = Arc::downgrade(&self.inner);
            thread::spawn(move || {
                let Ok(status) = completion.wait(handle, None) else { return; };
                if let Some(inner) = weak.upgrade() {
                    let _ = (ApplicationSession { inner, workflow_owner: None, workflow_cleanup: false }).motion_run_completed(status);
                }
            });
        }
        Ok(handle)
    }
    fn motion_run_completed(&self, status: AxdrStatus) -> Result<(), ApplicationError> {
        self.inner.motion.repeat_completed(status).map_err(ApplicationError::Motion)
    }
    pub fn motion_stop(&self) -> Result<ActionHandle, ApplicationError> {
        self.inner.motion.cancel_repeat_leg().map_err(ApplicationError::Motion)?;
        self.motor_stop()
    }
    /// Start only telemetry acquisition, never the motor. Called once on connection.
    pub fn runtime_start(&self) -> Result<Vec<u16>, ApplicationError> {
        if self.inner.motor_gate.is_closed() {
            return Err(ApplicationError::Motion("device connection is closing".to_owned()));
        }
        let capabilities = self.plot_capabilities()?;
        let mut slot = self.inner.scope.lock().map_err(|_| ApplicationError::Poisoned)?;
        if slot.is_none() {
            *slot = Some(SharedAcquisition::new(self.inner.session.clone(), capabilities,
                self.inner.schema.clone(), self.inner.parameters.clone())?);
        }
        Ok(slot.as_ref().ok_or(ApplicationError::ScopeNotConfigured)?.base_ids()?)
    }
    pub fn scope_configure(&self, selections: &[ScopeSelection], history: Duration, _config_id: u8) -> Result<MixedScopeConfig, ApplicationError> {
        let _workflow = self.workflow_permit()?;
        self.runtime_start()?;
        self.with_scope(|source| source.configure(View::Scope, selections, history))
    }
    pub fn scope_live(&self) -> Result<(), ApplicationError> {
        let _workflow = self.workflow_permit()?; self.with_scope(|source| source.live(View::Scope)) }
    pub fn scope_resume(&self) -> Result<(), ApplicationError> { self.scope_live() }
    pub fn scope_pause(&self) -> Result<(), ApplicationError> {
        let _workflow = self.workflow_permit()?; self.with_scope(|source| source.stop(View::Scope, StreamState::Paused)) }
    pub fn scope_stop(&self) -> Result<(), ApplicationError> {
        let _workflow = self.workflow_permit()?; self.with_scope(|source| source.stop(View::Scope, StreamState::Stopped)) }
    pub fn scope_clear(&self) -> Result<(), ApplicationError> {
        let _workflow = self.workflow_permit()?; self.with_scope(|source| source.clear(View::Scope)) }
    pub fn scope_capture(&self, duration: Duration) -> Result<(), ApplicationError> {
        let _workflow = self.workflow_permit()?; self.with_scope(|source| source.capture(View::Scope, duration)) }
    pub fn scope_status(&self) -> Result<MixedScopeStatus, ApplicationError> { self.with_scope(|source| source.status(View::Scope)) }
    pub fn scope_snapshot_tail(&self, window: Duration) -> Result<MixedScopeSnapshot, ApplicationError> { self.scope_snapshot_window(window, Duration::ZERO) }
    pub fn scope_snapshot_window(&self, window: Duration, end_offset: Duration) -> Result<MixedScopeSnapshot, ApplicationError> {
        self.with_scope(|source| source.snapshot(View::Scope, window, end_offset))
    }
    pub fn scope_recorded_duration(&self) -> Result<Duration, ApplicationError> { self.with_scope(|source| source.recorded_duration(View::Scope)) }
    pub fn scope_config(&self) -> Result<MixedScopeConfig, ApplicationError> { self.with_scope(|source| source.config(View::Scope)) }
    fn read_motion_mode(&self) -> Result<MotionMode, ApplicationError> {
        let metadata = self.inner.schema.parameter_by_key("PARAM_MOTOR_MODE").ok_or_else(|| ApplicationError::Motion("Motor mode is not exposed".to_owned()))?;
        match self.inner.parameters.read(metadata.id)? {
            ParameterValue::U8(value) => crate::motion::mode_from_wire_value(&self.inner.schema, value).map_err(ApplicationError::Motion),
            _ => Err(ApplicationError::Motion("Motor mode has an unexpected type".to_owned())),
        }
    }
    fn read_position_turns(&self, key: &str) -> Result<f64, ApplicationError> {
        let metadata = self.inner.schema.parameter_by_key(key).ok_or_else(|| ApplicationError::Motion(format!("{key} is not exposed")))?;
        match self.inner.parameters.read(metadata.id)? {
            ParameterValue::Position(value) => Ok(crate::motion::position_to_turns(value)),
            _ => Err(ApplicationError::Motion(format!("{key} has an unexpected type"))),
        }
    }
    pub fn motor_state(&self) -> Result<MotorState, ApplicationError> {
        self.read_motor_state()
    }
    fn read_motor_state(&self) -> Result<MotorState, ApplicationError> {
        let metadata = self.inner.schema.parameter_by_key("PARAM_MOTOR_STATE").ok_or_else(|| ApplicationError::Motion("Motor state is not exposed".to_owned()))?;
        match self.inner.parameters.read(metadata.id)? {
            ParameterValue::U8(value) => MotorState::from_parameter(metadata, value).map_err(ApplicationError::Motion),
            _ => Err(ApplicationError::Motion("Motor state has an unexpected type".to_owned())),
        }
    }
    fn with_scope<T>(&self, call: impl FnOnce(&SharedAcquisition) -> Result<T, MixedScopeError>) -> Result<T, ApplicationError> {
        let slot = self.inner.scope.lock().map_err(|_| ApplicationError::Poisoned)?;
        let scope = slot.as_ref().ok_or(ApplicationError::ScopeNotConfigured)?;
        Ok(call(scope)?)
    }
}


fn protection_parameter_ids(schema: &HostSchema) -> Vec<u16> {
    ["PARAM_EVENT_REPORT", "PARAM_EVENT_WARNING", "PARAM_EVENT_ERROR", "PARAM_EVENT_TRIP"]
        .into_iter()
        .filter_map(|symbol| schema.parameter_by_symbol(symbol).map(|metadata| metadata.id))
        .collect()
}

fn protection_parameter_values(schema: &HostSchema, status: ProtectionEventFrame) -> Vec<(u16, ParameterValue)> {
    [
        ("PARAM_EVENT_REPORT", status.report),
        ("PARAM_EVENT_WARNING", status.warning),
        ("PARAM_EVENT_ERROR", status.error),
        ("PARAM_EVENT_TRIP", status.trip),
    ]
    .into_iter()
    .filter_map(|(symbol, value)| schema.parameter_by_symbol(symbol).map(|metadata| (metadata.id, ParameterValue::U32(value))))
    .collect()
}
