use std::thread;
use std::time::{Duration, Instant};

use crate::{
    ActionHandle, ApplicationError, ApplicationSession, MotionMode, ParameterValue, PositionCommand,
    PositionValue,
};

#[derive(Debug, Clone, Copy)]
pub struct SpeedMotionRequest {
    pub target_rad_s: f32,
    pub max_speed_rad_s: Option<f32>,
    pub acceleration_rad_s2: Option<f32>,
    pub deceleration_rad_s2: Option<f32>,
}

#[derive(Debug, Clone, Copy)]
pub struct PositionMotionRequest {
    pub command: PositionCommand,
    /// Absolute target turns when command=Absolute; delta turns when command=Incremental.
    pub target_turn: f64,
    pub max_speed_rad_s: Option<f32>,
    pub acceleration_rad_s2: Option<f32>,
    pub deceleration_rad_s2: Option<f32>,
}

#[derive(Debug, Clone)]
pub struct MotionRuntimeStatus {
    pub motor_state_raw: u8,
    pub motor_state: String,
    pub mode: MotionMode,
    pub speed_target_rad_s: Option<f32>,
    pub speed_ref_rad_s: Option<f32>,
    pub speed_feedback_rad_s: Option<f32>,
    pub encoder_speed_rad_s: Option<f32>,
    pub speed_limit_rad_s: Option<f32>,
    pub position_target: Option<PositionValue>,
    pub position_ref: Option<PositionValue>,
    pub position_feedback: Option<PositionValue>,
    pub encoder_ready: Option<u8>,
    pub encoder_valid: Option<u8>,
    pub encoder_fault: Option<u8>,
}

impl ApplicationSession {
    /// Change the firmware motor mode through the authoritative Parameter path.
    ///
    /// Firmware state rules still apply; AxDr_L only accepts this while DISABLED.
    pub fn motion_set_mode(&self, mode: MotionMode) -> Result<MotionMode, ApplicationError> {
        if !motion_mode_supported(self, mode) {
            return Err(ApplicationError::Motion(format!(
                "connected device does not expose {mode:?} motion"
            )));
        }

        let parameter = self
            .schema()
            .parameter_by_key("PARAM_MOTOR_MODE")
            .ok_or_else(|| ApplicationError::Motion("Motor mode is not exposed".to_owned()))?;
        self.parameter_write(
            parameter.id,
            ParameterValue::U8(crate::motion::mode_wire_value(mode)),
        )?;
        Ok(mode)
    }

    /// Write the speed-command Parameters and start the already-enabled SPEED mode.
    ///
    /// Optional shaping values leave the current firmware Parameter unchanged when omitted.
    pub fn motion_run_speed(
        &self,
        request: SpeedMotionRequest,
    ) -> Result<ActionHandle, ApplicationError> {
        require_finite_f32(request.target_rad_s, "Speed target")?;
        require_optional_positive(request.max_speed_rad_s, "Max speed")?;
        require_optional_positive(request.acceleration_rad_s2, "Acceleration")?;
        require_optional_positive(request.deceleration_rad_s2, "Deceleration")?;
        self.require_motion_mode(MotionMode::Speed)?;

        self.write_motion_f32("PARAM_TARGET_SPEED", request.target_rad_s)?;
        self.write_optional_motion_f32("PARAM_MOTION_WM_MAX", request.max_speed_rad_s)?;
        self.write_optional_motion_f32("PARAM_MOTION_WM_ACC", request.acceleration_rad_s2)?;
        self.write_optional_motion_f32("PARAM_MOTION_WM_DEC", request.deceleration_rad_s2)?;

        self.motion_run()
    }

    /// Write the position-command Parameters and start the already-enabled POSITION mode.
    pub fn motion_run_position(
        &self,
        request: PositionMotionRequest,
    ) -> Result<ActionHandle, ApplicationError> {
        if !request.target_turn.is_finite() {
            return Err(ApplicationError::Motion(
                "Position target must be finite".to_owned(),
            ));
        }
        require_optional_positive(request.max_speed_rad_s, "Max speed")?;
        require_optional_positive(request.acceleration_rad_s2, "Acceleration")?;
        require_optional_positive(request.deceleration_rad_s2, "Deceleration")?;
        self.require_motion_mode(MotionMode::Position)?;

        self.write_optional_motion_f32("PARAM_MOTION_WM_MAX", request.max_speed_rad_s)?;
        self.write_optional_motion_f32("PARAM_MOTION_WM_ACC", request.acceleration_rad_s2)?;
        self.write_optional_motion_f32("PARAM_MOTION_WM_DEC", request.deceleration_rad_s2)?;

        let mut config = self.motion_get();
        config.position_command = request.command;
        config.repeat = false;

        match request.command {
            PositionCommand::Absolute => {
                self.write_motion_value(
                    "PARAM_TARGET_POSITION",
                    ParameterValue::Position(turns_to_position(request.target_turn)?),
                )?;
            }
            PositionCommand::Incremental => {
                config.incremental_delta_turn = request.target_turn;
            }
        }

        self.motion_set(config)?;
        self.motion_run()
    }

