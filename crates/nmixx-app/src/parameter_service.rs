use std::collections::HashMap;
use std::sync::{Arc, RwLock};

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
    #[error("parameter cache lock is poisoned")]
    CachePoisoned,
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

        self.session.parameter_write(id, value.clone())?;
        self.cache
            .write()
            .map_err(|_| ParameterServiceError::CachePoisoned)?
            .insert(id, value);
        Ok(())
    }
}
