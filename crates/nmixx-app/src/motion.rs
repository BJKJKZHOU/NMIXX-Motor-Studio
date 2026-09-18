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
    Mit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TrajectoryType {
    Trapezoidal,
    SCurve,
    Filtered,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SCurveMode {
    PeakAccel,
    MatchedTime,
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
    pub mode: MotionMode,
    pub trajectory: TrajectoryType,
    pub acceleration: f64,
    pub deceleration: f64,
    pub filter_time_ms: f64,
    pub s_curve_mode: SCurveMode,
    pub repeat: bool,

    pub position_command: PositionCommand,
    pub position_target_turn: f64,
    pub position_max_speed: f64,

    pub speed_target: f64,
    pub sensorless_speed_target: f64,
    pub sensorless_startup_current: f64,
    pub sensorless_entry_speed: f64,

    pub torque_target_nm: f64,
    pub torque_ramp_nm_per_s: f64,

    pub mit_position_ref: f64,
    pub mit_velocity_ref: f64,
    pub mit_kp: f64,
    pub mit_kd: f64,
    pub mit_torque_feedforward: f64,
}

impl Default for MotionConfig {
    fn default() -> Self {
        Self {
            mode: MotionMode::Position,
            trajectory: TrajectoryType::Trapezoidal,
            acceleration: 20.0,
            deceleration: 20.0,
            filter_time_ms: 20.0,
            s_curve_mode: SCurveMode::PeakAccel,
            repeat: false,
            position_command: PositionCommand::Incremental,
            position_target_turn: 1.0,
            position_max_speed: 8.0,
            speed_target: 20.0,
            sensorless_speed_target: 20.0,
            sensorless_startup_current: 1.0,
            sensorless_entry_speed: 8.0,
            torque_target_nm: 0.2,
            torque_ramp_nm_per_s: 1.0,
            mit_position_ref: 0.0,
            mit_velocity_ref: 0.0,
            mit_kp: 10.0,
            mit_kd: 0.5,
            mit_torque_feedforward: 0.0,
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
        validate(&config)?;
        *self.config.write().map_err(|_| "Motion config lock poisoned")? = config.clone();
        Ok(config)
    }

    pub fn preview(&self) -> Result<MotionPreview, String> {
        let config = self.get();
        validate(&config)?;
        Ok(generate_preview(&config))
    }

    pub fn run(
        &self,
        parameters: &ParameterService,
        session: &DeviceSession,
    ) -> Result<ActionHandle, String> {
        let config = self.get();
        validate(&config)?;

        if config.trajectory != TrajectoryType::Trapezoidal
            && matches!(config.mode, MotionMode::Position | MotionMode::Speed | MotionMode::SensorlessSpeed)
        {
            return Err("selected trajectory is not executable on the connected device".to_owned());
        }

        let state = read_u8(parameters, "PARAM_MOTOR_STATE")?;
        if state != 0 && state != 1 {
            return Err("motor must be DISABLED or ENABLED before Run".to_owned());
        }

        let desired_mode = mode_wire_value(config.mode)?;

        if state == 0 {
            write_by_symbol(parameters, "PARAM_MOTOR_MODE", ParameterValue::U8(desired_mode))?;
        } else {
            let active_mode = read_u8(parameters, "PARAM_MOTOR_MODE")?;
            if active_mode != desired_mode {
                return Err("changing Motion mode requires the motor to be DISABLED".to_owned());
            }
        }

        match config.mode {
            MotionMode::Position => {
                write_by_symbol(
                    parameters,
                    "PARAM_MOTION_WM_MAX",
                    ParameterValue::F32(config.position_max_speed as f32),
                )?;
                write_motion_limits(parameters, &config)?;

                let target_turns = match config.position_command {
                    PositionCommand::Absolute => config.position_target_turn,
                    PositionCommand::Incremental => {
                        let current = read_position(parameters, "PARAM_RUN_POSITION")?;
                        position_to_turns(current) + config.position_target_turn
                    }
                };
                write_by_symbol(
                    parameters,
                    "PARAM_TARGET_POSITION",
                    ParameterValue::Position(turns_to_position(target_turns)?),
                )?;
            }
            MotionMode::Speed => {
                write_motion_limits(parameters, &config)?;
                write_by_symbol(
                    parameters,
                    "PARAM_TARGET_SPEED",
                    ParameterValue::F32(config.speed_target as f32),
                )?;
            }
            MotionMode::SensorlessSpeed => {
                write_motion_limits(parameters, &config)?;
                write_by_symbol(
                    parameters,
                    "PARAM_TARGET_SPEED",
                    ParameterValue::F32(config.sensorless_speed_target as f32),
                )?;
            }
            MotionMode::Torque => {
                write_by_symbol(
                    parameters,
                    "PARAM_TARGET_TORQUE",
                    ParameterValue::F32(config.torque_target_nm as f32),
                )?;
            }
            MotionMode::Mit => {
                return Err("MIT is not supported by the connected device".to_owned());
            }
        }

        if state == 0 {
            start_action(parameters, session, "ACTION_MOTOR_ENABLE")?;
        }

        start_action(parameters, session, "ACTION_MOTOR_RUN")
    }

    pub fn stop(
        &self,
        parameters: &ParameterService,
        session: &DeviceSession,
    ) -> Result<ActionHandle, String> {
        start_action(parameters, session, "ACTION_MOTOR_STOP")
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

fn write_motion_limits(parameters: &ParameterService, config: &MotionConfig) -> Result<(), String> {
    write_by_symbol(
        parameters,
        "PARAM_MOTION_WM_ACC",
        ParameterValue::F32(config.acceleration as f32),
    )?;
    write_by_symbol(
        parameters,
        "PARAM_MOTION_WM_DEC",
        ParameterValue::F32(config.deceleration as f32),
    )
}

fn position_to_turns(position: PositionValue) -> f64 {
    f64::from(position.turns) + f64::from(position.theta) / std::f64::consts::TAU
}

fn turns_to_position(turns: f64) -> Result<PositionValue, String> {
    if !turns.is_finite() {
        return Err("position target must be finite".to_owned());
    }

    let whole = turns.floor();
    if whole < f64::from(i32::MIN) || whole > f64::from(i32::MAX) {
        return Err("position target is outside the supported turn range".to_owned());
    }

    let theta = ((turns - whole) * std::f64::consts::TAU) as f32;
    Ok(PositionValue {
        turns: whole as i32,
        theta,
    })
}

// Current AxDr_L Motor_Mode_e values. Host Schema already exports the symbols,
// but schema v1 does not yet export symbol -> numeric enum values. Keep this
// compatibility mapping isolated here so schema enum metadata can replace it
// without changing the Motion API or GUI.
fn mode_wire_value(mode: MotionMode) -> Result<u8, String> {
    match mode {
        MotionMode::Torque => Ok(0),
        MotionMode::Speed => Ok(1),
        MotionMode::Position => Ok(2),
        MotionMode::SensorlessSpeed => Ok(5),
        MotionMode::Mit => Err("MIT has no AxDr_L Motor Mode value".to_owned()),
    }
}

fn validate(config: &MotionConfig) -> Result<(), String> {
    let finite = [
        config.acceleration,
        config.deceleration,
        config.filter_time_ms,
        config.position_target_turn,
        config.position_max_speed,
        config.speed_target,
        config.sensorless_speed_target,
        config.sensorless_startup_current,
        config.sensorless_entry_speed,
        config.torque_target_nm,
        config.torque_ramp_nm_per_s,
        config.mit_position_ref,
        config.mit_velocity_ref,
        config.mit_kp,
        config.mit_kd,
        config.mit_torque_feedforward,
    ];
    if finite.iter().any(|value| !value.is_finite()) {
        return Err("Motion values must be finite".to_owned());
    }
    if config.acceleration <= 0.0 || config.deceleration <= 0.0 {
        return Err("Acceleration and deceleration must be positive".to_owned());
    }
    if config.position_max_speed <= 0.0 {
        return Err("Position max speed must be positive".to_owned());
    }
    if config.filter_time_ms < 0.0 {
        return Err("Filter time cannot be negative".to_owned());
    }
    if config.torque_ramp_nm_per_s <= 0.0 {
        return Err("Torque ramp must be positive".to_owned());
    }
    Ok(())
}

fn s_time_factor(config: &MotionConfig) -> f64 {
    if config.trajectory == TrajectoryType::SCurve && config.s_curve_mode == SCurveMode::PeakAccel {
        1.5
    } else {
        1.0
    }
}

fn smoothstep(x: f64) -> f64 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

fn ramp_fraction(config: &MotionConfig, x: f64) -> f64 {
    match config.trajectory {
        TrajectoryType::SCurve => smoothstep(x),
        _ => x.clamp(0.0, 1.0),
    }
}

fn generate_preview(config: &MotionConfig) -> MotionPreview {
    match config.mode {
        MotionMode::Position => position_preview(config),
        MotionMode::Speed | MotionMode::SensorlessSpeed => speed_preview(config),
        MotionMode::Torque => torque_preview(config),
        MotionMode::Mit => MotionPreview {
            times: Vec::new(),
            primary: Vec::new(),
            secondary: Vec::new(),
            primary_label: "Command",
            secondary_label: None,
            primary_unit: "",
            secondary_unit: None,
        },
    }
}

fn position_preview(config: &MotionConfig) -> MotionPreview {
    let distance = config.position_target_turn.abs() * std::f64::consts::TAU;
    if distance <= f64::EPSILON {
        return MotionPreview {
            times: vec![0.0, 1.0],
            primary: vec![0.0, 0.0],
            secondary: vec![0.0, 0.0],
            primary_label: "Position",
            secondary_label: Some("Speed"),
            primary_unit: "turn",
            secondary_unit: Some("rad/s"),
        };
    }

    let sign = config.position_target_turn.signum();
    let a = config.acceleration;
    let d = config.deceleration;
    let factor = s_time_factor(config);
    let speed_limit = config.position_max_speed;
    let ramp_distance_at_limit = 0.5 * factor * speed_limit * speed_limit * (1.0 / a + 1.0 / d);
    let peak_speed = if ramp_distance_at_limit <= distance {
        speed_limit
    } else {
        (2.0 * distance / (factor * (1.0 / a + 1.0 / d))).sqrt()
    };

    let t_acc = factor * peak_speed / a;
    let t_dec = factor * peak_speed / d;
    let ramp_distance = 0.5 * peak_speed * (t_acc + t_dec);
    let t_cruise = ((distance - ramp_distance).max(0.0)) / peak_speed.max(1e-9);
    let base_duration = t_acc + t_cruise + t_dec;
    let tau = if config.trajectory == TrajectoryType::Filtered {
        config.filter_time_ms / 1000.0
    } else {
        0.0
    };
    let total_duration = base_duration + if tau > 0.0 { 8.0 * tau } else { 0.0 };
    let samples = 801usize;
    let dt = total_duration.max(1e-6) / (samples - 1) as f64;

    let mut times = Vec::with_capacity(samples);
    let mut position = Vec::with_capacity(samples);
    let mut speed = Vec::with_capacity(samples);
    let mut pos = 0.0;
    let mut filtered_speed = 0.0;

    for i in 0..samples {
        let t = i as f64 * dt;
        let desired = if t < t_acc {
            peak_speed * ramp_fraction(config, t / t_acc.max(1e-9))
        } else if t < t_acc + t_cruise {
            peak_speed
        } else if t < base_duration {
            let x = (t - t_acc - t_cruise) / t_dec.max(1e-9);
            peak_speed * (1.0 - ramp_fraction(config, x))
        } else {
            0.0
        };

        let current_speed = if tau > 0.0 {
            let alpha = 1.0 - (-dt / tau).exp();
            filtered_speed += alpha * (desired - filtered_speed);
            filtered_speed
        } else {
            desired
        };

        if i > 0 {
            pos += 0.5 * (speed[i - 1].abs() + current_speed.abs()) * dt;
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

fn speed_preview(config: &MotionConfig) -> MotionPreview {
    let target = match config.mode {
        MotionMode::SensorlessSpeed => config.sensorless_speed_target,
        _ => config.speed_target,
    };
    let magnitude = target.abs();
    let sign = target.signum();
    let factor = s_time_factor(config);
    let base_ramp = factor * magnitude / config.acceleration;
    let tau = if config.trajectory == TrajectoryType::Filtered {
        config.filter_time_ms / 1000.0
    } else {
        0.0
    };
    let hold = 0.5_f64.max(base_ramp * 0.25).max(if tau > 0.0 { 6.0 * tau } else { 0.0 });
    let total_duration = base_ramp + hold;
    let samples = 301usize;
    let dt = total_duration.max(1e-6) / (samples - 1) as f64;
    let mut times = Vec::with_capacity(samples);
    let mut speed = Vec::with_capacity(samples);
    let mut filtered = 0.0;

    for i in 0..samples {
        let t = i as f64 * dt;
        let desired = if base_ramp <= f64::EPSILON || t >= base_ramp {
            magnitude
        } else {
            magnitude * ramp_fraction(config, t / base_ramp)
        };
        let value = if tau > 0.0 {
            let alpha = 1.0 - (-dt / tau).exp();
            filtered += alpha * (desired - filtered);
            filtered
        } else {
            desired
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

fn torque_preview(config: &MotionConfig) -> MotionPreview {
    let target = config.torque_target_nm;
    let ramp = target.abs() / config.torque_ramp_nm_per_s;
    let total = ramp + 0.5;
    let samples = 201usize;
    let mut times = Vec::with_capacity(samples);
    let mut torque = Vec::with_capacity(samples);
    for i in 0..samples {
        let t = i as f64 * total / (samples - 1) as f64;
        let value = if ramp <= f64::EPSILON || t >= ramp {
            target
        } else {
            target * t / ramp
        };
        times.push(t);
        torque.push(value);
    }
    MotionPreview {
        times,
        primary: torque,
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
    fn s_curve_peak_accel_takes_longer_than_matched_time() {
        let service = MotionService::default();
        let mut config = service.get();
        config.mode = MotionMode::Speed;
        config.speed_target = 20.0;
        config.trajectory = TrajectoryType::SCurve;

        config.s_curve_mode = SCurveMode::MatchedTime;
        service.set(config.clone()).unwrap();
        let matched = service.preview().unwrap();

        config.s_curve_mode = SCurveMode::PeakAccel;
        service.set(config).unwrap();
        let peak = service.preview().unwrap();

        assert!(peak.times.last().unwrap() > matched.times.last().unwrap());
    }

    #[test]
    fn position_preview_reaches_target() {
        let service = MotionService::default();
        let preview = service.preview().unwrap();
        assert!((preview.primary.last().unwrap() - 1.0).abs() < 2e-3);
    }

    #[test]
    fn filtered_position_preserves_integrated_position_and_settles_speed() {
        let service = MotionService::default();
        let mut config = service.get();
        config.trajectory = TrajectoryType::Filtered;
        config.filter_time_ms = 80.0;
        service.set(config).unwrap();

        let preview = service.preview().unwrap();
        let final_position = *preview.primary.last().unwrap();
        let final_speed = *preview.secondary.last().unwrap();

        assert!((final_position - 1.0).abs() < 5e-3);
        assert!(final_speed.abs() < 1e-2);
    }

    #[test]
    fn filtered_speed_converges_to_target() {
        let service = MotionService::default();
        let mut config = service.get();
        config.mode = MotionMode::Speed;
        config.trajectory = TrajectoryType::Filtered;
        config.filter_time_ms = 80.0;
        config.speed_target = 20.0;
        service.set(config).unwrap();

        let preview = service.preview().unwrap();
        assert!((preview.primary.last().unwrap() - 20.0).abs() < 1e-2);
    }
}
