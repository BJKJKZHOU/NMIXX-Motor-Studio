use std::collections::HashMap;

use thiserror::Error;

use crate::wire::{CanFdError, CanFdFrame};

use super::{can_id, ActionCompleteFrame, AxdrStatus, ResponseFrame, TransactionId, MSG_PARAMETER, NODE_ID_DEFAULT, PARAM_WRITE};

pub const PARAM_ACTION_TYPE: u8 = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ActionHandle {
    pub txn: TransactionId,
    pub action_id: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionState {
    AwaitingAcceptance,
    Running,
    Completed(AxdrStatus),
}

#[derive(Debug, Error)]
pub enum ActionError {
    #[error(transparent)]
    Frame(#[from] CanFdError),
    #[error("action response has invalid payload length")]
    InvalidLength,
    #[error("action response ID mismatch: expected 0x{expected:04x}, got 0x{actual:04x}")]
    IdMismatch { expected: u16, actual: u16 },
    #[error("action request failed with status {0:?}")]
    Status(AxdrStatus),
    #[error("action completion does not match a tracked action")]
    UnknownCompletion,
}

pub fn build_action_start(txn: TransactionId, action_id: u16, node_id: u8) -> Result<CanFdFrame, ActionError> {
    let payload = [
        txn.get(),
        PARAM_WRITE,
        action_id as u8,
        (action_id >> 8) as u8,
        PARAM_ACTION_TYPE,
    ];
    Ok(CanFdFrame::new(can_id(MSG_PARAMETER, node_id), &payload)?)
}

pub fn build_action_start_default(txn: TransactionId, action_id: u16) -> Result<CanFdFrame, ActionError> {
    build_action_start(txn, action_id, NODE_ID_DEFAULT)
}

pub fn parse_action_accept(response: &ResponseFrame, action_id: u16) -> Result<(), ActionError> {
    if response.status != AxdrStatus::Ok {
        return Err(ActionError::Status(response.status));
    }
    if response.data.len() < 2 {
        return Err(ActionError::InvalidLength);
    }
    let actual_id = u16::from_le_bytes([response.data[0], response.data[1]]);
    if actual_id != action_id {
        return Err(ActionError::IdMismatch { expected: action_id, actual: actual_id });
    }
    Ok(())
}

#[derive(Debug, Default)]
pub struct ActionTracker {
    states: HashMap<ActionHandle, ActionState>,
}

impl ActionTracker {
    pub fn track(&mut self, handle: ActionHandle) {
        self.states.insert(handle, ActionState::AwaitingAcceptance);
    }

    pub fn accepted(&mut self, handle: ActionHandle) {
        if let Some(state @ ActionState::AwaitingAcceptance) = self.states.get_mut(&handle) {
            *state = ActionState::Running;
        }
    }

    pub fn complete(&mut self, event: &ActionCompleteFrame) -> Result<ActionHandle, ActionError> {
        let handle = ActionHandle {
            txn: TransactionId::from_raw(event.txn),
            action_id: event.action_id,
        };
        let Some(state) = self.states.get_mut(&handle) else {
            return Err(ActionError::UnknownCompletion);
        };
        *state = ActionState::Completed(event.status);
        Ok(handle)
    }

    pub fn state(&self, handle: ActionHandle) -> Option<ActionState> {
        self.states.get(&handle).copied()
    }

    pub fn remove(&mut self, handle: ActionHandle) -> Option<ActionState> {
        self.states.remove(&handle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracks_accept_then_complete() {
        let handle = ActionHandle { txn: TransactionId::from_raw(3), action_id: 0x1102 };
        let mut tracker = ActionTracker::default();
        tracker.track(handle);
        tracker.accepted(handle);
        assert_eq!(tracker.state(handle), Some(ActionState::Running));
        tracker.complete(&ActionCompleteFrame { txn: 3, action_id: 0x1102, status: AxdrStatus::Ok }).unwrap();
        assert_eq!(tracker.state(handle), Some(ActionState::Completed(AxdrStatus::Ok)));
    }

    #[test]
    fn late_accept_does_not_overwrite_early_completion() {
        let handle = ActionHandle { txn: TransactionId::from_raw(4), action_id: 0x1004 };
        let mut tracker = ActionTracker::default();
        tracker.track(handle);
        tracker.complete(&ActionCompleteFrame { txn: 4, action_id: 0x1004, status: AxdrStatus::Ok }).unwrap();
        tracker.accepted(handle);
        assert_eq!(tracker.state(handle), Some(ActionState::Completed(AxdrStatus::Ok)));
    }
}
