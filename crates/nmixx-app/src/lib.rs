//! Application semantics for NMIXX Motor Studio.
//!
//! Device primitives stay in `nmixx-core`; GUI, CLI and automation converge on
//! this layer. A `DeviceSession` owns one transport and exposes shared
//! application-facing access to that device.

mod action_completion;
mod application;
mod automation;
mod commissioning_capabilities;
mod config_service;
mod motor_actions;
mod motor_state;
mod preflight;
mod connection;
mod parameter_service;
mod motion;
mod motion_commands;
mod mixed_scope;
mod motion_capabilities;
mod plot_capabilities;
mod schema;
mod schema_store;
mod session;
mod stream;
mod stream_pipeline;

pub use automation::{AutomationApi, AutomationRuntime, AutomationSnapshot, AutomationState, LogLine, OperationEffect, RunControl, ScriptSpec, SessionWorkflowApi, DIAGNOSIS_SCRIPT, ENCODER_TURN_SCRIPT, MOTION_SCRIPT};
pub use action_completion::{ActionCompletionError, ActionCompletionWaiter};
pub use application::{
    ApplicationError, ApplicationSession, TuningExperimentSnapshot, TuningExperimentState,
    TuningExperimentStatus,
};
pub use commissioning_capabilities::CommissioningCapabilities;
pub use config_service::ConfigServiceError;
pub use connection::DEFAULT_USB_BAUD;
pub use parameter_service::{ParameterServiceError, RuntimeChannelProgress, RuntimeStreamProgress};
pub use motion::{MotionConfig, MotionMode, MotionPreview, MotionService, PositionCommand};
pub use motion_commands::{MotionRuntimeStatus, PositionMotionRequest, SpeedMotionRequest};
pub use motion_capabilities::MotionCapabilities;
pub use motor_actions::{IdentificationStart, MotorActionError};
pub use motor_state::MotorState;
pub use mixed_scope::{
    MixedScopeChannel, MixedScopeConfig, MixedScopeError, MixedScopeSeries,
    MixedScopeSnapshot, MixedScopeStatus, ScopeRate, ScopeSelection,
};
pub use preflight::{IdentificationKind, PreflightDomain, PreflightError, PreflightIssue};
pub use plot_capabilities::{
    DevicePlotCapabilities, DevicePlotChannel, PlotCapabilitiesError, PlotChannelInfo,
};
pub use schema::{
    ActionMetadata, HostSchema, ParameterMetadata, RangeMetadata, SchemaError, SchemaNumber,
    SchemaSource,
};
pub use schema_store::{SchemaStore, SchemaStoreError, StoredSchema, schema_store_key};
pub use stream::StreamState;

// Internal modules keep short crate-local names while external low-level users
// opt in explicitly through nmixx_app::raw.
pub(crate) use mixed_scope::MixedScopeSession;
pub(crate) use session::{DeviceSession, SessionError, SessionEvent};
pub(crate) use stream::{StreamConfig, StreamError, StreamSession, StreamSnapshot};
pub(crate) use stream_pipeline::{
    StreamIngestReport, StreamPipeline, StreamPipelineError, StreamWireMode,
};

// These are application-facing domain value types/constants. Clients import
// them from `nmixx-app`; they do not depend on `nmixx-core` directly.
pub use nmixx_core::protocol::{
    ActionHandle, AxdrStatus, ParameterType, ParameterValue, PositionValue,
};
pub(crate) use nmixx_core::protocol::{
    ActionError, PLOT_CAP_FAST, PLOT_CAP_NORMAL, PLOT_FAST_MASK, PLOT_GROUP_FAST,
    PLOT_GROUP_NORMAL, PLOT_NORMAL_MASK, SequenceStatus,
};

/// Low-level device/session and stream primitives for protocol bring-up, smoke
/// tests and transport validation. Normal GUI/CLI/Automation code uses
/// ApplicationSession and the typed application-facing values at crate root.
pub mod raw {
    pub use crate::mixed_scope::MixedScopeSession;
    pub use crate::session::{DeviceSession, SessionError, SessionEvent};
    pub use crate::stream::{StreamConfig, StreamError, StreamSession, StreamSnapshot, StreamState};
    pub use crate::stream_pipeline::{
        StreamIngestReport, StreamPipeline, StreamPipelineError, StreamWireMode,
    };
    pub use nmixx_core::protocol::{
        ActionError, PLOT_CAP_FAST, PLOT_CAP_NORMAL, PLOT_FAST_MASK, PLOT_GROUP_FAST,
        PLOT_GROUP_NORMAL, PLOT_NORMAL_MASK, SequenceStatus,
    };
}
