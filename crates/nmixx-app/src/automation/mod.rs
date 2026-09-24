//! Automation and GUI compose the same ApplicationSession. CLI is a peer,
//! not an automation dependency. Scripts must be trusted local programs.
pub(crate) mod access;
mod runner;
mod session_api;
pub use runner::{AutomationApi, AutomationRuntime, AutomationSnapshot, AutomationState, LogLine, OperationEffect, RunControl, ScriptSpec};
pub use session_api::SessionWorkflowApi;
pub const DIAGNOSIS_SCRIPT: &str = include_str!("assets/runtime_diagnosis.py");
pub const MOTION_SCRIPT: &str = include_str!("assets/motion_workflow.py");
