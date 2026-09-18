use std::path::Path;
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use thiserror::Error;

use crate::{
    ActionHandle, ConfigService, ConfigServiceError, DevicePlotCapabilities, DeviceSession,
    HostSchema, IdentificationKind, MixedScopeConfig, MixedScopeError, MixedScopeSession,
    MixedScopeSnapshot, MixedScopeStatus, MotionCapabilities, MotionConfig, MotionPreview,
    MotionService, MotorActionError, MotorActionService, ParameterMetadata, ParameterService,
    ParameterServiceError, ParameterValue, PlotCapabilitiesError, PreflightError, PreflightIssue,
    PreflightService, ScopeSelection, SessionError, SessionEvent,
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
    #[error("{0}")]
    Motion(String),
}

struct ApplicationInner {
    session: DeviceSession,
    schema: HostSchema,
    parameters: ParameterService,
    plot_capabilities: Mutex<Option<DevicePlotCapabilities>>,
    motion_capabilities: MotionCapabilities,
    motion: MotionService,
    scope: Mutex<Option<MixedScopeSession>>,
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
    ) -> Result<(), ApplicationError> {
        Ok(self.inner.parameters.write(id, value)?)
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
        self.inner.motion.preview().map_err(ApplicationError::Motion)
    }

    pub fn motion_run(&self) -> Result<ActionHandle, ApplicationError> {
        self.inner
            .motion
            .run(&self.inner.parameters, &self.inner.session)
            .map_err(ApplicationError::Motion)
    }

    pub fn motion_stop(&self) -> Result<ActionHandle, ApplicationError> {
        self.inner
            .motion
            .stop(&self.inner.parameters, &self.inner.session)
            .map_err(ApplicationError::Motion)
    }

    pub fn scope_configure(
        &self,
        selections: &[ScopeSelection],
        history: Duration,
        config_id: u8,
    ) -> Result<MixedScopeConfig, ApplicationError> {
        {
            let slot = self.inner.scope.lock().map_err(|_| ApplicationError::Poisoned)?;
            if let Some(scope) = slot.as_ref() {
                return Ok(scope.reconfigure(selections)?);
            }
        }

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

    pub fn scope_config(&self) -> Result<MixedScopeConfig, ApplicationError> {
        self.with_scope(|scope| scope.config())
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
