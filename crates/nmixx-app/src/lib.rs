//! Application semantics for NMIXX Motor Studio.
//!
//! Device primitives stay in `nmixx-core`; GUI, CLI and automation converge on
//! this layer. A `DeviceSession` owns one transport and exposes shared
//! application-facing access to that device.

mod connection;
mod schema;
mod session;

pub use connection::DEFAULT_USB_BAUD;
pub use schema::{
    ActionMetadata, HostSchema, ParameterMetadata, RangeMetadata, SchemaError, SchemaNumber,
    SchemaSource,
};
pub use session::{DeviceSession, SessionError, SessionEvent};

// These are application-facing domain value types. Clients import them from
// `nmixx-app`; they do not depend on `nmixx-core` directly.
pub use nmixx_core::protocol::{
    ActionHandle, AxdrStatus, ParameterType, ParameterValue, PositionValue,
};
