//! Application semantics for NMIXX Motor Studio.
//!
//! Device primitives stay in `nmixx-core`; GUI, CLI and automation converge on
//! this layer. A `DeviceSession` owns one transport and exposes shared
//! application-facing access to that device.

mod connection;
mod plot_capabilities;
mod schema;
mod schema_store;
mod scope;
mod session;
mod stream;
mod stream_pipeline;

pub use connection::DEFAULT_USB_BAUD;
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
