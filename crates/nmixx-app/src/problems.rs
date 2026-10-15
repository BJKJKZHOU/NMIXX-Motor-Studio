use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::parameter_service::ParameterService;
use crate::{HostSchema, ParameterValue, ProtectionEventFrame};

const EVENT_REPORT: &str = "PARAM_EVENT_REPORT";
const EVENT_WARNING: &str = "PARAM_EVENT_WARNING";
const EVENT_ERROR: &str = "PARAM_EVENT_ERROR";
const EVENT_TRIP: &str = "PARAM_EVENT_TRIP";
const ENCODER_PROTOCOL: &str = "PARAM_ENCODER_PROTOCOL";
const ENCODER_READY: &str = "PARAM_ENCODER_READY";
const ENCODER_VALID: &str = "PARAM_ENCODER_VALID";
const ENCODER_FAULT: &str = "PARAM_ENCODER_FAULT";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProblemSeverity {
    Info,
    Warning,
    Error,
    Fault,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProblemDomain {
    Protection,
    Encoder,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProblemRecord {
    pub id: String,
    pub severity: ProblemSeverity,
    pub domain: ProblemDomain,
    pub summary: String,
    pub description: String,
    pub code: u32,
    pub active: bool,
    pub first_seen_ms: u64,
    pub last_seen_ms: u64,
    pub occurrence_count: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProblemSnapshot {
    pub active: Vec<ProblemRecord>,
    pub history: Vec<ProblemRecord>,
}

#[derive(Default)]
struct ProblemState {
    records: BTreeMap<String, ProblemRecord>,
}

#[derive(Clone, Default)]
pub(crate) struct ProblemService {
    state: Arc<Mutex<ProblemState>>,
}

impl ProblemService {
    pub(crate) fn update_protection(&self, status: ProtectionEventFrame) {
        let Ok(mut state) = self.state.lock() else { return; };
        let now = now_ms();
        update_record(&mut state, "protection.report", ProblemDomain::Protection, ProblemSeverity::Info, "Protection report active",
            "The device reports one or more active informational protection bits.", status.report, now);
        update_record(&mut state, "protection.warning", ProblemDomain::Protection, ProblemSeverity::Warning, "Protection warning active",
            "The device reports one or more active protection warnings.", status.warning, now);
        update_record(&mut state, "protection.error", ProblemDomain::Protection, ProblemSeverity::Error, "Protection error active",
            "The device reports a software protection error. The drive may be disabled until the fault is cleared.", status.error, now);
        update_record(&mut state, "protection.trip", ProblemDomain::Protection, ProblemSeverity::Fault, "Hardware trip active",
            "The device reports a hardware-fast protection trip.", status.trip, now);
    }

    pub(crate) fn sync_from_parameters(&self, parameters: &ParameterService, schema: &HostSchema) {
        if let (Some(report), Some(warning), Some(error), Some(trip)) = (
            cached_u32(parameters, schema, EVENT_REPORT), cached_u32(parameters, schema, EVENT_WARNING),
            cached_u32(parameters, schema, EVENT_ERROR), cached_u32(parameters, schema, EVENT_TRIP),
        ) {
            self.update_protection(ProtectionEventFrame { report, warning, error, trip });
        }

        let Some(protocol_meta) = schema.parameter_by_symbol(ENCODER_PROTOCOL) else { return; };
        let Some(protocol) = cached_u8(parameters, schema, ENCODER_PROTOCOL) else { return; };
        let Ok(none) = protocol_meta.enum_u8("ENC_PROTOCOL_NONE") else { return; };
        if protocol == none {
            self.update_encoder_health(false, 0, 0, 0);
        } else if let (Some(ready), Some(valid), Some(fault)) = (
            cached_u8(parameters, schema, ENCODER_READY), cached_u8(parameters, schema, ENCODER_VALID),
            cached_u8(parameters, schema, ENCODER_FAULT),
        ) {
            self.update_encoder_health(true, ready, valid, fault);
        }
    }

    fn update_encoder_health(&self, configured: bool, ready: u8, valid: u8, fault: u8) {
        let Ok(mut state) = self.state.lock() else { return; };
        let now = now_ms();
        update_record(&mut state, "encoder.fault", ProblemDomain::Encoder, ProblemSeverity::Error,
            "Encoder fault", "The configured encoder reports a fault. Inspect the encoder, wiring and feedback protocol.",
            u32::from(configured && fault != 0), now);
        update_record(&mut state, "encoder.ready", ProblemDomain::Encoder, ProblemSeverity::Warning,
            "Encoder not ready", "The configured encoder is not ready for servo operation.",
            u32::from(configured && fault == 0 && ready == 0), now);
        update_record(&mut state, "encoder.valid", ProblemDomain::Encoder, ProblemSeverity::Warning,
            "Encoder feedback invalid", "The encoder is initialized but its current feedback is invalid.",
            u32::from(configured && fault == 0 && ready != 0 && valid == 0), now);
    }

    pub(crate) fn snapshot(&self) -> ProblemSnapshot {
        let Ok(state) = self.state.lock() else { return ProblemSnapshot::default(); };
        let mut active = state.records.values().filter(|record| record.active).cloned().collect::<Vec<_>>();
        active.sort_by(|left, right| right.severity.cmp(&left.severity).then_with(|| left.id.cmp(&right.id)));
        let mut history = state.records.values().cloned().collect::<Vec<_>>();
        history.sort_by(|left, right| right.last_seen_ms.cmp(&left.last_seen_ms).then_with(|| left.id.cmp(&right.id)));
        ProblemSnapshot { active, history }
    }

    pub(crate) fn clear_history(&self) {
        let Ok(mut state) = self.state.lock() else { return; };
        state.records.retain(|_, record| record.active);
    }
}

fn cached_u8(parameters: &ParameterService, schema: &HostSchema, symbol: &str) -> Option<u8> {
    let metadata = schema.parameter_by_symbol(symbol)?;
    match parameters.cached(metadata.id).ok().flatten()? {
        ParameterValue::U8(value) => Some(value),
        _ => None,
    }
}

fn cached_u32(parameters: &ParameterService, schema: &HostSchema, symbol: &str) -> Option<u32> {
    let metadata = schema.parameter_by_symbol(symbol)?;
    match parameters.cached(metadata.id).ok().flatten()? {
        ParameterValue::U32(value) => Some(value),
        _ => None,
    }
}

fn update_record(
    state: &mut ProblemState,
    id: &str,
    domain: ProblemDomain,
    severity: ProblemSeverity,
    summary: &str,
    description: &str,
    code: u32,
    now: u64,
) {
    match state.records.get_mut(id) {
        Some(record) => {
            if code == 0 {
                if record.active {
                    record.active = false;
                    record.last_seen_ms = now;
                }
                return;
            }
            if !record.active || record.code != code {
                record.occurrence_count = record.occurrence_count.saturating_add(1);
                record.last_seen_ms = now;
            }
            record.active = true;
            record.code = code;
            record.description = format!("{description} Active mask: 0x{code:08X}.");
        }
        None if code != 0 => {
            state.records.insert(id.to_owned(), ProblemRecord {
                id: id.to_owned(),
                severity,
                domain,
                summary: summary.to_owned(),
                description: format!("{description} Active mask: 0x{code:08X}."),
                code,
                active: true,
                first_seen_ms: now,
                last_seen_ms: now,
                occurrence_count: 1,
            });
        }
        None => {}
    }
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis().min(u128::from(u64::MAX)) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protection_problem_moves_from_active_to_resolved_history() {
        let service = ProblemService::default();
        service.update_protection(ProtectionEventFrame { report: 0, warning: 0, error: 8, trip: 0 });
        let first = service.snapshot();
        assert_eq!(first.active.len(), 1);
        assert_eq!(first.active[0].id, "protection.error");
        assert_eq!(first.active[0].code, 8);

        service.update_protection(ProtectionEventFrame { report: 0, warning: 0, error: 0, trip: 0 });
        let resolved = service.snapshot();
        assert!(resolved.active.is_empty());
        assert_eq!(resolved.history.len(), 1);
        assert!(!resolved.history[0].active);
    }

    #[test]
    fn changing_active_mask_counts_as_another_occurrence() {
        let service = ProblemService::default();
        service.update_protection(ProtectionEventFrame { report: 0, warning: 1, error: 0, trip: 0 });
        service.update_protection(ProtectionEventFrame { report: 0, warning: 3, error: 0, trip: 0 });
        assert_eq!(service.snapshot().active[0].occurrence_count, 2);
    }

    #[test]
    fn clearing_history_keeps_active_problem() {
        let service = ProblemService::default();
        service.update_protection(ProtectionEventFrame { report: 1, warning: 0, error: 0, trip: 0 });
        service.clear_history();
        let snapshot = service.snapshot();
        assert_eq!(snapshot.active.len(), 1);
        assert_eq!(snapshot.history.len(), 1);
    }
    #[test]
    fn encoder_health_problem_recovers_without_clearing_history() {
        let service = ProblemService::default();
        service.update_encoder_health(true, 1, 0, 0);
        let invalid = service.snapshot();
        assert_eq!(invalid.active.len(), 1);
        assert_eq!(invalid.active[0].id, "encoder.valid");
        assert_eq!(invalid.active[0].domain, ProblemDomain::Encoder);
        service.update_encoder_health(true, 1, 1, 0);
        assert!(service.snapshot().active.is_empty());
        assert_eq!(service.snapshot().history.len(), 1);
        service.update_encoder_health(true, 1, 0, 1);
        assert_eq!(service.snapshot().active[0].id, "encoder.fault");
        service.update_encoder_health(false, 0, 0, 0);
        assert!(service.snapshot().active.is_empty());
    }

}
