//! Application semantics for NMIXX Motor Studio.
//!
//! Device primitives stay in `nmixx-core`; GUI, CLI and automation converge on
//! this layer. A `DeviceSession` owns one transport and exposes shared
//! application-facing access to that device.

mod application;
mod automation;
mod config_service;
mod motor_actions;
mod preflight;
mod connection;
mod parameter_service;
mod motion;
mod mixed_scope;
mod motion_capabilities;
mod plot_capabilities;
mod schema;
mod schema_store;
mod scope;
mod session;
mod stream;
mod stream_pipeline;

pub use automation::{AutomationApi, AutomationRuntime, AutomationSnapshot, AutomationState, LogLine, OperationEffect, RunControl, ScriptSpec, SessionWorkflowApi, DIAGNOSIS_SCRIPT, MOTION_SCRIPT};
pub use application::{
    ApplicationError, ApplicationSession, TuningExperimentSnapshot, TuningExperimentState,
    TuningExperimentStatus,
};
pub use config_service::{ConfigService, ConfigServiceError};
pub use connection::DEFAULT_USB_BAUD;
pub use parameter_service::{ParameterService, ParameterServiceError, RuntimeChannelProgress, RuntimeStreamProgress};
pub use motion::{MotionConfig, MotionMode, MotionPreview, MotionService, PositionCommand};
pub use motion_capabilities::MotionCapabilities;
pub use motor_actions::{IdentificationStart, MotorActionError, MotorActionService};
pub use mixed_scope::{
    MixedScopeChannel, MixedScopeConfig, MixedScopeError, MixedScopeSeries, MixedScopeSession,
    MixedScopeSnapshot, MixedScopeStatus, ScopeRate, ScopeSelection,
};
pub use preflight::{
    IdentificationKind, PreflightDomain, PreflightError, PreflightIssue, PreflightService,
};
pub use plot_capabilities::{
    DevicePlotCapabilities, DevicePlotChannel, PlotCapabilitiesError, PlotChannelInfo,
};
pub use schema::{
    ActionMetadata, HostSchema, ParameterMetadata, RangeMetadata, SchemaError, SchemaNumber,
    SchemaSource,
};
pub use schema_store::{SchemaStore, SchemaStoreError, StoredSchema, schema_store_key};
pub use scope::{ScopeChannel, ScopeConfig, ScopeError, ScopeSession, ScopeStatus};
pub use session::{DeviceSession, SessionError, SessionEvent};
pub use stream::{StreamConfig, StreamError, StreamSession, StreamSnapshot, StreamState};
pub use stream_pipeline::{
    StreamIngestReport, StreamPipeline, StreamPipelineError, StreamWireMode,
};

// These are application-facing domain value types/constants. Clients import
// them from `nmixx-app`; they do not depend on `nmixx-core` directly.
pub use nmixx_core::protocol::{
    ActionError, ActionHandle, AxdrStatus, PLOT_CAP_FAST, PLOT_CAP_NORMAL, PLOT_FAST_MASK,
    PLOT_GROUP_FAST, PLOT_GROUP_NORMAL, PLOT_NORMAL_MASK, ParameterType, ParameterValue,
    PositionValue, SequenceStatus,
};
