//! Device-session runtime.
//!
//! A session owns exactly one transport on exactly one worker thread. Clients
//! submit semantic requests through `DeviceSession`; they never read or write
//! the transport directly.

use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use nmixx_core::protocol::{
    ActionHandle, ActionTracker, AxdrStatus, InboundFrame, MSG_PARAMETER, NODE_ID_DEFAULT,
    PARAM_READ, PARAM_WRITE, ParameterType, ParameterValue, ResponseFrame, TransactionError,
    TransactionId, TransactionTable, build_action_start_default, build_parameter_read_default,
    build_parameter_write, decode_inbound, parse_action_accept, parse_parameter_read,
    parse_parameter_write,
};
use nmixx_core::transport::{FrameTransport, TransportError};
use nmixx_core::wire::CanFdFrame;
use thiserror::Error;

const RX_POLL: Duration = Duration::from_millis(20);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone)]
pub enum SessionEvent {
    ActionCompleted {
        handle: ActionHandle,
        status: AxdrStatus,
    },
    DeviceEvent(CanFdFrame),
    FastData(CanFdFrame),
    NormalData(CanFdFrame),
}

#[derive(Debug, Error)]
pub enum SessionError {
    #[error("device session is closed")]
    Closed,
    #[error("device request timed out")]
    Timeout,
    #[error(transparent)]
    Transport(#[from] TransportError),
    #[error(transparent)]
    Transaction(#[from] TransactionError),
    #[error("protocol decode failed: {0}")]
    Decode(String),
    #[error("parameter operation failed: {0}")]
    Parameter(String),
    #[error("action operation failed: {0}")]
    Action(String),
}

enum Command {
    ParameterRead {
        id: u16,
        ty: ParameterType,
        reply: mpsc::Sender<Result<ParameterValue, SessionError>>,
    },
    ParameterWrite {
        id: u16,
        value: ParameterValue,
        reply: mpsc::Sender<Result<(), SessionError>>,
    },
    ActionStart {
        action_id: u16,
        reply: mpsc::Sender<Result<ActionHandle, SessionError>>,
    },
    Subscribe {
        subscriber: mpsc::Sender<SessionEvent>,
    },
    Shutdown,
}

struct SessionInner {
    commands: mpsc::Sender<Command>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl Drop for SessionInner {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Shutdown);
        if let Some(worker) = self.worker.lock().ok().and_then(|mut slot| slot.take()) {
            let _ = worker.join();
        }
    }
}

/// Cloneable client handle for one physical/logical device session.
///
/// Cloning this value does not clone or reopen the transport. Every clone sends
/// commands to the same single-owner worker.
#[derive(Clone)]
pub struct DeviceSession {
    inner: Arc<SessionInner>,
}

impl DeviceSession {
    pub fn spawn(transport: Box<dyn FrameTransport>) -> Self {
        let (command_tx, command_rx) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("nmixx-device-session".into())
            .spawn(move || Worker::new(transport).run(command_rx))
            .expect("failed to spawn NMIXX device-session worker");

        Self {
            inner: Arc::new(SessionInner {
                commands: command_tx,
                worker: Mutex::new(Some(worker)),
            }),
        }
    }

    pub fn parameter_read(
        &self,
        id: u16,
        ty: ParameterType,
    ) -> Result<ParameterValue, SessionError> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.inner
            .commands
            .send(Command::ParameterRead { id, ty, reply: reply_tx })
            .map_err(|_| SessionError::Closed)?;
        reply_rx.recv().map_err(|_| SessionError::Closed)?
    }

    pub fn parameter_write(&self, id: u16, value: ParameterValue) -> Result<(), SessionError> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.inner
            .commands
            .send(Command::ParameterWrite { id, value, reply: reply_tx })
            .map_err(|_| SessionError::Closed)?;
        reply_rx.recv().map_err(|_| SessionError::Closed)?
    }

    /// Starts an Action and returns after the firmware has accepted it.
    ///
    /// Finite completion is delivered separately as `SessionEvent::ActionCompleted`.
    pub fn action_start(&self, action_id: u16) -> Result<ActionHandle, SessionError> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.inner
            .commands
            .send(Command::ActionStart { action_id, reply: reply_tx })
            .map_err(|_| SessionError::Closed)?;
        reply_rx.recv().map_err(|_| SessionError::Closed)?
    }

    pub fn subscribe(&self) -> Result<mpsc::Receiver<SessionEvent>, SessionError> {
        let (tx, rx) = mpsc::channel();
        self.inner
            .commands
            .send(Command::Subscribe { subscriber: tx })
            .map_err(|_| SessionError::Closed)?;
        Ok(rx)
    }
}

struct Worker {
    transport: Box<dyn FrameTransport>,
    transactions: TransactionTable,
    actions: ActionTracker,
    subscribers: Vec<mpsc::Sender<SessionEvent>>,
}

impl Worker {
    fn new(transport: Box<dyn FrameTransport>) -> Self {
        Self {
            transport,
            transactions: TransactionTable::default(),
            actions: ActionTracker::default(),
            subscribers: Vec::new(),
        }
    }

