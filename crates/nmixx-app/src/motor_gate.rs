//! Orders in-flight motor starts against Stop/Disconnect. Scope controls do not
//! use this gate: stopping acquisition must not cancel a motor operation.
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};

#[derive(Default)]
pub(super) struct MotorGate {
    commands: Mutex<()>,
    revision: AtomicU64,
    closed: AtomicBool,
}

impl MotorGate {
    pub(super) fn ticket(&self) -> u64 { self.revision.load(Ordering::SeqCst) }
    pub(super) fn cancel(&self) { self.revision.fetch_add(1, Ordering::SeqCst); }
    pub(super) fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
        self.cancel();
    }
    pub(super) fn is_closed(&self) -> bool { self.closed.load(Ordering::SeqCst) }

    pub(super) fn start(&self, ticket: u64) -> Result<MutexGuard<'_, ()>, &'static str> {
        let guard = self.commands.lock().map_err(|_| "motor command state is poisoned")?;
        if self.is_closed() { return Err("device connection is closing"); }
        if self.ticket() != ticket { return Err("motor start was cancelled"); }
        Ok(guard)
    }

    // Safety actions remain possible even after a previous command panicked.
    pub(super) fn stop(&self) -> MutexGuard<'_, ()> {
        self.commands.lock().unwrap_or_else(|error| error.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, mpsc};
    use std::thread;

    #[test]
    fn stop_invalidates_prepared_start_but_not_a_later_explicit_run() {
        let gate = MotorGate::default();
        let ticket = gate.ticket();
        gate.cancel();
        assert!(gate.start(ticket).is_err());
        assert!(gate.start(gate.ticket()).is_ok());
    }
    #[test]
    fn disconnect_rejects_old_and_new_start_tickets() {
        let gate = MotorGate::default();
        let old = gate.ticket();
        gate.close();
        assert!(gate.start(old).is_err());
        assert!(gate.start(gate.ticket()).is_err());
        let _stop = gate.stop();
    }
    #[test]
    fn a_queued_start_cannot_run_after_the_stop_barrier() {
        let gate = Arc::new(MotorGate::default());
        let ticket = gate.ticket();
        let in_flight = gate.start(ticket).unwrap();
        let (cancelled_tx, cancelled_rx) = mpsc::channel();
        let other = gate.clone();
        let stopper = thread::spawn(move || {
            other.cancel();
            cancelled_tx.send(()).unwrap();
            let _stop = other.stop();
        });
        cancelled_rx.recv().unwrap();
        drop(in_flight);
        stopper.join().unwrap();
        assert!(gate.start(ticket).is_err());
    }
}