    /// Read the small runtime set needed for headless sensored speed/position debugging.
    pub fn motion_status(&self) -> Result<MotionRuntimeStatus, ApplicationError> {
        let motor_state_raw = self.read_required_u8("PARAM_MOTOR_STATE")?;
        let motor_state = self.enum_symbol("PARAM_MOTOR_STATE", motor_state_raw)?;
        let mode_raw = self.read_required_u8("PARAM_MOTOR_MODE")?;
        let mode =
            crate::motion::mode_from_wire_value(mode_raw).map_err(ApplicationError::Motion)?;

        Ok(MotionRuntimeStatus {
            motor_state_raw,
            motor_state,
            mode,
            speed_target_rad_s: self.read_optional_f32("PARAM_TARGET_SPEED")?,
            speed_ref_rad_s: self.read_optional_f32("PARAM_REF_WM")?,
            speed_feedback_rad_s: self.read_optional_f32("PARAM_RUN_WM")?,
            encoder_speed_rad_s: self.read_optional_f32("PARAM_ENCODER_WM")?,
            speed_limit_rad_s: self.read_optional_f32("PARAM_LIMIT_WM_EFFECTIVE")?,
            position_target: self.read_optional_position("PARAM_TARGET_POSITION")?,
            position_ref: self.read_optional_position("PARAM_REF_POSITION")?,
            position_feedback: self.read_optional_position("PARAM_RUN_POSITION")?,
            encoder_ready: self.read_optional_u8("PARAM_ENCODER_READY")?,
            encoder_valid: self.read_optional_u8("PARAM_ENCODER_VALID")?,
            encoder_fault: self.read_optional_u8("PARAM_ENCODER_FAULT")?,
        })
    }

    /// Wait for a controlled Stop to leave RUN without relying on ACTION_COMPLETE.
    ///
    /// AxDr_L acknowledges ACTION_MOTOR_STOP immediately, then remains RUN while
    /// the configured deceleration reaches zero.
    pub fn motor_wait_stopped(
        &self,
        timeout: Duration,
    ) -> Result<MotionRuntimeStatus, ApplicationError> {
        let deadline = Instant::now() + timeout;

        loop {
            let state = self.read_required_u8("PARAM_MOTOR_STATE")?;
            let symbol = self.enum_symbol("PARAM_MOTOR_STATE", state)?;
            if symbol != "RUN" {
                return self.motion_status();
            }

            if Instant::now() >= deadline {
                return Err(ApplicationError::Motion(format!(
                    "motor remained RUN for {:.3} s after Stop",
                    timeout.as_secs_f64()
                )));
            }

            thread::sleep(Duration::from_millis(20));
        }
    }

    fn require_motion_mode(&self, expected: MotionMode) -> Result<(), ApplicationError> {
        let raw = self.read_required_u8("PARAM_MOTOR_MODE")?;
        let current =
            crate::motion::mode_from_wire_value(raw).map_err(ApplicationError::Motion)?;
        if current != expected {
            return Err(ApplicationError::Motion(format!(
                "motion command requires {expected:?} mode, current mode is {current:?}; change mode while DISABLED first"
            )));
        }
        Ok(())
    }

    fn write_optional_motion_f32(
        &self,
        key: &str,
        value: Option<f32>,
    ) -> Result<(), ApplicationError> {
        if let Some(value) = value {
            self.write_motion_f32(key, value)?;
        }
        Ok(())
    }

    fn write_motion_f32(&self, key: &str, value: f32) -> Result<(), ApplicationError> {
        require_finite_f32(value, key)?;
        self.write_motion_value(key, ParameterValue::F32(value))
    }

    fn write_motion_value(
        &self,
        key: &str,
        value: ParameterValue,
    ) -> Result<(), ApplicationError> {
        let parameter = self
            .schema()
            .parameter_by_key(key)
            .ok_or_else(|| ApplicationError::Motion(format!("{key} is not exposed")))?;
        self.parameter_write(parameter.id, value)?;
        Ok(())
    }

