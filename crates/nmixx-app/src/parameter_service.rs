use thiserror::Error;

use crate::{DeviceSession, HostSchema, ParameterMetadata, ParameterType, ParameterValue, SchemaError, SessionError};

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
    #[error(transparent)]
    Schema(#[from] SchemaError),
    #[error(transparent)]
    Session(#[from] SessionError),
}

#[derive(Clone)]
pub struct ParameterService {
    session: DeviceSession,
    schema: HostSchema,
}

impl ParameterService {
    pub fn new(session: DeviceSession, schema: HostSchema) -> Self {
        Self { session, schema }
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

    pub fn read(&self, id: u16) -> Result<ParameterValue, ParameterServiceError> {
        let metadata = self.metadata(id)?;
        let ty = metadata.parameter_type()?;
        Ok(self.session.parameter_read(id, ty)?)
    }

    pub fn read_many(
        &self,
        ids: &[u16],
    ) -> Result<Vec<(u16, ParameterValue)>, ParameterServiceError> {
        ids.iter()
            .copied()
            .map(|id| self.read(id).map(|value| (id, value)))
            .collect()
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

        Ok(self.session.parameter_write(id, value)?)
    }
}
