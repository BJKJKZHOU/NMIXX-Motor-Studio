//! Per-session workflow ownership. Permits protect the claim/dispatch boundary;
//! they do not hold a mutex while a command waits for device I/O.
use std::sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}};

#[derive(Default)]
struct State {
    next: u64,
    owner: Option<(u64, Arc<AtomicBool>)>,
    calls: usize,
    closed: bool,
}
#[derive(Default)]
pub(crate) struct WorkflowAccess(Mutex<State>);
pub(crate) struct Permit<'a>(&'a WorkflowAccess);
impl Drop for Permit<'_> {
    fn drop(&mut self) {
        let mut state = self.0.0.lock().unwrap_or_else(|e| e.into_inner());
        state.calls -= 1;
    }
}
impl WorkflowAccess {
    pub(crate) fn claim(&self) -> Result<(u64, Arc<AtomicBool>), String> {
        let mut state = self.0.lock().map_err(|_| "workflow access lock poisoned")?;
        if state.closed { return Err("The original connection is closing".into()); }
        if state.owner.is_some() { return Err("An Automation task already owns this session".into()); }
        if state.calls != 0 { return Err("An application operation is in progress; wait for it to finish".into()); }
        state.next = state.next.checked_add(1).ok_or("workflow generation exhausted")?;
        let id = state.next;
        let cancel = Arc::new(AtomicBool::new(false));
        state.owner = Some((id, cancel.clone()));
        Ok((id, cancel))
    }
    pub(crate) fn enter(&self, caller: Option<u64>, cleanup: bool) -> Result<Permit<'_>, String> {
        let mut state = self.0.lock().map_err(|_| "workflow access lock poisoned")?;
        match (&state.owner, caller) {
            (Some((owner, cancel)), Some(id)) if *owner == id => {
                if !cleanup && (state.closed || cancel.load(Ordering::Acquire)) {
                    return Err("Automation was cancelled; no further operation is allowed".into());
                }
            }
            (None, None) if !state.closed => {}
            (Some(_), None) => return Err("Automation owns this operation; cancel it or use global motor Stop/Disable".into()),
            _ => return Err("The operation belongs to an expired or closed session".into()),
        }
        state.calls += 1;
        Ok(Permit(self))
    }
    pub(crate) fn revoke(&self) {
        if let Some((_, cancel)) = &self.0.lock().unwrap_or_else(|e| e.into_inner()).owner {
            cancel.store(true, Ordering::Release);
        }
    }
    pub(crate) fn close(&self) {
        let mut state = self.0.lock().unwrap_or_else(|e| e.into_inner());
        state.closed = true;
        if let Some((_, cancel)) = &state.owner { cancel.store(true, Ordering::Release); }
    }
    pub(crate) fn release(&self, id: u64) {
        let mut state = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if state.owner.as_ref().is_some_and(|(owner, _)| *owner == id) { state.owner = None; }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn claim_waits_for_existing_manual_operation() {
        let access = WorkflowAccess::default();
        let permit = access.enter(None, false).unwrap();
        assert!(access.claim().is_err());
        drop(permit);
        assert!(access.claim().is_ok());
    }
    #[test] fn owner_reuses_normal_methods_but_other_mutations_are_rejected() {
        let access = WorkflowAccess::default();
        let (owner, _) = access.claim().unwrap();
        assert!(access.enter(None, false).is_err());
        let first = access.enter(Some(owner), false).unwrap();
        let nested = access.enter(Some(owner), false).unwrap();
        drop((first, nested));
        access.release(owner);
        assert!(access.enter(None, false).is_ok());
    }
    #[test] fn cancel_blocks_later_commands_without_blocking_owned_cleanup() {
        let access = WorkflowAccess::default();
        let (owner, cancelled) = access.claim().unwrap();
        access.revoke();
        assert!(cancelled.load(Ordering::Acquire));
        assert!(access.enter(Some(owner), false).is_err());
        assert!(access.enter(Some(owner), true).is_ok());
        assert!(access.enter(None, true).is_err());
    }
    #[test] fn expired_owner_cannot_act_on_later_task() {
        let access = WorkflowAccess::default();
        let (old, _) = access.claim().unwrap(); access.release(old);
        let (new, _) = access.claim().unwrap();
        access.release(old);
        assert!(access.enter(Some(old), true).is_err());
        assert!(access.enter(Some(new), false).is_ok());
    }
    #[test] fn closing_revokes_and_never_accepts_a_new_task() {
        let access = WorkflowAccess::default();
        let (owner, cancelled) = access.claim().unwrap(); access.close();
        assert!(cancelled.load(Ordering::Acquire));
        access.release(owner);
        assert!(access.claim().is_err());
        assert!(access.enter(None, false).is_err());
    }
}
