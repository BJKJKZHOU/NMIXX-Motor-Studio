//! Device-session runtime.
//!
//! A session owns exactly one transport on exactly one worker thread. Clients
//! submit semantic requests through `DeviceSession`; they never read or write
//! the transport directly.

use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use nmixx_core::protocol::{
    ActionError, ActionHandle, ActionTracker, AxdrStatus, InboundFrame, MSG_PARAMETER, MSG_PLOT,
    NODE_ID_DEFAULT, PARAM_READ, PARAM_WRITE, PLOT_CONFIG, PLOT_START, PLOT_STOP, ParameterType,
    ParameterValue, ResponseFrame, TransactionError, TransactionId, TransactionTable,
    build_action_start_default, build_parameter_read_default, build_parameter_write,
    build_plot_config_default, build_plot_start_default, build_plot_stop_default, decode_inbound,
    parse_action_accept, parse_parameter_read, parse_parameter_write, parse_plot_config_response,
    parse_plot_start_response, parse_plot_stop_response,
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
    #[error(transparent)]
    Action(#[from] ActionError),
    #[error("plot operation failed: {0}")]
    Plot(String),
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
    PlotConfig {
        group: u8,
        config_id: u8,
        parameters: Vec<u16>,
        reply: mpsc::Sender<Result<(), SessionError>>,
    },
    PlotStart {
        group_mask: u8,
        reply: mpsc::Sender<Result<(), SessionError>>,
    },
    PlotStop {
        group_mask: u8,
        reply: mpsc::Sender<Result<(), SessionError>>,
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

    pub fn parameter_read(&self, id: u16, ty: ParameterType) -> Result<ParameterValue, SessionError> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.inner.commands.send(Command::ParameterRead { id, ty, reply: reply_tx }).map_err(|_| SessionError::Closed)?;
        reply_rx.recv().map_err(|_| SessionError::Closed)?
    }

    pub fn parameter_write(&self, id: u16, value: ParameterValue) -> Result<(), SessionError> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.inner.commands.send(Command::ParameterWrite { id, value, reply: reply_tx }).map_err(|_| SessionError::Closed)?;
        reply_rx.recv().map_err(|_| SessionError::Closed)?
    }

    pub fn action_start(&self, action_id: u16) -> Result<ActionHandle, SessionError> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.inner.commands.send(Command::ActionStart { action_id, reply: reply_tx }).map_err(|_| SessionError::Closed)?;
        reply_rx.recv().map_err(|_| SessionError::Closed)?
    }

    pub fn plot_config(&self, group: u8, config_id: u8, parameters: &[u16]) -> Result<(), SessionError> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.inner.commands.send(Command::PlotConfig { group, config_id, parameters: parameters.to_vec(), reply: reply_tx }).map_err(|_| SessionError::Closed)?;
        reply_rx.recv().map_err(|_| SessionError::Closed)?
    }

    pub fn plot_start(&self, group_mask: u8) -> Result<(), SessionError> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.inner.commands.send(Command::PlotStart { group_mask, reply: reply_tx }).map_err(|_| SessionError::Closed)?;
        reply_rx.recv().map_err(|_| SessionError::Closed)?
    }

    pub fn plot_stop(&self, group_mask: u8) -> Result<(), SessionError> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.inner.commands.send(Command::PlotStop { group_mask, reply: reply_tx }).map_err(|_| SessionError::Closed)?;
        reply_rx.recv().map_err(|_| SessionError::Closed)?
    }

    pub fn subscribe(&self) -> Result<mpsc::Receiver<SessionEvent>, SessionError> {
        let (tx, rx) = mpsc::channel();
        self.inner.commands.send(Command::Subscribe { subscriber: tx }).map_err(|_| SessionError::Closed)?;
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
        Self { transport, transactions: TransactionTable::default(), actions: ActionTracker::default(), subscribers: Vec::new() }
    }

    fn run(mut self, commands: mpsc::Receiver<Command>) {
        loop {
            match commands.try_recv() {
                Ok(Command::Shutdown) | Err(mpsc::TryRecvError::Disconnected) => break,
                Ok(command) => { self.handle_command(command); continue; }
                Err(mpsc::TryRecvError::Empty) => {}
            }
            let _ = self.poll_one(RX_POLL);
        }
    }

    fn handle_command(&mut self, command: Command) {
        match command {
            Command::ParameterRead { id, ty, reply } => { let _ = reply.send(self.parameter_read(id, ty)); }
            Command::ParameterWrite { id, value, reply } => { let _ = reply.send(self.parameter_write(id, value)); }
            Command::ActionStart { action_id, reply } => { let _ = reply.send(self.action_start(action_id)); }
            Command::PlotConfig { group, config_id, parameters, reply } => { let _ = reply.send(self.plot_config(group, config_id, &parameters)); }
            Command::PlotStart { group_mask, reply } => { let _ = reply.send(self.plot_start(group_mask)); }
            Command::PlotStop { group_mask, reply } => { let _ = reply.send(self.plot_stop(group_mask)); }
            Command::Subscribe { subscriber } => self.subscribers.push(subscriber),
            Command::Shutdown => {}
        }
    }

    fn parameter_read(&mut self, id: u16, ty: ParameterType) -> Result<ParameterValue, SessionError> {
        let txn = self.transactions.allocate(MSG_PARAMETER, PARAM_READ)?;
        let frame = build_parameter_read_default(txn, id).map_err(|error| SessionError::Parameter(error.to_string()))?;
        if let Err(error) = self.transport.send(&frame) { self.transactions.cancel(txn); return Err(error.into()); }
        let response = self.wait_response(txn, REQUEST_TIMEOUT)?;
        parse_parameter_read(&response, id, ty).map_err(|error| SessionError::Parameter(error.to_string()))
    }

    fn parameter_write(&mut self, id: u16, value: ParameterValue) -> Result<(), SessionError> {
        let txn = self.transactions.allocate(MSG_PARAMETER, PARAM_WRITE)?;
        let frame = build_parameter_write(txn, id, value, NODE_ID_DEFAULT).map_err(|error| SessionError::Parameter(error.to_string()))?;
        if let Err(error) = self.transport.send(&frame) { self.transactions.cancel(txn); return Err(error.into()); }
        let response = self.wait_response(txn, REQUEST_TIMEOUT)?;
        parse_parameter_write(&response, id).map_err(|error| SessionError::Parameter(error.to_string()))
    }

    fn action_start(&mut self, action_id: u16) -> Result<ActionHandle, SessionError> {
        let txn = self.transactions.allocate(MSG_PARAMETER, PARAM_WRITE)?;
        let handle = ActionHandle { txn, action_id };
        self.actions.track(handle);

        let frame = build_action_start_default(txn, action_id)?;
        if let Err(error) = self.transport.send(&frame) {
            self.transactions.cancel(txn);
            self.actions.remove(handle);
            return Err(error.into());
        }

        let response = match self.wait_response(txn, REQUEST_TIMEOUT) {
            Ok(response) => response,
            Err(error) => { self.actions.remove(handle); return Err(error); }
        };
        if let Err(error) = parse_action_accept(&response, action_id) {
            self.actions.remove(handle);
            return Err(error.into());
        }
        self.actions.accepted(handle);
        Ok(handle)
    }

    fn plot_config(&mut self, group: u8, config_id: u8, parameters: &[u16]) -> Result<(), SessionError> {
        let txn = self.transactions.allocate(MSG_PLOT, PLOT_CONFIG)?;
        let frame = build_plot_config_default(txn, group, config_id, parameters).map_err(|error| SessionError::Plot(error.to_string()))?;
        if let Err(error) = self.transport.send(&frame) { self.transactions.cancel(txn); return Err(error.into()); }
        let response = self.wait_response(txn, REQUEST_TIMEOUT)?;
        parse_plot_config_response(&response, group, config_id, parameters).map_err(|error| SessionError::Plot(error.to_string()))
    }

    fn plot_start(&mut self, group_mask: u8) -> Result<(), SessionError> {
        let txn = self.transactions.allocate(MSG_PLOT, PLOT_START)?;
        let frame = build_plot_start_default(txn, group_mask).map_err(|error| SessionError::Plot(error.to_string()))?;
        if let Err(error) = self.transport.send(&frame) { self.transactions.cancel(txn); return Err(error.into()); }
        let response = self.wait_response(txn, REQUEST_TIMEOUT)?;
        parse_plot_start_response(&response).map_err(|error| SessionError::Plot(error.to_string()))
    }

    fn plot_stop(&mut self, group_mask: u8) -> Result<(), SessionError> {
        let txn = self.transactions.allocate(MSG_PLOT, PLOT_STOP)?;
        let frame = build_plot_stop_default(txn, group_mask).map_err(|error| SessionError::Plot(error.to_string()))?;
        if let Err(error) = self.transport.send(&frame) { self.transactions.cancel(txn); return Err(error.into()); }
        let response = self.wait_response(txn, REQUEST_TIMEOUT)?;
        parse_plot_stop_response(&response).map_err(|error| SessionError::Plot(error.to_string()))
    }

    fn wait_response(&mut self, txn: TransactionId, timeout: Duration) -> Result<ResponseFrame, SessionError> {
        let deadline = Instant::now() + timeout;
        loop {
            let now = Instant::now();
            if now >= deadline { self.transactions.cancel(txn); return Err(SessionError::Timeout); }
            match self.transport.receive(deadline.saturating_duration_since(now)) {
                Ok(frame) => {
                    let inbound = match decode_inbound(frame) {
                        Ok(inbound) => inbound,
                        Err(error) => { self.transactions.cancel(txn); return Err(SessionError::Decode(error.to_string())); }
                    };
                    match inbound {
                        InboundFrame::Response(response) if response.txn == txn.get() => {
                            if let Err(error) = self.transactions.complete(&response) { self.transactions.cancel(txn); return Err(error.into()); }
                            return Ok(response);
                        }
                        other => self.dispatch(other),
                    }
                }
                Err(TransportError::Timeout) => { self.transactions.cancel(txn); return Err(SessionError::Timeout); }
                Err(error) => { self.transactions.cancel(txn); return Err(error.into()); }
            }
        }
    }

    fn poll_one(&mut self, timeout: Duration) -> Result<(), SessionError> {
        match self.transport.receive(timeout) {
            Ok(frame) => {
                let inbound = decode_inbound(frame).map_err(|error| SessionError::Decode(error.to_string()))?;
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
                let Ok(handle) = self.actions.complete(&completion) else { return; };
                Some(SessionEvent::ActionCompleted { handle, status: completion.status })
            }
            InboundFrame::Event(frame) => Some(SessionEvent::DeviceEvent(frame)),
            InboundFrame::FastData(frame) => Some(SessionEvent::FastData(frame)),
            InboundFrame::NormalData(frame) => Some(SessionEvent::NormalData(frame)),
            InboundFrame::Response(_) | InboundFrame::Unknown(_) => None,
        };
        if let Some(event) = event { self.subscribers.retain(|subscriber| subscriber.send(event.clone()).is_ok()); }
    }
}
