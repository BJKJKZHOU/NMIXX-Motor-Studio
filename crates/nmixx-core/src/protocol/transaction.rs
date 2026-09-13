use std::collections::HashMap;

use thiserror::Error;

use super::ResponseFrame;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TransactionId(u8);

impl TransactionId {
    pub fn get(self) -> u8 { self.0 }

    pub(crate) fn from_raw(value: u8) -> Self {
        debug_assert!(value != 0);
        Self(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingRequest {
    pub message: u8,
    pub op: u8,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum TransactionError {
    #[error("all AXDR transaction IDs are currently in use")]
    Exhausted,
    #[error("response for unknown transaction {0}")]
    Unknown(u8),
    #[error("response transaction {txn} does not match request message/op: expected {expected_message:#04x}/{expected_op:#04x}, got {actual_message:#04x}/{actual_op:#04x}")]
    ResponseMismatch {
        txn: u8,
        expected_message: u8,
        expected_op: u8,
        actual_message: u8,
        actual_op: u8,
    },
}

#[derive(Debug)]
pub struct TransactionTable {
    next: u8,
    pending: HashMap<u8, PendingRequest>,
}

impl Default for TransactionTable {
    fn default() -> Self {
        Self { next: 1, pending: HashMap::new() }
    }
}

impl TransactionTable {
    pub fn allocate(&mut self, message: u8, op: u8) -> Result<TransactionId, TransactionError> {
        for _ in 0..255 {
            let txn = self.next;
            self.next = if txn == 255 { 1 } else { txn + 1 };
            if !self.pending.contains_key(&txn) {
                self.pending.insert(txn, PendingRequest { message, op });
                return Ok(TransactionId(txn));
            }
        }
        Err(TransactionError::Exhausted)
    }

    pub fn cancel(&mut self, txn: TransactionId) {
        self.pending.remove(&txn.0);
    }

    /// Matches and consumes a pending transaction.
    ///
    /// Transaction ownership ends once txn/message/op match. AXDR response
    /// status is domain semantics and must be interpreted by the Parameter,
    /// Action or Plot parser rather than by this generic transaction layer.
    pub fn complete(&mut self, response: &ResponseFrame) -> Result<TransactionId, TransactionError> {
        let Some(expected) = self.pending.get(&response.txn).copied() else {
            return Err(TransactionError::Unknown(response.txn));
        };
        if expected.message != response.request_message || expected.op != response.request_op {
            return Err(TransactionError::ResponseMismatch {
                txn: response.txn,
                expected_message: expected.message,
                expected_op: expected.op,
                actual_message: response.request_message,
                actual_op: response.request_op,
            });
        }
        self.pending.remove(&response.txn);
        Ok(TransactionId(response.txn))
    }

    pub fn len(&self) -> usize { self.pending.len() }
    pub fn is_empty(&self) -> bool { self.pending.is_empty() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::AxdrStatus;

    fn response(txn: u8, message: u8, op: u8, status: AxdrStatus) -> ResponseFrame {
        ResponseFrame { txn, request_message: message, request_op: op, status, data: Vec::new() }
    }

    #[test]
    fn allocates_and_matches_response() {
        let mut table = TransactionTable::default();
        let txn = table.allocate(0x07, 0x01).unwrap();
        assert_eq!(txn.get(), 1);
        table.complete(&response(1, 0x07, 0x01, AxdrStatus::Ok)).unwrap();
        assert!(table.is_empty());
    }

    #[test]
    fn matching_error_status_still_completes_transaction() {
        let mut table = TransactionTable::default();
        let txn = table.allocate(0x07, 0x02).unwrap();
        assert_eq!(
            table.complete(&response(1, 0x07, 0x02, AxdrStatus::ErrState)),
            Ok(txn)
        );
        assert!(table.is_empty());
    }

    #[test]
    fn mismatch_does_not_consume_pending_request() {
        let mut table = TransactionTable::default();
        table.allocate(0x07, 0x01).unwrap();
        assert!(matches!(table.complete(&response(1, 0x07, 0x02, AxdrStatus::Ok)), Err(TransactionError::ResponseMismatch { .. })));
        assert_eq!(table.len(), 1);
    }
}
