use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};

use crate::{ActionHandle, DeviceSession, ParameterService, ParameterValue, PositionValue};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MotionMode {
    Position,
    Speed,
    SensorlessSpeed,
    Torque,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PositionCommand {
    Absolute,
    Incremental,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MotionConfig {
    pub position_command: PositionCommand,
    pub incremental_delta_turn: f64,
    pub repeat: bool,
}

impl Default for MotionConfig {
    fn default() -> Self {
        Self {
            position_command: PositionCommand::Incremental,
            incremental_delta_turn: 1.0,
            repeat: false,
        }
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

#[derive(Debug, Clone, Default)]
pub struct MotionService {
    config: Arc<RwLock<MotionConfig>>,
}

impl MotionService {
    pub fn get(&self) -> MotionConfig {
        self.config.read().expect("Motion config lock poisoned").clone()
    }

    pub fn set(&self, config: MotionConfig) -> Result<MotionConfig, String> {
        validate_host_config(&config)?;
        *self.config.write().map_err(|_| "Motion config lock poisoned")? = config.clone();
        Ok(config)
    }

    pub fn preview_with_parameters(
        &self,
        parameters: &ParameterService,
        effective_speed_limit: Option<f64>,
    ) -> Result<MotionPreview, String> {
        let config = self.get();
        validate_host_config(&config)?;
        let mode = mode_from_wire_value(read_u8(parameters, "PARAM_MOTOR_MODE")?)?;
        match mode {
            MotionMode::Position => {
                let acc = read_f32(parameters, "PARAM_MOTION_WM_ACC")?;
                let dec = read_f32(parameters, "PARAM_MOTION_WM_DEC")?;
                let max_speed = read_f32(parameters, "PARAM_MOTION_WM_MAX")?;
                let distance_turn = match config.position_command {
                    PositionCommand::Incremental => config.incremental_delta_turn,
                    PositionCommand::Absolute => {
                        let current = position_to_turns(read_position(parameters, "PARAM_RUN_POSITION")?);
                        let target = position_to_turns(read_position(parameters, "PARAM_TARGET_POSITION")?);
                        target - current
                    }
                };
                Ok(position_preview(distance_turn, max_speed, acc, dec, effective_speed_limit))
            }
            MotionMode::Speed | MotionMode::SensorlessSpeed => {
                let acc = read_f32(parameters, "PARAM_MOTION_WM_ACC")?;
                let target = read_f32(parameters, "PARAM_TARGET_SPEED")?;
                Ok(speed_preview(target, acc, effective_speed_limit))
            }
            MotionMode::Torque => {
                let target = read_f32(parameters, "PARAM_TARGET_TORQUE")?;
                Ok(torque_preview(target))
            }
        }
    }

    pub(crate) fn run(
        &self,
        parameters: &ParameterService,
        session: &DeviceSession,
        position_target_override: Option<f64>,
    ) -> Result<ActionHandle, String> {
        let config = self.get();
        validate_host_config(&config)?;

        let state = read_u8(parameters, "PARAM_MOTOR_STATE")?;
        if state != 1 {
            return Err("motor must be ENABLED before Run".to_owned());
        }

        let mode = mode_from_wire_value(read_u8(parameters, "PARAM_MOTOR_MODE")?)?;
        if mode == MotionMode::Position {
            let target_turns = if let Some(target) = position_target_override {
                Some(target)
            } else if config.position_command == PositionCommand::Incremental {
                let current = position_to_turns(read_position(parameters, "PARAM_RUN_POSITION")?);
                Some(current + config.incremental_delta_turn)
            } else {
                None
            };

            if let Some(target) = target_turns {
                write_by_symbol(
                    parameters,
                    "PARAM_TARGET_POSITION",
                    ParameterValue::Position(turns_to_position(target)?),
                )?;
            }
        }

        start_action(parameters, session, "ACTION_MOTOR_RUN")
    }
}

fn parameter_id(parameters: &ParameterService, symbol: &str) -> Result<u16, String> {
    parameters
        .schema()
        .parameter_by_key(symbol)
        .map(|metadata| metadata.id)
        .ok_or_else(|| format!("connected device does not expose {symbol}"))
}

fn action_id(parameters: &ParameterService, symbol: &str) -> Result<u16, String> {
    parameters
        .schema()
        .action_by_key(symbol)
        .map(|metadata| metadata.id)
        .ok_or_else(|| format!("connected device does not expose {symbol}"))
}

fn write_by_symbol(
    parameters: &ParameterService,
    symbol: &str,
    value: ParameterValue,
) -> Result<(), String> {
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

fn read_f32(parameters: &ParameterService, symbol: &str) -> Result<f64, String> {
    let id = parameter_id(parameters, symbol)?;
    match parameters.read(id).map_err(|error| error.to_string())? {
        ParameterValue::F32(value) => Ok(f64::from(value)),
        _ => Err(format!("{symbol} is not an f32 parameter")),
    }
}

fn read_position(parameters: &ParameterService, symbol: &str) -> Result<PositionValue, String> {
    let id = parameter_id(parameters, symbol)?;
    match parameters.read(id).map_err(|error| error.to_string())? {
        ParameterValue::Position(value) => Ok(value),
        _ => Err(format!("{symbol} is not a position parameter")),
    }
}

fn start_action(
    parameters: &ParameterService,
    session: &DeviceSession,
    symbol: &str,
) -> Result<ActionHandle, String> {
    let id = action_id(parameters, symbol)?;
    session.action_start(id).map_err(|error| error.to_string())
}

pub(crate) fn position_to_turns(position: PositionValue) -> f64 {
    f64::from(position.turns) + f64::from(position.theta) / std::f64::consts::TAU
}

pub(crate) fn turns_to_position(turns: f64) -> Result<PositionValue, String> {
    if !turns.is_finite() {
        return Err("position target must be finite".to_owned());
    }
    let whole = turns.floor();
    if whole < f64::from(i32::MIN) || whole > f64::from(i32::MAX) {
        return Err("position target is outside the supported turn range".to_owned());
    }
    Ok(PositionValue {
        turns: whole as i32,
        theta: ((turns - whole) * std::f64::consts::TAU) as f32,
    })
}

pub(crate) fn mode_wire_value(mode: MotionMode) -> u8 {
    match mode {
        MotionMode::Torque => 0,
        MotionMode::Speed => 1,
        MotionMode::Position => 2,
        MotionMode::SensorlessSpeed => 5,
    }
}

pub(crate) fn mode_from_wire_value(value: u8) -> Result<MotionMode, String> {
    match value {
        0 => Ok(MotionMode::Torque),
        1 => Ok(MotionMode::Speed),
        2 => Ok(MotionMode::Position),
        5 => Ok(MotionMode::SensorlessSpeed),
        other => Err(format!("unsupported motor mode value {other}")),
    }
}

fn validate_host_config(config: &MotionConfig) -> Result<(), String> {
    if !config.incremental_delta_turn.is_finite() {
        return Err("Incremental position must be finite".to_owned());
    }
    Ok(())
}

fn limited_speed(requested: f64, effective_speed_limit: Option<f64>) -> f64 {
    match effective_speed_limit {
        Some(limit) if limit.is_finite() && limit >= 0.0 => requested.min(limit),
        _ => requested,
    }
}

fn position_preview(
    distance_turn: f64,
    requested_speed: f64,
    acceleration: f64,
    deceleration: f64,
    effective_speed_limit: Option<f64>,
) -> MotionPreview {
    let distance = distance_turn.abs() * std::f64::consts::TAU;
    if distance <= f64::EPSILON || acceleration <= 0.0 || deceleration <= 0.0 {
        return MotionPreview {
            times: vec![0.0, 1.0],
            primary: vec![0.0, distance_turn],
            secondary: vec![0.0, 0.0],
            primary_label: "Position",
            secondary_label: Some("Speed"),
            primary_unit: "turn",
            secondary_unit: Some("rad/s"),
        };
    }

    let sign = distance_turn.signum();
    let speed_limit = limited_speed(requested_speed.abs(), effective_speed_limit).max(1e-9);
    let ramp_distance_at_limit =
        0.5 * speed_limit * speed_limit * (1.0 / acceleration + 1.0 / deceleration);
    let peak_speed = if ramp_distance_at_limit <= distance {
        speed_limit
    } else {
        (2.0 * distance / (1.0 / acceleration + 1.0 / deceleration)).sqrt()
    };

    let t_acc = peak_speed / acceleration;
    let t_dec = peak_speed / deceleration;
    let ramp_distance = 0.5 * peak_speed * (t_acc + t_dec);
    let t_cruise = ((distance - ramp_distance).max(0.0)) / peak_speed.max(1e-9);
    let total_duration = t_acc + t_cruise + t_dec;
    let samples = 801usize;
    let dt = total_duration.max(1e-6) / (samples - 1) as f64;

    let mut times = Vec::with_capacity(samples);
    let mut position = Vec::with_capacity(samples);
    let mut speed = Vec::with_capacity(samples);
    let mut pos = 0.0;

    for i in 0..samples {
        let t = i as f64 * dt;
        let current_speed = if t < t_acc {
            acceleration * t
        } else if t < t_acc + t_cruise {
            peak_speed
        } else {
            (peak_speed - deceleration * (t - t_acc - t_cruise)).max(0.0)
        };

        if i > 0 {
            pos += 0.5 * (speed[i - 1].abs() + current_speed) * dt;
        }
        times.push(t);
        speed.push(sign * current_speed);
        position.push(sign * pos / std::f64::consts::TAU);
    }

    MotionPreview {
        times,
        primary: position,
        secondary: speed,
        primary_label: "Position",
        secondary_label: Some("Speed"),
        primary_unit: "turn",
        secondary_unit: Some("rad/s"),
    }
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
        let value = if ramp <= f64::EPSILON || t >= ramp {
            magnitude
        } else {
            magnitude * t / ramp
        };
        times.push(t);
        speed.push(sign * value);
    }

    MotionPreview {
        times,
        primary: speed,
        secondary: Vec::new(),
        primary_label: "Speed",
        secondary_label: None,
        primary_unit: "rad/s",
        secondary_unit: None,
    }
}

fn torque_preview(target: f64) -> MotionPreview {
    MotionPreview {
        times: vec![0.0, 0.5],
        primary: vec![target, target],
        secondary: Vec::new(),
        primary_label: "Torque",
        secondary_label: None,
        primary_unit: "N·m",
        secondary_unit: None,
    }
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
        let values = [-2.25, -0.1, 0.0, 1.75, 123.125];
        for value in values {
            let encoded = turns_to_position(value).unwrap();
            let decoded = position_to_turns(encoded);
            assert!((decoded - value).abs() < 1e-6);
        }
    }
}
