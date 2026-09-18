use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use thiserror::Error;

use crate::{
    DeviceSession, HostSchema, ParameterMetadata, ParameterType, ParameterValue, SchemaError,
    SchemaNumber, SessionError,
};

#[derive(Debug, Error)]
pub enum ParameterServiceError {
    #[error("unknown parameter ID 0x{0:04X}")]
    UnknownParameter(u16),
    #[error("parameter 0x{0:04X} is read-only")]
    ReadOnly(u16),
    #[error("parameter 0x{id:04X} type mismatch: expected {expected:?}, got {actual:?}")]
    TypeMismatch {
        id: u16,
        expected: ParameterType,
        actual: ParameterType,
    },
    #[error("parameter cache lock is poisoned")]
    CachePoisoned,
    #[error("parameter 0x{id:04X} value {value} is outside the HostSchema range")]
    OutOfRange { id: u16, value: String },
    #[error("parameter 0x{id:04X} value {value} is not allowed by the HostSchema")]
    NotAllowed { id: u16, value: String },
    #[error(transparent)]
    Schema(#[from] SchemaError),
    #[error(transparent)]
    Session(#[from] SessionError),
}

#[derive(Clone)]
pub struct ParameterService {
    session: DeviceSession,
    schema: HostSchema,
    cache: Arc<RwLock<HashMap<u16, ParameterValue>>>,
}

impl ParameterService {
    pub fn new(session: DeviceSession, schema: HostSchema) -> Self {
        Self {
            session,
            schema,
            cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn schema(&self) -> &HostSchema {
        &self.schema
    }

    pub fn parameters(&self) -> &[ParameterMetadata] {
        &self.schema.parameters
    }

    pub fn metadata(&self, id: u16) -> Result<&ParameterMetadata, ParameterServiceError> {
        self.schema
            .parameter_by_id(id)
            .ok_or(ParameterServiceError::UnknownParameter(id))
    }

    pub fn cached(&self, id: u16) -> Result<Option<ParameterValue>, ParameterServiceError> {
        self.metadata(id)?;
        Ok(self
            .cache
            .read()
            .map_err(|_| ParameterServiceError::CachePoisoned)?
            .get(&id)
            .cloned())
    }

    pub fn read(&self, id: u16) -> Result<ParameterValue, ParameterServiceError> {
        let metadata = self.metadata(id)?;
        let ty = metadata.parameter_type()?;
        let value = self.session.parameter_read(id, ty)?;
        self.cache
            .write()
            .map_err(|_| ParameterServiceError::CachePoisoned)?
            .insert(id, value.clone());
        Ok(value)
    }

    /// Convenience batch read. Each requested ID produces one independent result in input order.
    /// A failed item does not discard successful values or stop the remaining reads.
    pub fn read_many(
        &self,
        ids: &[u16],
    ) -> Result<Vec<(u16, Result<ParameterValue, ParameterServiceError>)>, ParameterServiceError> {
        Ok(ids
            .iter()
            .copied()
            .map(|id| (id, self.read(id)))
            .collect())
    }

    /// Refresh every readable Host-visible parameter into the shared cache.
    /// Individual read failures are preserved without discarding successful values.
    pub fn refresh_all(
        &self,
    ) -> Result<Vec<(u16, Result<ParameterValue, ParameterServiceError>)>, ParameterServiceError> {
        let ids = self
            .parameters()
            .iter()
            .filter(|parameter| parameter.access.contains('r'))
            .map(|parameter| parameter.id)
            .collect::<Vec<_>>();
        self.read_many(&ids)
    }

    pub fn write(&self, id: u16, value: ParameterValue) -> Result<(), ParameterServiceError> {
        let metadata = self.metadata(id)?;
        if !metadata.access.contains('w') {
            return Err(ParameterServiceError::ReadOnly(id));
        }

        let expected = metadata.parameter_type()?;
        let actual = value.parameter_type();
        if actual != expected {
            return Err(ParameterServiceError::TypeMismatch {
                id,
                expected,
                actual,
            });
        }

        validate_static_constraints(metadata, &value)?;

        self.session.parameter_write(id, value.clone())?;
        self.cache
            .write()
            .map_err(|_| ParameterServiceError::CachePoisoned)?
            .insert(id, value);
        Ok(())
    }
}


fn validate_static_constraints(
    metadata: &ParameterMetadata,
    value: &ParameterValue,
) -> Result<(), ParameterServiceError> {
    let Some(number) = parameter_number(value) else {
        return Ok(());
    };

    if !metadata.allowed.is_empty()
        && !metadata
            .allowed
            .iter()
            .copied()
            .any(|allowed| schema_number(allowed) == number)
    {
        return Err(ParameterServiceError::NotAllowed {
            id: metadata.id,
            value: number.to_string(),
        });
    }

    if let Some(range) = metadata.range.as_ref() {
        if let Some(min) = range.min {
            let min = schema_number(min);
            let invalid = if range.exclusive_min {
                number <= min
            } else {
                number < min
            };
            if invalid {
                return Err(ParameterServiceError::OutOfRange {
                    id: metadata.id,
                    value: number.to_string(),
                });
            }
        }

        if let Some(max) = range.max {
            let max = schema_number(max);
            let invalid = if range.exclusive_max {
                number >= max
            } else {
                number > max
            };
            if invalid {
                return Err(ParameterServiceError::OutOfRange {
                    id: metadata.id,
                    value: number.to_string(),
                });
            }
        }
    }

    Ok(())
}

fn parameter_number(value: &ParameterValue) -> Option<f64> {
    match value {
        ParameterValue::U8(value) => Some(f64::from(*value)),
        ParameterValue::I8(value) => Some(f64::from(*value)),
        ParameterValue::F32(value) => Some(f64::from(*value)),
        ParameterValue::I32(value) => Some(f64::from(*value)),
        ParameterValue::U32(value) => Some(f64::from(*value)),
        ParameterValue::Position(_) => None,
    }
}

fn schema_number(value: SchemaNumber) -> f64 {
    match value {
        SchemaNumber::Integer(value) => value as f64,
        SchemaNumber::Float(value) => value,
    }
}
