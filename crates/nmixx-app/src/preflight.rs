use thiserror::Error;

use crate::{HostSchema, ParameterService, ParameterServiceError, ParameterValue};

const LIMIT_I_MAX: &str = "PARAM_LIMIT_I_MAX";
const LIMIT_WM_MAX: &str = "PARAM_LIMIT_WM_MAX";
const IDENT_RS_LS_VALID: &str = "PARAM_IDENT_RS_LS_VALID";
const IDENT_FLUX_VALID: &str = "PARAM_IDENT_FLUX_VALID";

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

    pub fn check_identification(
        &self,
        kind: IdentificationKind,
    ) -> Result<Vec<PreflightIssue>, PreflightError> {
        let mut issues = Vec::new();

        self.require_positive(
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
                self.require_flag(
                    IDENT_RS_LS_VALID,
                    PreflightDomain::Motor,
                    "Apply a valid Rs/Ls result before Flux identification.",
                    &mut issues,
                )?;
            }
            IdentificationKind::Jb => {
                self.require_positive(
                    LIMIT_WM_MAX,
                    PreflightDomain::LimitsSafety,
                    "Speed limit must be configured before J/B identification.",
                    &mut issues,
                )?;
                self.require_flag(
                    IDENT_FLUX_VALID,
                    PreflightDomain::Motor,
                    "Apply a valid Flux result before J/B identification.",
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
    ) -> Result<(), PreflightError> {
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
        }
        Ok(())
    }

    fn require_flag(
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
        let value = self.parameters.read(metadata.id)?;
        let valid = match value {
            ParameterValue::U8(value) => value == 1,
            _ => return Err(PreflightError::InvalidParameterType(key.to_owned())),
        };

        if !valid {
            issues.push(PreflightIssue {
                parameter_id: Some(metadata.id),
                reason: reason.to_owned(),
                suggested_domain: domain,
            });
        }
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
