use std::collections::HashMap;
use std::sync::{mpsc, Arc, Mutex, RwLock};

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
    #[error("parameter 0x{id:04X} cannot be written while motor state is {state}")]
    InvalidWriteState { id: u16, state: String },
    #[error("unsupported write_state '{0}' in HostSchema")]
    UnsupportedWriteState(String),
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
    subscribers: Arc<Mutex<Vec<mpsc::Sender<Vec<u16>>>>>,
}

impl ParameterService {
    pub fn new(session: DeviceSession, schema: HostSchema) -> Self {
        Self {
            session,
            schema,
            cache: Arc::new(RwLock::new(HashMap::new())),
            subscribers: Arc::new(Mutex::new(Vec::new())),
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

    pub fn subscribe(&self) -> Result<mpsc::Receiver<Vec<u16>>, ParameterServiceError> {
        let (sender, receiver) = mpsc::channel();
        self.subscribers
            .lock()
            .map_err(|_| ParameterServiceError::CachePoisoned)?
            .push(sender);
        Ok(receiver)
    }

    pub fn read(&self, id: u16) -> Result<ParameterValue, ParameterServiceError> {
        let value = self.read_cache(id)?;
        self.notify_changed(vec![id]);
        Ok(value)
    }

    /// Convenience batch read. Each requested ID produces one independent result in input order.
    /// A failed item does not discard successful values or stop the remaining reads.
    pub fn read_many(
        &self,
        ids: &[u16],
    ) -> Result<Vec<(u16, Result<ParameterValue, ParameterServiceError>)>, ParameterServiceError> {
        let mut changed = Vec::new();
        let results = ids
            .iter()
            .copied()
            .map(|id| {
                let result = self.read_cache(id);
                if result.is_ok() {
                    changed.push(id);
                }
                (id, result)
            })
            .collect();
        self.notify_changed(changed);
        Ok(results)
    }

    fn read_cache(&self, id: u16) -> Result<ParameterValue, ParameterServiceError> {
        let metadata = self.metadata(id)?;
        let ty = metadata.parameter_type()?;
        let value = self.session.parameter_read(id, ty)?;
        self.cache
            .write()
            .map_err(|_| ParameterServiceError::CachePoisoned)?
            .insert(id, value.clone());
        Ok(value)
    }

    fn notify_changed(&self, ids: Vec<u16>) {
        if ids.is_empty() {
            return;
        }
        if let Ok(mut subscribers) = self.subscribers.lock() {
            subscribers.retain(|subscriber| subscriber.send(ids.clone()).is_ok());
        }
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
        self.write_readback(id, value).map(|_| ())
    }

    pub fn write_readback(
        &self,
        id: u16,
        value: ParameterValue,
    ) -> Result<ParameterValue, ParameterServiceError> {
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
        self.validate_write_state(metadata)?;

        self.session.parameter_write(id, value)?;
        self.read(id)
    }
    fn validate_write_state(
        &self,
        metadata: &ParameterMetadata,
    ) -> Result<(), ParameterServiceError> {
        let Some(write_state) = metadata.write_state.as_deref() else {
            return Ok(());
        };

        let state_metadata = self
            .schema
            .parameter_by_key("PARAM_MOTOR_STATE")
            .ok_or_else(|| ParameterServiceError::Schema(SchemaError::Parse(
                "HostSchema is missing PARAM_MOTOR_STATE".to_owned(),
            )))?;
        let state_value = self.read(state_metadata.id)?;
        let ParameterValue::U8(state) = state_value else {
            return Err(ParameterServiceError::TypeMismatch {
                id: state_metadata.id,
                expected: ParameterType::U8,
                actual: state_value.parameter_type(),
            });
        };

        let disabled = enum_u8(state_metadata, "DISABLED")?;
        let run = enum_u8(state_metadata, "RUN")?;
        let allowed = match write_state {
            "disabled" => state == disabled,
            "not_running" => state != run,
            other => {
                return Err(ParameterServiceError::UnsupportedWriteState(other.to_owned()));
            }
        };

        if !allowed {
            return Err(ParameterServiceError::InvalidWriteState {
                id: metadata.id,
                state: state_symbol(state_metadata, state),
            });
        }

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

    if !number.is_finite() {
        return Err(ParameterServiceError::OutOfRange {
            id: metadata.id,
            value: number.to_string(),
        });
    }

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


fn enum_u8(
    metadata: &ParameterMetadata,
    wanted_symbol: &str,
) -> Result<u8, ParameterServiceError> {
    let Some(index) = metadata
        .allowed_symbols
        .iter()
        .position(|symbol| symbol == wanted_symbol)
    else {
        return Err(ParameterServiceError::Schema(SchemaError::Parse(format!(
            "parameter '{}' does not expose enum symbol '{}'",
            metadata.symbol, wanted_symbol
        ))));
    };

    if let Some(value) = metadata.allowed.get(index).copied() {
        let number = schema_number(value);
        if number.is_finite()
            && number.fract() == 0.0
            && number >= 0.0
            && number <= f64::from(u8::MAX)
        {
            return Ok(number as u8);
        }
        return Err(ParameterServiceError::Schema(SchemaError::Parse(format!(
            "parameter '{}' enum symbol '{}' is not a u8 value",
            metadata.symbol, wanted_symbol
        ))));
    }

    u8::try_from(index).map_err(|_| {
        ParameterServiceError::Schema(SchemaError::Parse(format!(
            "parameter '{}' enum symbol '{}' ordinal does not fit u8",
            metadata.symbol, wanted_symbol
        )))
    })
}

fn state_symbol(metadata: &ParameterMetadata, state: u8) -> String {
    metadata
        .allowed_symbols
        .get(usize::from(state))
        .cloned()
        .unwrap_or_else(|| state.to_string())
}
