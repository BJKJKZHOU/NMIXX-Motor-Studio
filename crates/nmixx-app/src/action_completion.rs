use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use thiserror::Error;

use crate::{ActionHandle, AxdrStatus, SessionEvent};

#[derive(Debug, Error)]
pub enum ActionCompletionError {
    #[error("Action completion timed out")]
    Timeout,
    #[error("Application event stream closed while waiting for Action completion")]
    Closed,
    #[error("{0}")]
    Interrupted(String),
}

/// Subscription created before a finite Action starts. It owns no device I/O and
/// only filters the Application event stream for the matching Action handle.
pub struct ActionCompletionWaiter {
    events: Receiver<SessionEvent>,
}

impl ActionCompletionWaiter {
    pub(crate) fn new(events: Receiver<SessionEvent>) -> Self {
        Self { events }
    }

    pub fn wait(
        &self,
        handle: ActionHandle,
        timeout: Option<Duration>,
    ) -> Result<AxdrStatus, ActionCompletionError> {
        let deadline = timeout.and_then(|duration| Instant::now().checked_add(duration));
        loop {
            let event = match deadline {
                Some(deadline) => {
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    if remaining.is_zero() {
                        return Err(ActionCompletionError::Timeout);
                    }
                    match self.events.recv_timeout(remaining) {
                        Ok(event) => event,
                        Err(RecvTimeoutError::Timeout) => return Err(ActionCompletionError::Timeout),
                        Err(RecvTimeoutError::Disconnected) => return Err(ActionCompletionError::Closed),
                    }
                }
                None => self.events.recv().map_err(|_| ActionCompletionError::Closed)?,
            };

            if let SessionEvent::ActionCompleted { handle: completed, status } = event {
                if completed == handle {
                    return Ok(status);
                }
            }
        }
    }

    pub(crate) fn wait_checked(
        &self,
        handle: ActionHandle,
        deadline: Instant,
        mut check: impl FnMut() -> Result<(), String>,
    ) -> Result<AxdrStatus, ActionCompletionError> {
        loop {
            check().map_err(ActionCompletionError::Interrupted)?;
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(ActionCompletionError::Timeout);
            }
            match self.wait(handle, Some(remaining.min(Duration::from_millis(20)))) {
                Err(ActionCompletionError::Timeout) => {}
                result => return result,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    use nmixx_core::protocol::TransactionId;

    fn handle(txn: u8, action_id: u16) -> ActionHandle {
        ActionHandle { txn: TransactionId::from_raw(txn), action_id }
    }

    #[test]
    fn ignores_other_action_completions() {
        let (tx, rx) = mpsc::channel();
        let waiter = ActionCompletionWaiter::new(rx);
        let wanted = handle(7, 0x1102);
        tx.send(SessionEvent::ActionCompleted { handle: handle(6, 0x1101), status: AxdrStatus::ErrState }).unwrap();
        tx.send(SessionEvent::ActionCompleted { handle: wanted, status: AxdrStatus::Ok }).unwrap();
        assert_eq!(waiter.wait(wanted, Some(Duration::from_millis(10))).unwrap(), AxdrStatus::Ok);
    }

    #[test]
    fn reports_timeout_without_completion() {
        let (_tx, rx) = mpsc::channel();
        let waiter = ActionCompletionWaiter::new(rx);
        assert!(matches!(
            waiter.wait(handle(1, 0x1002), Some(Duration::from_millis(1))),
            Err(ActionCompletionError::Timeout)
        ));
    }

    #[test]
    fn checked_wait_propagates_cancellation() {
        let (_tx, rx) = mpsc::channel();
        let waiter = ActionCompletionWaiter::new(rx);
        let result = waiter.wait_checked(
            handle(1, 0x1002),
            Instant::now() + Duration::from_secs(1),
            || Err("cancelled".to_owned()),
        );
        assert!(matches!(result, Err(ActionCompletionError::Interrupted(message)) if message == "cancelled"));
    }
}
