//! Application semantics for NMIXX Motor Studio.
//!
//! Device primitives stay in `nmixx-core`; GUI, CLI and automation converge on
//! this layer. A `DeviceSession` owns one transport and exposes shared
//! application-facing access to that device.

mod session;

pub use session::{DeviceSession, SessionError, SessionEvent};
