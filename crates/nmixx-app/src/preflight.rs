use thiserror::Error;

use crate::{HostSchema, ParameterService, ParameterServiceError, ParameterValue};

const MOTOR_PP: &str = "PARAM_MOTOR_PP";
const MOTOR_RS: &str = "PARAM_MOTOR_RS";
const MOTOR_LD: &str = "PARAM_MOTOR_LD";
const MOTOR_LQ: &str = "PARAM_MOTOR_LQ";
const MOTOR_FLUX: &str = "PARAM_MOTOR_FLUX";
const LIMIT_I_MAX: &str = "PARAM_LIMIT_I_MAX";
const LIMIT_WM_MAX: &str = "PARAM_LIMIT_WM_MAX";
const IDENT_IF_CURRENT: &str = "PARAM_IDENT_IF_CURRENT";
const IDENT_JB_EXCITE_RATIO: &str = "PARAM_IDENT_JB_EXCITE_RATIO";
const IDENT_JB_EXCITE_HZ: &str = "PARAM_IDENT_JB_EXCITE_HZ";

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
    Identification,
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

        let current_limit = self.require_positive(
            LIMIT_I_MAX,
            PreflightDomain::LimitsSafety,
            "Current limit must be configured before identification.",
            &mut issues,
        )?;

        match kind {
            IdentificationKind::RsLs => {}
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
