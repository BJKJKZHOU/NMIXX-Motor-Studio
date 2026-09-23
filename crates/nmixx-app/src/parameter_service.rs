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
    #[error("parameter 0x{id:04X} is not synchronized: {message}")]
    CachedReadFailed { id: u16, message: String },
    #[error("parameter 0x{0:04X} has not been read into the shared cache")]
    CacheMissing(u16),
    #[error("RAM write to parameter 0x{id:04X} succeeded, but readback failed: {details}")]
    ReadbackFailed { id: u16, details: String },
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
    cache: Arc<RwLock<HashMap<u16, Result<ParameterValue, String>>>>,
    // Serialize device reads and write/readback groups, not cache-only views or Actions.
    io: Arc<Mutex<()>>,
    subscribers: Arc<Mutex<Vec<mpsc::Sender<Vec<u16>>>>>,
}

impl ParameterService {
    pub fn new(session: DeviceSession, schema: HostSchema) -> Self {
        Self {
            session,
            schema,
            cache: Arc::new(RwLock::new(HashMap::new())),
            io: Arc::new(Mutex::new(())),
            subscribers: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn schema(&self) -> &HostSchema {
        &self.schema
    }

    pub fn parameters(&self) -> &[ParameterMetadata] {
        self.schema.parameters.as_slice()
    }

    pub fn metadata(&self, id: u16) -> Result<&ParameterMetadata, ParameterServiceError> {
        self.schema
            .parameter_by_id(id)
            .ok_or(ParameterServiceError::UnknownParameter(id))
    }

    pub fn cached(&self, id: u16) -> Result<Option<ParameterValue>, ParameterServiceError> {
        self.metadata(id)?;
        let cache = self.cache.read().map_err(|_| ParameterServiceError::CachePoisoned)?;
        match cache.get(&id) {
            Some(Ok(value)) => Ok(Some(value.clone())),
            Some(Err(message)) => Err(ParameterServiceError::CachedReadFailed {
                id,
                message: message.clone(),
            }),
            None => Ok(None),
        }
    }

    /// One cache-only snapshot. It does not perform I/O or emit change notifications.
    pub fn snapshot(&self) -> Result<HashMap<u16, Result<ParameterValue, String>>, ParameterServiceError> {
        Ok(self.cache.read().map_err(|_| ParameterServiceError::CachePoisoned)?.clone())
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
        let mut results = self.read_many(&[id])?;
        results.remove(0).1
    }

    /// Read a group and publish its value/error changes together. A failed read invalidates
    /// the old cache entry rather than presenting an old value as a successful readback.
    pub fn read_many(
        &self,
        ids: &[u16],
    ) -> Result<Vec<(u16, Result<ParameterValue, ParameterServiceError>)>, ParameterServiceError> {
        let _io = self.io.lock().map_err(|_| ParameterServiceError::CachePoisoned)?;
        self.read_many_locked(ids)
    }

    fn read_many_locked(
        &self,
        ids: &[u16],
    ) -> Result<Vec<(u16, Result<ParameterValue, ParameterServiceError>)>, ParameterServiceError> {
        let results = ids.iter().copied().map(|id| {
            let result = (|| {
                let ty = self.metadata(id)?.parameter_type()?;
                Ok(self.session.parameter_read(id, ty)?)
            })();
            (id, result)
        }).collect::<Vec<_>>();
        let entries = results.iter().filter(|(id, _)| self.schema.parameter_by_id(*id).is_some()).map(|(id, result)| {
            (*id, result.as_ref().cloned().map_err(ToString::to_string))
        }).collect::<Vec<_>>();
        let changed = {
            let mut cache = self.cache.write().map_err(|_| ParameterServiceError::CachePoisoned)?;
            update_cache(&mut cache, entries)
        };
        self.notify_changed(changed);
        Ok(results)
    }

    fn readback_ids(&self, id: u16) -> Result<Vec<u16>, ParameterServiceError> {
        let symbol = &self.metadata(id)?.symbol;
        let mut ids = vec![id];
        ids.extend(self.parameters().iter().filter(|meta| {
            meta.id != id && meta.access.contains('r') && related_parameter(symbol, &meta.symbol)
        }).map(|meta| meta.id));
        Ok(ids)
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

        let ids = self.readback_ids(id)?;
        let _io = self.io.lock().map_err(|_| ParameterServiceError::CachePoisoned)?;
        self.session.parameter_write(id, value)?;
        let results = self.read_many_locked(&ids)?;
        let mut written = None;
        let mut failures = Vec::new();
        for (read_id, result) in results {
            match result {
                Ok(value) if read_id == id => written = Some(value),
                Ok(_) => {}
                Err(error) => failures.push(format!("0x{read_id:04X}: {error}")),
            }
        }
        if !failures.is_empty() {
            return Err(ParameterServiceError::ReadbackFailed { id, details: failures.join("; ") });
        }
        written.ok_or(ParameterServiceError::CacheMissing(id))
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


fn update_cache(
    cache: &mut HashMap<u16, Result<ParameterValue, String>>,
    entries: Vec<(u16, Result<ParameterValue, String>)>,
) -> Vec<u16> {
    let mut changed = Vec::new();
    for (id, entry) in entries {
        if cache.get(&id) != Some(&entry) {
            cache.insert(id, entry);
            if !changed.contains(&id) { changed.push(id); }
        }
    }
    changed
}

#[cfg(test)]
mod linkage_readback_tests {
    use super::related_parameter;

    #[test]
    fn motion_limit_write_invalidates_the_effective_speed_view() {
        assert!(related_parameter("PARAM_MOTION_WM_MAX", "PARAM_LIMIT_WM_EFFECTIVE"));
    }
    #[test]
    fn model_and_encoder_changes_reread_actual_calibration_validity() {
        for written in ["PARAM_MOTOR_PP", "PARAM_ENCODER_PROTOCOL", "PARAM_ENCODER_SPI_TYPE"] {
            assert!(related_parameter(written, "PARAM_CAL_VALID"));
        }
        assert!(!related_parameter("PARAM_ENCODER_PROTOCOL", "PARAM_POSITION_ZERO_VALID"));
    }
    #[test]
    fn encoder_reset_and_direction_mapping_refresh_user_feedback() {
        for written in ["PARAM_ENCODER_PROTOCOL", "PARAM_ENCODER_SPI_TYPE", "PARAM_MOTOR_DIR"] {
            for candidate in ["PARAM_RUN_POSITION", "PARAM_RUN_WM"] {
                assert!(related_parameter(written, candidate));
            }
        }
    }
}

// Firmware owns these calculations. The host only rereads their outputs. This small
// dependency table is shared by every client; pages must not supply readback lists.
fn related_parameter(written: &str, candidate: &str) -> bool {
    const CURRENT: &[&str] = &[
        "PARAM_CTRL_CURRENT_BW_HZ", "PARAM_CTRL_CURRENT_SOURCE",
        "PARAM_CTRL_ID_KP", "PARAM_CTRL_ID_KI", "PARAM_CTRL_IQ_KP", "PARAM_CTRL_IQ_KI",
    ];
    const SPEED: &[&str] = &[
        "PARAM_CTRL_SPEED_BW_HZ", "PARAM_CTRL_SPEED_SOURCE", "PARAM_CTRL_SPEED_KP", "PARAM_CTRL_SPEED_KI",
    ];
    const MODEL: &[&str] = &[
        "PARAM_MOTOR_PP", "PARAM_MOTOR_RS", "PARAM_MOTOR_LD", "PARAM_MOTOR_LQ",
        "PARAM_MOTOR_FLUX", "PARAM_MOTOR_J", "PARAM_MOTOR_B",
    ];
    (CURRENT.contains(&written) && CURRENT.contains(&candidate))
        || (SPEED.contains(&written) && SPEED.contains(&candidate))
        || (MODEL.contains(&written)
            && (candidate.starts_with("PARAM_CTRL_") || candidate.starts_with("PARAM_LIMIT_")))
        || (written.starts_with("PARAM_LIMIT_") && candidate.starts_with("PARAM_LIMIT_"))
        || (written == "PARAM_MOTION_WM_MAX" && candidate == "PARAM_LIMIT_WM_EFFECTIVE")
        || (written == "PARAM_MOTOR_PP" && candidate == "PARAM_CAL_VALID")
        || ((written.starts_with("PARAM_ENCODER_") || written == "PARAM_MOTOR_DIR")
            && (candidate.starts_with("PARAM_ENCODER_") || candidate == "PARAM_CAL_VALID"
                || candidate == "PARAM_RUN_POSITION" || candidate == "PARAM_RUN_WM"))
        || (written == "PARAM_MOTOR_MODE"
            && (candidate == "PARAM_MOTOR_STATE" || candidate.starts_with("PARAM_TARGET_")))
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

#[cfg(test)]
mod shared_cache_tests {
    use super::*;

    #[test]
    fn repeated_reads_do_not_publish_changes() {
        let mut cache = HashMap::new();
        let entry = (7, Ok(ParameterValue::F32(1.25)));
        assert_eq!(update_cache(&mut cache, vec![entry.clone()]), vec![7]);
        assert!(update_cache(&mut cache, vec![entry]).is_empty());
    }

    #[test]
    fn failures_replace_stale_values_and_recovery_is_a_change() {
        let mut cache = HashMap::new();
        update_cache(&mut cache, vec![(7, Ok(ParameterValue::F32(1.25)))]);
        let error = (7, Err("timeout".to_owned()));
        assert_eq!(update_cache(&mut cache, vec![error.clone()]), vec![7]);
        assert!(cache.get(&7).unwrap().is_err());
        assert!(update_cache(&mut cache, vec![error]).is_empty());
        assert_eq!(update_cache(&mut cache, vec![(7, Ok(ParameterValue::F32(1.25)))]), vec![7]);
    }

    #[test]
    fn current_bandwidth_and_gains_always_include_source() {
        assert!(related_parameter("PARAM_CTRL_CURRENT_BW_HZ", "PARAM_CTRL_CURRENT_SOURCE"));
        assert!(related_parameter("PARAM_CTRL_ID_KP", "PARAM_CTRL_CURRENT_SOURCE"));
        assert!(related_parameter("PARAM_CTRL_CURRENT_SOURCE", "PARAM_CTRL_IQ_KI"));
        assert!(related_parameter("PARAM_MOTOR_RS", "PARAM_CTRL_IQ_KI"));
        assert!(related_parameter("PARAM_MOTOR_J", "PARAM_CTRL_SPEED_KP"));
        assert!(!related_parameter("PARAM_TARGET_SPEED", "PARAM_CTRL_SPEED_KP"));
    }

    #[test]
    fn groups_publish_only_the_ids_that_changed() {
        let mut cache = HashMap::new();
        update_cache(&mut cache, vec![(1, Ok(ParameterValue::U8(0)))]);
        assert_eq!(update_cache(&mut cache, vec![
            (1, Ok(ParameterValue::U8(0))), (2, Ok(ParameterValue::F32(10.0))),
        ]), vec![2]);
    }
}
