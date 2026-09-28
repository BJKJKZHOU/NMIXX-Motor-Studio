use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use serde::{Deserialize, Serialize};

use crate::parameter_service::ParameterService;
use crate::{ActionHandle, AxdrStatus, DeviceSession, HostSchema, MotorState, ParameterValue, PositionValue};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MotionMode { Position, Speed, SensorlessSpeed, Torque }

impl MotionMode {
    pub fn symbol(self) -> &'static str {
        match self {
            Self::Position => "POSITION",
            Self::Speed => "SPEED",
            Self::SensorlessSpeed => "SENSORLESS_SPEED",
            Self::Torque => "TORQUE",
        }
    }

    fn from_symbol(symbol: &str) -> Option<Self> {
        match symbol {
            "POSITION" => Some(Self::Position),
            "SPEED" => Some(Self::Speed),
            "SENSORLESS_SPEED" => Some(Self::SensorlessSpeed),
            "TORQUE" => Some(Self::Torque),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PositionCommand { Absolute, Incremental }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MotionConfig {
    pub position_command: PositionCommand,
    pub incremental_delta_turn: f64,
    pub repeat: bool,
}
impl Default for MotionConfig {
    fn default() -> Self {
        Self { position_command: PositionCommand::Incremental, incremental_delta_turn: 1.0, repeat: false }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MotionPreview {
    pub times: Vec<f64>,
    pub primary: Vec<f64>,
    pub secondary: Vec<f64>,
    pub primary_label: &'static str,
    pub secondary_label: Option<&'static str>,
    pub primary_unit: &'static str,
    pub secondary_unit: Option<&'static str>,
}

#[derive(Debug)]
struct MotionRepeatRuntime {
    endpoint_a: Option<f64>,
    endpoint_b: Option<f64>,
    next_is_b: bool,
    pending_target_is_b: Option<bool>,
}
impl Default for MotionRepeatRuntime {
    fn default() -> Self {
        Self { endpoint_a: None, endpoint_b: None, next_is_b: true, pending_target_is_b: None }
    }
}

#[derive(Debug, Clone)]
pub struct MotionService {
    config: Arc<RwLock<MotionConfig>>,
    repeat: Arc<Mutex<MotionRepeatRuntime>>,
}
impl Default for MotionService {
    fn default() -> Self {
        Self {
            config: Arc::new(RwLock::new(MotionConfig::default())),
            repeat: Arc::new(Mutex::new(MotionRepeatRuntime::default())),
        }
    }
}

impl MotionService {
    pub fn get(&self) -> MotionConfig {
        self.config.read().expect("Motion config lock poisoned").clone()
    }
    pub fn set(&self, config: MotionConfig) -> Result<MotionConfig, String> {
        validate_host_config(&config)?;
        let current = self.get();
        let reset_repeat = current.repeat != config.repeat
            || current.position_command != config.position_command
            || current.incremental_delta_turn != config.incremental_delta_turn;
        *self.config.write().map_err(|_| "Motion config lock poisoned")? = config.clone();
        if reset_repeat { self.reset_repeat()?; }
        Ok(config)
    }

    pub(crate) fn repeat_target(
        &self,
        current_position_turn: f64,
        absolute_target_turn: Option<f64>,
    ) -> Result<Option<(f64, bool)>, String> {
        let config = self.get();
        if !config.repeat { return Ok(None); }

        let mut runtime = self.repeat.lock().map_err(|_| "Motion repeat lock poisoned")?;
        if runtime.endpoint_a.is_none() || runtime.endpoint_b.is_none() {
            let target = match config.position_command {
                PositionCommand::Absolute => absolute_target_turn
                    .ok_or_else(|| "Absolute repeat target is unavailable".to_owned())?,
                PositionCommand::Incremental => current_position_turn + config.incremental_delta_turn,
            };
            if (target - current_position_turn).abs() <= 1e-9 {
                return Err("Repeat position endpoints must be different".to_owned());
            }
            runtime.endpoint_a = Some(current_position_turn);
            runtime.endpoint_b = Some(target);
            runtime.next_is_b = true;
            runtime.pending_target_is_b = None;
        }

        let target_is_b = runtime.next_is_b;
        let target = (if target_is_b { runtime.endpoint_b } else { runtime.endpoint_a })
            .ok_or_else(|| "Repeat position endpoints are not initialized".to_owned())?;
        Ok(Some((target, target_is_b)))
    }

    pub(crate) fn repeat_mark_started(&self, target_is_b: bool) -> Result<(), String> {
        self.repeat.lock().map_err(|_| "Motion repeat lock poisoned")?.pending_target_is_b = Some(target_is_b);
        Ok(())
    }

    pub(crate) fn repeat_completed(&self, status: AxdrStatus) -> Result<(), String> {
        let mut runtime = self.repeat.lock().map_err(|_| "Motion repeat lock poisoned")?;
        if let Some(target_is_b) = runtime.pending_target_is_b.take() {
            if status == AxdrStatus::Ok { runtime.next_is_b = !target_is_b; }
        }
        Ok(())
    }

    pub(crate) fn cancel_repeat_leg(&self) -> Result<(), String> {
        self.repeat.lock().map_err(|_| "Motion repeat lock poisoned")?.pending_target_is_b = None;
        Ok(())
    }

    pub(crate) fn reset_runtime(&self) -> Result<(), String> {
        self.reset_repeat()
    }

    fn reset_repeat(&self) -> Result<(), String> {
        *self.repeat.lock().map_err(|_| "Motion repeat lock poisoned")? = MotionRepeatRuntime::default();
        Ok(())
    }

    /// A display preview uses one committed cache snapshot. It never reads the device
    /// or publishes Parameter events. Execution-time reads remain in `run`/Application.
    pub(crate) fn preview_with_parameters(
        &self,
        parameters: &ParameterService,
        effective_speed_limit: Option<f64>,
    ) -> Result<MotionPreview, String> {
        let config = self.get();
        validate_host_config(&config)?;
        let snapshot = parameters.snapshot().map_err(|error| error.to_string())?;
        let value = |symbol: &str| cached_value(parameters, &snapshot, symbol);
        let number = |symbol: &str| -> Result<f64, String> {
            match value(symbol)? {
                ParameterValue::F32(value) if value.is_finite() => Ok(f64::from(value)),
                _ => Err(format!("{symbol} is not a synchronized finite f32 parameter")),
            }
        };
        let position = |symbol: &str| -> Result<PositionValue, String> {
            match value(symbol)? {
                ParameterValue::Position(value) if value.theta.is_finite() => Ok(value),
                _ => Err(format!("{symbol} is not a synchronized position parameter")),
            }
        };
        let mode = match value("PARAM_MOTOR_MODE")? {
            ParameterValue::U8(value) => mode_from_wire_value(parameters.schema(), value)?,
            _ => return Err("Motor mode is not a synchronized u8 parameter".to_owned()),
        };
        let limit = if let Some(limit) = effective_speed_limit {
            Some(limit)
        } else if parameters.schema().parameter_by_key("PARAM_LIMIT_WM_EFFECTIVE").is_some() {
            Some(number("PARAM_LIMIT_WM_EFFECTIVE")?)
        } else {
            None
        };
        match mode {
            MotionMode::Position => {
                let acc = number("PARAM_MOTION_WM_ACC")?;
                let dec = number("PARAM_MOTION_WM_DEC")?;
                let max_speed = number("PARAM_MOTION_WM_MAX")?;
                let distance_turn = match config.position_command {
                    PositionCommand::Incremental => config.incremental_delta_turn,
                    PositionCommand::Absolute => {
                        let current = position_to_turns(position("PARAM_RUN_POSITION")?);
                        let target = position_to_turns(position("PARAM_TARGET_POSITION")?);
                        target - current
                    }
                };
                Ok(position_preview(distance_turn, max_speed, acc, dec, limit))
            }
            MotionMode::Speed | MotionMode::SensorlessSpeed => {
                Ok(speed_preview(number("PARAM_TARGET_SPEED")?, number("PARAM_MOTION_WM_ACC")?, limit))
            }
            MotionMode::Torque => Ok(torque_preview(number("PARAM_TARGET_TORQUE")?)),
        }
    }

    pub(crate) fn run_checked(
        &self,
        parameters: &ParameterService,
        session: &DeviceSession,
        position_target_override: Option<f64>,
        checkpoint: impl Fn() -> Result<(), String>,
    ) -> Result<ActionHandle, String> {
        let config = self.get();
        validate_host_config(&config)?;
        let state_parameter = parameters.schema().parameter_by_key("PARAM_MOTOR_STATE")
            .ok_or_else(|| "connected device does not expose PARAM_MOTOR_STATE".to_owned())?;
        let state = MotorState::from_parameter(state_parameter, read_u8(parameters, "PARAM_MOTOR_STATE")?)?;
        if state != MotorState::Enabled { return Err(format!("motor must be ENABLED before Run; current state is {state}")); }
        let mode = mode_from_wire_value(parameters.schema(), read_u8(parameters, "PARAM_MOTOR_MODE")?)?;
        if mode == MotionMode::Position {
            let target_turns = if let Some(target) = position_target_override {
                Some(target)
            } else if config.position_command == PositionCommand::Incremental {
                let current = position_to_turns(read_position(parameters, "PARAM_RUN_POSITION")?);
                Some(current + config.incremental_delta_turn)
            } else { None };
            if let Some(target) = target_turns {
                checkpoint()?;
                write_by_symbol(parameters, "PARAM_TARGET_POSITION", ParameterValue::Position(turns_to_position(target)?))?;
            }
        }
        checkpoint()?;
        start_action(parameters, session, "ACTION_MOTOR_RUN")
    }
}

fn cached_value(
    parameters: &ParameterService,
    snapshot: &HashMap<u16, Result<ParameterValue, String>>,
    symbol: &str,
) -> Result<ParameterValue, String> {
    let id = parameter_id(parameters, symbol)?;
    snapshot.get(&id).cloned().unwrap_or_else(|| Err(format!("{symbol} has not been read into the shared cache")))
}
fn parameter_id(parameters: &ParameterService, symbol: &str) -> Result<u16, String> {
    parameters.schema().parameter_by_key(symbol).map(|metadata| metadata.id)
        .ok_or_else(|| format!("connected device does not expose {symbol}"))
}
fn action_id(parameters: &ParameterService, symbol: &str) -> Result<u16, String> {
    parameters.schema().action_by_key(symbol).map(|metadata| metadata.id)
        .ok_or_else(|| format!("connected device does not expose {symbol}"))
}
fn write_by_symbol(parameters: &ParameterService, symbol: &str, value: ParameterValue) -> Result<(), String> {
    let id = parameter_id(parameters, symbol)?;
    parameters.write(id, value).map_err(|error| error.to_string())
}
fn read_u8(parameters: &ParameterService, symbol: &str) -> Result<u8, String> {
    let id = parameter_id(parameters, symbol)?;
    match parameters.read(id).map_err(|error| error.to_string())? {
        ParameterValue::U8(value) => Ok(value),
        _ => Err(format!("{symbol} is not a u8 parameter")),
    }
}
fn read_position(parameters: &ParameterService, symbol: &str) -> Result<PositionValue, String> {
    let id = parameter_id(parameters, symbol)?;
    match parameters.read(id).map_err(|error| error.to_string())? {
        ParameterValue::Position(value) => Ok(value),
        _ => Err(format!("{symbol} is not a position parameter")),
    }
}
fn start_action(parameters: &ParameterService, session: &DeviceSession, symbol: &str) -> Result<ActionHandle, String> {
    session.action_start(action_id(parameters, symbol)?).map_err(|error| error.to_string())
}
pub(crate) fn position_to_turns(position: PositionValue) -> f64 {
    f64::from(position.turns) + f64::from(position.theta) / std::f64::consts::TAU
}
pub(crate) fn turns_to_position(turns: f64) -> Result<PositionValue, String> {
    if !turns.is_finite() { return Err("position target must be finite".to_owned()); }
    let whole = turns.floor();
    if whole < f64::from(i32::MIN) || whole > f64::from(i32::MAX) { return Err("position target is outside the supported turn range".to_owned()); }
    Ok(PositionValue { turns: whole as i32, theta: ((turns - whole) * std::f64::consts::TAU) as f32 })
}
pub(crate) fn mode_wire_value(schema: &HostSchema, mode: MotionMode) -> Result<u8, String> {
    let parameter = schema.parameter_by_key("PARAM_MOTOR_MODE")
        .ok_or_else(|| "Motor mode is not exposed".to_owned())?;
    parameter.enum_u8(mode.symbol()).map_err(|error| error.to_string())
}
pub(crate) fn mode_from_wire_value(schema: &HostSchema, value: u8) -> Result<MotionMode, String> {
    let parameter = schema.parameter_by_key("PARAM_MOTOR_MODE")
        .ok_or_else(|| "Motor mode is not exposed".to_owned())?;
    let symbol = parameter.enum_symbol_u8(value)
        .ok_or_else(|| format!("unsupported motor mode value {value}"))?;
    MotionMode::from_symbol(symbol).ok_or_else(|| format!("unsupported motor mode symbol {symbol}"))
}
fn validate_host_config(config: &MotionConfig) -> Result<(), String> {
    if !config.incremental_delta_turn.is_finite() { return Err("Incremental position must be finite".to_owned()); }
    Ok(())
}
fn limited_speed(requested: f64, effective_speed_limit: Option<f64>) -> f64 {
    match effective_speed_limit { Some(limit) if limit.is_finite() && limit >= 0.0 => requested.min(limit), _ => requested }
}
fn position_preview(distance_turn: f64, requested_speed: f64, acceleration: f64, deceleration: f64, effective_speed_limit: Option<f64>) -> MotionPreview {
    let distance = distance_turn.abs() * std::f64::consts::TAU;
    if distance <= f64::EPSILON || acceleration <= 0.0 || deceleration <= 0.0 {
        return MotionPreview { times: vec![0.0, 1.0], primary: vec![0.0, distance_turn], secondary: vec![0.0, 0.0],
            primary_label: "Position", secondary_label: Some("Speed"), primary_unit: "turn", secondary_unit: Some("rad/s") };
    }
    let sign = distance_turn.signum();
    let speed_limit = limited_speed(requested_speed.abs(), effective_speed_limit).max(1e-9);
    let ramp_distance_at_limit = 0.5 * speed_limit * speed_limit * (1.0 / acceleration + 1.0 / deceleration);
    let peak_speed = if ramp_distance_at_limit <= distance { speed_limit } else { (2.0 * distance / (1.0 / acceleration + 1.0 / deceleration)).sqrt() };
    let t_acc = peak_speed / acceleration;
    let t_dec = peak_speed / deceleration;
    let ramp_distance = 0.5 * peak_speed * (t_acc + t_dec);
    let t_cruise = ((distance - ramp_distance).max(0.0)) / peak_speed.max(1e-9);
    let total_duration = t_acc + t_cruise + t_dec;
    let samples = 801usize;
    let dt = total_duration.max(1e-6) / (samples - 1) as f64;
    let mut times = Vec::with_capacity(samples);
    let mut position = Vec::with_capacity(samples);
    let mut speed: Vec<f64> = Vec::with_capacity(samples);
    let mut pos = 0.0;
    for i in 0..samples {
        let t = i as f64 * dt;
        let current_speed = if t < t_acc { acceleration * t } else if t < t_acc + t_cruise { peak_speed }
            else { (peak_speed - deceleration * (t - t_acc - t_cruise)).max(0.0) };
        if i > 0 { pos += 0.5 * (speed[i - 1].abs() + current_speed) * dt; }
        times.push(t); speed.push(sign * current_speed); position.push(sign * pos / std::f64::consts::TAU);
    }
    MotionPreview { times, primary: position, secondary: speed, primary_label: "Position", secondary_label: Some("Speed"), primary_unit: "turn", secondary_unit: Some("rad/s") }
}
fn speed_preview(target: f64, acceleration: f64, effective_speed_limit: Option<f64>) -> MotionPreview {
    let magnitude = limited_speed(target.abs(), effective_speed_limit);
    let sign = target.signum();
    let ramp = if acceleration > 0.0 { magnitude / acceleration } else { 0.0 };
    let hold = 0.5_f64.max(ramp * 0.25);
    let total = ramp + hold;
    let samples = 301usize;
    let mut times = Vec::with_capacity(samples);
    let mut speed = Vec::with_capacity(samples);
    for i in 0..samples {
        let t = i as f64 * total.max(1e-6) / (samples - 1) as f64;
        let value = if ramp <= f64::EPSILON || t >= ramp { magnitude } else { magnitude * t / ramp };
        times.push(t); speed.push(sign * value);
    }
    MotionPreview { times, primary: speed, secondary: Vec::new(), primary_label: "Speed", secondary_label: None, primary_unit: "rad/s", secondary_unit: None }
}
fn torque_preview(target: f64) -> MotionPreview {
    MotionPreview { times: vec![0.0, 0.5], primary: vec![target, target], secondary: Vec::new(),
        primary_label: "Torque", secondary_label: None, primary_unit: "N·m", secondary_unit: None }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn host_config_only_validates_incremental_delta() {
        let service = MotionService::default();
        let mut config = service.get();
        config.incremental_delta_turn = f64::NAN;
        assert!(service.set(config).is_err());
    }
    #[test]
    fn position_round_trip_preserves_turns() {
        for value in [-2.25, -0.1, 0.0, 1.75, 123.125] {
            assert!((position_to_turns(turns_to_position(value).unwrap()) - value).abs() < 1e-6);
        }
    }
    #[test]
    fn speed_preview_respects_cached_effective_limit() {
        let preview = speed_preview(-100.0, 20.0, Some(30.0));
        assert!(preview.primary.iter().all(|value| *value >= -30.0 && *value <= 0.0));
        assert_eq!(preview.primary.last(), Some(&-30.0));
    }

    #[test]
    fn motion_mode_uses_schema_values_not_fixed_ordinals() {
        let schema = HostSchema::parse(r#"
schema_version = 1
protocol = "axdr-canfd-v1"

[source]
repository = "fixture"
git_sha = "abc"
parameter_schema = 1

[[parameters]]
symbol = "PARAM_MOTOR_MODE"
label = "Motor Mode"
id = 1793
type = "u8"
access = "rw"
description = "mode"
allowed = [7, 8, 9, 12]
allowed_symbols = ["TORQUE", "SPEED", "POSITION", "SENSORLESS_SPEED"]
"#).unwrap();

        assert_eq!(mode_wire_value(&schema, MotionMode::Position).unwrap(), 9);
        assert_eq!(mode_from_wire_value(&schema, 12).unwrap(), MotionMode::SensorlessSpeed);
    }

    #[test]
    fn repeat_advances_only_after_successful_leg() {
        let service = MotionService::default();
        let mut config = service.get();
        config.repeat = true;
        config.position_command = PositionCommand::Incremental;
        config.incremental_delta_turn = 1.0;
        service.set(config).unwrap();

        let (target, target_is_b) = service.repeat_target(2.0, None).unwrap().unwrap();
        assert_eq!(target, 3.0);
        assert!(target_is_b);
        service.repeat_mark_started(target_is_b).unwrap();
        service.repeat_completed(AxdrStatus::ErrState).unwrap();

        let (target, target_is_b) = service.repeat_target(2.0, None).unwrap().unwrap();
        assert_eq!(target, 3.0);
        assert!(target_is_b);
        service.repeat_mark_started(target_is_b).unwrap();
        service.repeat_completed(AxdrStatus::Ok).unwrap();

        let (target, target_is_b) = service.repeat_target(3.0, None).unwrap().unwrap();
        assert_eq!(target, 2.0);
        assert!(!target_is_b);

        service.reset_runtime().unwrap();
        assert!(service.get().repeat);
        assert_eq!(service.get().incremental_delta_turn, 1.0);
        let (target, target_is_b) = service.repeat_target(10.0, None).unwrap().unwrap();
        assert_eq!(target, 11.0);
        assert!(target_is_b);
    }
}
