use thiserror::Error;

use crate::parameter_service::ParameterService;
use crate::{HostSchema, ParameterServiceError, ParameterValue};

const MOTOR_PP: &str = "PARAM_MOTOR_PP";
const MOTOR_RS: &str = "PARAM_MOTOR_RS";
const MOTOR_LD: &str = "PARAM_MOTOR_LD";
const MOTOR_LQ: &str = "PARAM_MOTOR_LQ";
const MOTOR_FLUX: &str = "PARAM_MOTOR_FLUX";
const LIMIT_I_MAX: &str = "PARAM_LIMIT_I_MAX";
const LIMIT_WM_MAX: &str = "PARAM_LIMIT_WM_MAX";
const IDENT_IF_CURRENT: &str = "PARAM_MOTOR_IF_CURRENT";
const ALIGN_CURRENT: &str = "PARAM_MOTOR_ALIGN_CURRENT";
const RL_I_PEAK: &str = "PARAM_MOTOR_RL_I_PEAK";
const IDENT_JB_EXCITE_RATIO: &str = "PARAM_IDENT_JB_EXCITE_RATIO";
const IDENT_JB_EXCITE_HZ: &str = "PARAM_IDENT_JB_EXCITE_HZ";
const ENCODER_READY: &str = "PARAM_ENCODER_READY";
const ENCODER_VALID: &str = "PARAM_ENCODER_VALID";
const ENCODER_FAULT: &str = "PARAM_ENCODER_FAULT";
const PHASE_I_SEARCH: &str = "PARAM_PHASE_I_SEARCH";
const EVENT_ERROR: &str = "PARAM_EVENT_ERROR";
const EVENT_TRIP: &str = "PARAM_EVENT_TRIP";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentificationKind {
    RsLs,
    Flux,
    Jb,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreflightDomain {
    LimitsSafety,
    Motor,
    Encoder,
    Identification,
    Protection,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreflightIssue {
    pub parameter_id: Option<u16>,
    pub reason: String,
    pub suggested_domain: PreflightDomain,
}

#[derive(Debug, Error)]
pub enum PreflightError {
    #[error("required parameter '{0}' is not exposed by the HostSchema")]
    MissingParameter(String),
    #[error("parameter '{0}' has an unexpected value type")]
    InvalidParameterType(String),
    #[error(transparent)]
    Parameter(#[from] ParameterServiceError),
}

#[derive(Clone)]
pub struct PreflightService {
    schema: HostSchema,
    parameters: ParameterService,
}

impl PreflightService {
    pub fn new(parameters: ParameterService) -> Self {
        let schema = parameters.schema().clone();
        Self { schema, parameters }
    }

    /// Check user-actionable prerequisites for an identification operation.
    ///
    /// This does not start, sequence, apply, or abort identification actions.
    /// Device-side start validation remains authoritative.
    pub fn check_identification(
        &self,
        kind: IdentificationKind,
    ) -> Result<Vec<PreflightIssue>, PreflightError> {
        let mut issues = Vec::new();

        self.require_clear_protection(&mut issues)?;

        let current_limit = self.require_positive(
            LIMIT_I_MAX,
            PreflightDomain::LimitsSafety,
            "Current limit must be configured before identification.",
            &mut issues,
        )?;

        match kind {
            IdentificationKind::RsLs => {
                let peak = self.require_positive(
                    RL_I_PEAK,
                    PreflightDomain::Identification,
                    "Rs/Ls injection peak current must be configured.",
                    &mut issues,
                )?;
                if let (Some(peak), Some(limit)) = (peak, current_limit) {
                    if peak >= limit {
                        self.push_issue(
                            RL_I_PEAK,
                            PreflightDomain::Identification,
                            "Rs/Ls injection peak must be below the configured current limit.",
                            &mut issues,
                        )?;
                    }
                }
            }
            IdentificationKind::Flux => {
                self.require_positive(
                    LIMIT_WM_MAX,
                    PreflightDomain::LimitsSafety,
                    "Speed limit must be configured before Flux identification.",
                    &mut issues,
                )?;
                self.require_positive(
                    MOTOR_PP,
                    PreflightDomain::Motor,
                    "Pole pairs must be configured before Flux identification.",
                    &mut issues,
                )?;
                self.require_positive(
                    MOTOR_RS,
                    PreflightDomain::Motor,
                    "Active Rs must be valid before Flux identification.",
                    &mut issues,
                )?;
                self.require_positive(
                    MOTOR_LD,
                    PreflightDomain::Motor,
                    "Active Ld must be valid before Flux identification.",
                    &mut issues,
                )?;
                self.require_positive(
                    MOTOR_LQ,
                    PreflightDomain::Motor,
                    "Active Lq must be valid before Flux identification.",
                    &mut issues,
                )?;
                let align = self.require_positive(
                    ALIGN_CURRENT,
                    PreflightDomain::Identification,
                    "Align current must be configured before Flux identification.",
                    &mut issues,
                )?;
                if let (Some(align), Some(limit)) = (align, current_limit) {
                    if align > limit {
                        self.push_issue(
                            ALIGN_CURRENT,
                            PreflightDomain::Identification,
                            "Align current must not exceed the configured current limit.",
                            &mut issues,
                        )?;
                    }
                }
                let if_current = self.require_positive(
                    IDENT_IF_CURRENT,
                    PreflightDomain::Identification,
                    "I/F startup current must be configured before Flux identification.",
                    &mut issues,
                )?;
                if let (Some(if_current), Some(current_limit)) = (if_current, current_limit) {
                    if if_current > current_limit {
                        self.push_issue(
                            IDENT_IF_CURRENT,
                            PreflightDomain::Identification,
                            "I/F startup current must not exceed the configured current limit.",
                            &mut issues,
                        )?;
                    }
                }
            }
            IdentificationKind::Jb => {
                self.require_positive(
                    LIMIT_WM_MAX,
                    PreflightDomain::LimitsSafety,
                    "Speed limit must be configured before J/B identification.",
                    &mut issues,
                )?;
                self.require_positive(
                    MOTOR_PP,
                    PreflightDomain::Motor,
                    "Pole pairs must be configured before J/B identification.",
                    &mut issues,
                )?;
                self.require_positive(
                    MOTOR_RS,
                    PreflightDomain::Motor,
                    "Active Rs must be valid before J/B identification.",
                    &mut issues,
                )?;
                self.require_positive(
                    MOTOR_LD,
                    PreflightDomain::Motor,
                    "Active Ld must be valid before J/B identification.",
                    &mut issues,
                )?;
                self.require_positive(
                    MOTOR_LQ,
                    PreflightDomain::Motor,
                    "Active Lq must be valid before J/B identification.",
                    &mut issues,
                )?;
                self.require_positive(
                    MOTOR_FLUX,
                    PreflightDomain::Motor,
                    "Active Flux must be valid before J/B identification.",
                    &mut issues,
                )?;
                let align = self.require_positive(
                    ALIGN_CURRENT,
                    PreflightDomain::Identification,
                    "Align current must be configured before J/B identification.",
                    &mut issues,
                )?;
                let if_current = self.require_positive(
                    IDENT_IF_CURRENT,
                    PreflightDomain::Identification,
                    "I/F startup current must be configured before J/B identification.",
                    &mut issues,
                )?;
                if let Some(limit) = current_limit {
                    if align.is_some_and(|value| value > limit) {
                        self.push_issue(
                            ALIGN_CURRENT,
                            PreflightDomain::Identification,
                            "Align current must not exceed the configured current limit.",
                            &mut issues,
                        )?;
                    }
                    if if_current.is_some_and(|value| value > limit) {
                        self.push_issue(
                            IDENT_IF_CURRENT,
                            PreflightDomain::Identification,
                            "I/F startup current must not exceed the configured current limit.",
                            &mut issues,
                        )?;
                    }
                }
                self.require_positive(
                    IDENT_JB_EXCITE_RATIO,
                    PreflightDomain::Identification,
                    "J/B excitation ratio must be configured.",
                    &mut issues,
                )?;
                self.require_positive(
                    IDENT_JB_EXCITE_HZ,
                    PreflightDomain::Identification,
                    "J/B excitation frequency must be configured.",
                    &mut issues,
                )?;
            }
        }

        Ok(issues)
    }

    pub fn check_phase_search(&self) -> Result<Vec<PreflightIssue>, PreflightError> {
        let mut issues = Vec::new();

        self.require_clear_protection(&mut issues)?;

        self.require_flag(
            ENCODER_READY,
            true,
            PreflightDomain::Encoder,
            "Encoder must be ready before phase search.",
            &mut issues,
        )?;
        self.require_flag(
            ENCODER_VALID,
            true,
            PreflightDomain::Encoder,
            "Encoder feedback must be valid before phase search.",
            &mut issues,
        )?;
        self.require_flag(
            ENCODER_FAULT,
            false,
            PreflightDomain::Encoder,
            "Clear the encoder fault before phase search.",
            &mut issues,
        )?;
        self.require_positive(
            MOTOR_PP,
            PreflightDomain::Motor,
            "Pole pairs must be configured before phase search.",
            &mut issues,
        )?;
        self.require_positive(
            LIMIT_I_MAX,
            PreflightDomain::LimitsSafety,
            "Current limit must be configured before phase search.",
            &mut issues,
        )?;
        self.require_positive(
            PHASE_I_SEARCH,
            PreflightDomain::Encoder,
            "Phase-search current must be configured.",
            &mut issues,
        )?;

        Ok(issues)
    }

    fn require_clear_protection(
        &self,
        issues: &mut Vec<PreflightIssue>,
    ) -> Result<(), PreflightError> {
        self.require_zero_u32(
            EVENT_ERROR,
            PreflightDomain::Protection,
            "Clear active protection errors before starting this operation.",
            issues,
        )?;
        self.require_zero_u32(
            EVENT_TRIP,
            PreflightDomain::Protection,
            "Clear the active hardware trip before starting this operation.",
            issues,
        )
    }

    fn require_zero_u32(
        &self,
        key: &str,
        domain: PreflightDomain,
        reason: &str,
        issues: &mut Vec<PreflightIssue>,
    ) -> Result<(), PreflightError> {
        let metadata = self
            .schema
            .parameter_by_key(key)
            .ok_or_else(|| PreflightError::MissingParameter(key.to_owned()))?;
        match self.parameters.read(metadata.id)? {
            ParameterValue::U32(0) => {}
            ParameterValue::U32(_) => issues.push(PreflightIssue {
                parameter_id: Some(metadata.id),
                reason: reason.to_owned(),
                suggested_domain: domain,
            }),
            _ => return Err(PreflightError::InvalidParameterType(key.to_owned())),
        }
        Ok(())
    }

    fn require_flag(
        &self,
        key: &str,
        expected: bool,
        domain: PreflightDomain,
        reason: &str,
        issues: &mut Vec<PreflightIssue>,
    ) -> Result<(), PreflightError> {
        let metadata = self
            .schema
            .parameter_by_key(key)
            .ok_or_else(|| PreflightError::MissingParameter(key.to_owned()))?;
        let value = self.parameters.read(metadata.id)?;
        let actual = match value {
            ParameterValue::U8(value) => value != 0,
            _ => return Err(PreflightError::InvalidParameterType(key.to_owned())),
        };

        if actual != expected {
            issues.push(PreflightIssue {
                parameter_id: Some(metadata.id),
                reason: reason.to_owned(),
                suggested_domain: domain,
            });
        }
        Ok(())
    }

    fn require_positive(
        &self,
        key: &str,
        domain: PreflightDomain,
        reason: &str,
        issues: &mut Vec<PreflightIssue>,
    ) -> Result<Option<f64>, PreflightError> {
        let metadata = self
            .schema
            .parameter_by_key(key)
            .ok_or_else(|| PreflightError::MissingParameter(key.to_owned()))?;
        let value = self.parameters.read(metadata.id)?;
        let numeric = numeric_value(&value)
            .ok_or_else(|| PreflightError::InvalidParameterType(key.to_owned()))?;

        if !numeric.is_finite() || numeric <= 0.0 {
            issues.push(PreflightIssue {
                parameter_id: Some(metadata.id),
                reason: reason.to_owned(),
                suggested_domain: domain,
            });
            return Ok(None);
        }
        Ok(Some(numeric))
    }

    fn push_issue(
        &self,
        key: &str,
        domain: PreflightDomain,
        reason: &str,
        issues: &mut Vec<PreflightIssue>,
    ) -> Result<(), PreflightError> {
        let metadata = self
            .schema
            .parameter_by_key(key)
            .ok_or_else(|| PreflightError::MissingParameter(key.to_owned()))?;
        issues.push(PreflightIssue {
            parameter_id: Some(metadata.id),
            reason: reason.to_owned(),
            suggested_domain: domain,
        });
        Ok(())
    }
}

fn numeric_value(value: &ParameterValue) -> Option<f64> {
    match value {
        ParameterValue::U8(value) => Some(f64::from(*value)),
        ParameterValue::I8(value) => Some(f64::from(*value)),
        ParameterValue::F32(value) => Some(f64::from(*value)),
        ParameterValue::I32(value) => Some(f64::from(*value)),
        ParameterValue::U32(value) => Some(f64::from(*value)),
        ParameterValue::Position(_) => None,
    }
}

#[cfg(test)]
mod onboarding_tests {
    use super::*;
    use std::collections::{HashMap, VecDeque};
    use std::sync::{Arc, Mutex};
    use nmixx_core::protocol::{
        AxdrStatus, ParameterType, MSG_PARAMETER, MSG_RESPONSE, NODE_ID_DEFAULT,
        PARAM_READ, can_id,
    };
    use nmixx_core::transport::{FrameTransport, TransportError};
    use nmixx_core::wire::CanFdFrame;

    struct MotorFixture {
        parameters: Arc<Mutex<HashMap<u16, f32>>>,
        responses: VecDeque<CanFdFrame>,
    }
    impl FrameTransport for MotorFixture {
        fn send(&mut self, frame: &CanFdFrame) -> Result<(), TransportError> {
            let request = frame.data();
            let id = u16::from_le_bytes([request[2], request[3]]);
            let mut body = vec![
                request[0], MSG_PARAMETER, PARAM_READ, AxdrStatus::Ok as u8,
                request[2], request[3],
            ];
            if id == 0x0803 || id == 0x0804 {
                body.push(ParameterType::U32 as u8);
                body.extend_from_slice(&0u32.to_le_bytes());
            } else {
                body.push(ParameterType::F32 as u8);
                let value = *self.parameters.lock().unwrap().get(&id).unwrap_or(&0.0);
                body.extend_from_slice(&value.to_le_bytes());
            }
            self.responses.push_back(
                CanFdFrame::new(can_id(MSG_RESPONSE, NODE_ID_DEFAULT), &body).unwrap()
            );
            Ok(())
        }
        fn receive(&mut self, _timeout: std::time::Duration) -> Result<CanFdFrame, TransportError> {
            self.responses.pop_front().ok_or(TransportError::Timeout)
        }
    }

    fn environment() -> (PreflightService, Arc<Mutex<HashMap<u16, f32>>>) {
        let entries: &[(&str, u16)] = &[
            (EVENT_ERROR, 0x0803), (EVENT_TRIP, 0x0804),
            (LIMIT_I_MAX, 0x0201), (LIMIT_WM_MAX, 0x0202),
            (MOTOR_PP, 0x0110), (MOTOR_RS, 0x0111),
            (MOTOR_LD, 0x0112), (MOTOR_LQ, 0x0113),
            (MOTOR_FLUX, 0x0114), (ALIGN_CURRENT, 0x0117),
            (IDENT_IF_CURRENT, 0x0118), (RL_I_PEAK, 0x0119),
            (IDENT_JB_EXCITE_RATIO, 0x0923), (IDENT_JB_EXCITE_HZ, 0x0924),
        ];
        let mut schema = String::from(
            "schema_version = 1\nprotocol = \"axdr-canfd-v1\"\n[source]\nrepository = \"fixture\"\ngit_sha = \"abc\"\nparameter_schema = 1\n"
        );
        for (symbol, id) in entries {
            let typ = if *symbol == EVENT_ERROR || *symbol == EVENT_TRIP { "u32" } else { "f32" };
            schema.push_str(&format!(
                "\n[[parameters]]\nsymbol = \"{symbol}\"\nlabel = \"{symbol}\"\nid = {id}\ntype = \"{typ}\"\naccess = \"ro\"\ndescription = \"fixture\"\n"
            ));
        }
        let values = Arc::new(Mutex::new(HashMap::from([
            (0x0201, 2.0), (0x0202, 40.0), (0x0110, 7.0),
            (0x0111, 0.3), (0x0112, 0.0002), (0x0113, 0.0002),
            (0x0114, 0.01), (0x0117, 0.5), (0x0118, 0.5),
            (0x0119, 1.0), (0x0923, 0.2), (0x0924, 3.0),
        ])));
        let session = crate::DeviceSession::spawn(Box::new(MotorFixture {
            parameters: values.clone(),
            responses: VecDeque::new(),
        }));
        let parameters = ParameterService::new(session, HostSchema::parse(&schema).unwrap());
        (PreflightService::new(parameters), values)
    }

    #[test]
    fn new_motor_rs_ls_does_not_pass_with_peak_equal_to_user_limit() {
        let (preflight, values) = environment();
        values.lock().unwrap().insert(0x0119, 2.0);
        let issues = preflight.check_identification(IdentificationKind::RsLs).unwrap();
        assert!(issues.iter().any(|issue| issue.parameter_id == Some(0x0119)));
        values.lock().unwrap().insert(0x0119, 1.0);
        assert!(preflight.check_identification(IdentificationKind::RsLs).unwrap().is_empty());
    }

    #[test]
    fn new_motor_flux_and_jb_reject_invalid_align_and_if_currents() {
        let (preflight, values) = environment();
        values.lock().unwrap().insert(0x0117, 0.0);
        let issues = preflight.check_identification(IdentificationKind::Flux).unwrap();
        assert!(issues.iter().any(|issue| issue.parameter_id == Some(0x0117)));
        values.lock().unwrap().insert(0x0117, 3.0);
        let issues = preflight.check_identification(IdentificationKind::Flux).unwrap();
        assert!(issues.iter().any(|issue| issue.parameter_id == Some(0x0117)));
        values.lock().unwrap().insert(0x0117, 0.5);
        assert!(preflight.check_identification(IdentificationKind::Flux).unwrap().is_empty());

        values.lock().unwrap().insert(0x0118, 3.0);
        let issues = preflight.check_identification(IdentificationKind::Jb).unwrap();
        assert!(issues.iter().any(|issue| issue.parameter_id == Some(0x0118)));
        values.lock().unwrap().insert(0x0118, 0.5);
        assert!(preflight.check_identification(IdentificationKind::Jb).unwrap().is_empty());
    }
}