    fn read_required_u8(&self, key: &str) -> Result<u8, ApplicationError> {
        let parameter = self
            .schema()
            .parameter_by_key(key)
            .ok_or_else(|| ApplicationError::Motion(format!("{key} is not exposed")))?;
        match self.parameter_read(parameter.id)? {
            ParameterValue::U8(value) => Ok(value),
            _ => Err(ApplicationError::Motion(format!(
                "{key} has an unexpected type"
            ))),
        }
    }

    fn read_optional_u8(&self, key: &str) -> Result<Option<u8>, ApplicationError> {
        let Some(parameter) = self.schema().parameter_by_key(key) else {
            return Ok(None);
        };
        match self.parameter_read(parameter.id)? {
            ParameterValue::U8(value) => Ok(Some(value)),
            _ => Err(ApplicationError::Motion(format!(
                "{key} has an unexpected type"
            ))),
        }
    }

    fn read_optional_f32(&self, key: &str) -> Result<Option<f32>, ApplicationError> {
        let Some(parameter) = self.schema().parameter_by_key(key) else {
            return Ok(None);
        };
        match self.parameter_read(parameter.id)? {
            ParameterValue::F32(value) => Ok(Some(value)),
            _ => Err(ApplicationError::Motion(format!(
                "{key} has an unexpected type"
            ))),
        }
    }

    fn read_optional_position(
        &self,
        key: &str,
    ) -> Result<Option<PositionValue>, ApplicationError> {
        let Some(parameter) = self.schema().parameter_by_key(key) else {
            return Ok(None);
        };
        match self.parameter_read(parameter.id)? {
            ParameterValue::Position(value) => Ok(Some(value)),
            _ => Err(ApplicationError::Motion(format!(
                "{key} has an unexpected type"
            ))),
        }
    }

    fn enum_symbol(&self, key: &str, raw: u8) -> Result<String, ApplicationError> {
        let parameter = self
            .schema()
            .parameter_by_key(key)
            .ok_or_else(|| ApplicationError::Motion(format!("{key} is not exposed")))?;
        Ok(parameter
            .allowed_symbols
            .get(raw as usize)
            .cloned()
            .unwrap_or_else(|| raw.to_string()))
    }
}

fn motion_mode_supported(app: &ApplicationSession, mode: MotionMode) -> bool {
    let capabilities = app.motion_capabilities();
    match mode {
        MotionMode::Position => capabilities.position,
        MotionMode::Speed => capabilities.speed,
        MotionMode::SensorlessSpeed => capabilities.sensorless_speed,
        MotionMode::Torque => capabilities.torque,
    }
}

fn require_finite_f32(value: f32, name: &str) -> Result<(), ApplicationError> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(ApplicationError::Motion(format!("{name} must be finite")))
    }
}

fn require_optional_positive(
    value: Option<f32>,
    name: &str,
) -> Result<(), ApplicationError> {
    let Some(value) = value else {
        return Ok(());
    };
    if !value.is_finite() || value <= 0.0 {
        return Err(ApplicationError::Motion(format!(
            "{name} must be positive and finite"
        )));
    }
    Ok(())
}

fn turns_to_position(turns: f64) -> Result<PositionValue, ApplicationError> {
    let whole = turns.floor();
    if whole < f64::from(i32::MIN) || whole > f64::from(i32::MAX) {
        return Err(ApplicationError::Motion(
            "Position target is outside the supported turn range".to_owned(),
        ));
    }

    Ok(PositionValue {
        turns: whole as i32,
        theta: ((turns - whole) * std::f64::consts::TAU) as f32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absolute_position_conversion_keeps_negative_fraction_canonical() {
        let value = turns_to_position(-0.25).unwrap();
        assert_eq!(value.turns, -1);
        assert!((f64::from(value.theta) - 0.75 * std::f64::consts::TAU).abs() < 1.0e-6);
    }

    #[test]
    fn optional_motion_limits_reject_nonpositive_values() {
        assert!(require_optional_positive(None, "Acceleration").is_ok());
        assert!(require_optional_positive(Some(1.0), "Acceleration").is_ok());
        assert!(require_optional_positive(Some(0.0), "Acceleration").is_err());
        assert!(require_optional_positive(Some(f32::NAN), "Acceleration").is_err());
    }
}
