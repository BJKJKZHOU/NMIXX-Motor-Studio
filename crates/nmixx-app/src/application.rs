use std::path::Path;
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use thiserror::Error;

use crate::{
    ActionHandle, DevicePlotCapabilities, DeviceSession, HostSchema, MotionCapabilities,
    MotionConfig, MotionPreview, MotionService, ParameterMetadata, ParameterService,
    ParameterServiceError, ParameterValue, PlotCapabilitiesError, ScopeConfig, ScopeError,
    ScopeSession, ScopeStatus, SessionError, SessionEvent, StreamSnapshot,
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
    Scope(#[from] ScopeError),
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
    plot_capabilities: DevicePlotCapabilities,
    motion_capabilities: MotionCapabilities,
    motion: MotionService,
    scope: Mutex<Option<ScopeSession>>,
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
        let plot_capabilities = DevicePlotCapabilities::discover(&session)?;
        let parameters = ParameterService::new(session.clone(), schema.clone());
        let motion_capabilities = MotionCapabilities::from_schema(&schema);

        Ok(Self {
            inner: Arc::new(ApplicationInner {
                session,
                schema,
                parameters,
                plot_capabilities,
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

    pub fn plot_capabilities(&self) -> &DevicePlotCapabilities {
        &self.inner.plot_capabilities
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
        parameter_ids: &[u16],
        history: Duration,
        config_id: u8,
    ) -> Result<ScopeConfig, ApplicationError> {
        let scope = ScopeSession::from_fast_capabilities(
            self.inner.session.clone(),
            &self.inner.plot_capabilities,
            &self.inner.schema,
            parameter_ids,
            history,
            config_id,
        )?;
        let config = scope.config().clone();
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

    pub fn scope_status(&self) -> Result<ScopeStatus, ApplicationError> {
        self.with_scope(|scope| scope.status())
    }

    pub fn scope_snapshot(&self) -> Result<StreamSnapshot, ApplicationError> {
        self.with_scope(|scope| scope.snapshot())
    }

    pub fn scope_snapshot_tail(&self, max_samples: usize) -> Result<StreamSnapshot, ApplicationError> {
        self.with_scope(|scope| scope.snapshot_tail(max_samples))
    }

    pub fn scope_config(&self) -> Result<ScopeConfig, ApplicationError> {
        self.with_scope(|scope| Ok(scope.config().clone()))
    }

    fn with_scope<T>(
        &self,
        call: impl FnOnce(&ScopeSession) -> Result<T, ScopeError>,
    ) -> Result<T, ApplicationError> {
        let slot = self.inner.scope.lock().map_err(|_| ApplicationError::Poisoned)?;
        let scope = slot.as_ref().ok_or(ApplicationError::ScopeNotConfigured)?;
        Ok(call(scope)?)
    }
}