    fn run(mut self, commands: mpsc::Receiver<Command>) {
        loop {
            match commands.recv_timeout(RX_POLL) {
                Ok(Command::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Ok(command) => self.handle_command(command),
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    let _ = self.poll_one(RX_POLL);
                }
            }
        }
    }

    fn handle_command(&mut self, command: Command) {
        match command {
            Command::ParameterRead { id, ty, reply } => {
                let _ = reply.send(self.parameter_read(id, ty));
            }
            Command::ParameterWrite { id, value, reply } => {
                let _ = reply.send(self.parameter_write(id, value));
            }
            Command::ActionStart { action_id, reply } => {
                let _ = reply.send(self.action_start(action_id));
            }
            Command::Subscribe { subscriber } => self.subscribers.push(subscriber),
            Command::Shutdown => {}
        }
    }

    fn parameter_read(&mut self, id: u16, ty: ParameterType) -> Result<ParameterValue, SessionError> {
        let txn = self.transactions.allocate(MSG_PARAMETER, PARAM_READ)?;
        let frame = build_parameter_read_default(txn, id)
            .map_err(|error| SessionError::Parameter(error.to_string()))?;
        if let Err(error) = self.transport.send(&frame) {
            self.transactions.cancel(txn);
            return Err(error.into());
        }
        let response = self.wait_response(txn, REQUEST_TIMEOUT)?;
        parse_parameter_read(&response, id, ty)
            .map_err(|error| SessionError::Parameter(error.to_string()))
    }

    fn parameter_write(&mut self, id: u16, value: ParameterValue) -> Result<(), SessionError> {
        let txn = self.transactions.allocate(MSG_PARAMETER, PARAM_WRITE)?;
        let frame = build_parameter_write(txn, id, value, NODE_ID_DEFAULT)
            .map_err(|error| SessionError::Parameter(error.to_string()))?;
        if let Err(error) = self.transport.send(&frame) {
            self.transactions.cancel(txn);
            return Err(error.into());
        }
        let response = self.wait_response(txn, REQUEST_TIMEOUT)?;
        parse_parameter_write(&response, id)
            .map_err(|error| SessionError::Parameter(error.to_string()))
    }

    fn action_start(&mut self, action_id: u16) -> Result<ActionHandle, SessionError> {
        let txn = self.transactions.allocate(MSG_PARAMETER, PARAM_WRITE)?;
        let handle = ActionHandle { txn, action_id };
        self.actions.track(handle);

        let frame = build_action_start_default(txn, action_id)
            .map_err(|error| SessionError::Action(error.to_string()))?;
        if let Err(error) = self.transport.send(&frame) {
            self.transactions.cancel(txn);
            self.actions.remove(handle);
            return Err(error.into());
        }

        let response = match self.wait_response(txn, REQUEST_TIMEOUT) {
            Ok(response) => response,
            Err(error) => {
                self.actions.remove(handle);
                return Err(error);
            }
        };
        parse_action_accept(&response, action_id)
            .map_err(|error| SessionError::Action(error.to_string()))?;
        self.actions.accepted(handle);
        Ok(handle)
    }

    fn wait_response(
        &mut self,
        txn: TransactionId,
        timeout: Duration,
    ) -> Result<ResponseFrame, SessionError> {
        let deadline = Instant::now() + timeout;
        loop {
            let now = Instant::now();
            if now >= deadline {
                self.transactions.cancel(txn);
                return Err(SessionError::Timeout);
            }

            match self.transport.receive(deadline.saturating_duration_since(now)) {
                Ok(frame) => {
                    let inbound = decode_inbound(frame)
                        .map_err(|error| SessionError::Decode(error.to_string()))?;
                    match inbound {
                        InboundFrame::Response(response) if response.txn == txn.get() => {
                            self.transactions.complete(&response)?;
                            return Ok(response);
                        }
                        other => self.dispatch(other),
                    }
                }
                Err(TransportError::Timeout) => {
                    self.transactions.cancel(txn);
                    return Err(SessionError::Timeout);
                }
                Err(error) => {
                    self.transactions.cancel(txn);
                    return Err(error.into());
                }
            }
        }
    }

    fn poll_one(&mut self, timeout: Duration) -> Result<(), SessionError> {
        match self.transport.receive(timeout) {
            Ok(frame) => {
                let inbound = decode_inbound(frame)
                    .map_err(|error| SessionError::Decode(error.to_string()))?;
                self.dispatch(inbound);
                Ok(())
            }
            Err(TransportError::Timeout) => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    fn dispatch(&mut self, inbound: InboundFrame) {
        let event = match inbound {
            InboundFrame::ActionComplete(completion) => {
                let Ok(handle) = self.actions.complete(&completion) else {
                    return;
                };
                Some(SessionEvent::ActionCompleted {
                    handle,
                    status: completion.status,
                })
            }
            InboundFrame::Event(frame) => Some(SessionEvent::DeviceEvent(frame)),
            InboundFrame::FastData(frame) => Some(SessionEvent::FastData(frame)),
            InboundFrame::NormalData(frame) => Some(SessionEvent::NormalData(frame)),
            InboundFrame::Response(_) | InboundFrame::Unknown(_) => None,
        };

        if let Some(event) = event {
            self.subscribers.retain(|subscriber| subscriber.send(event.clone()).is_ok());
        }
    }
}
