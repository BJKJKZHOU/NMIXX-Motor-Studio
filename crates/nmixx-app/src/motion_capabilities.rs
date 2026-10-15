use serde::Serialize;

use crate::HostSchema;

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MotionCapabilities {
    pub position: bool,
    pub speed: bool,
    pub sensorless_speed: bool,
    pub torque: bool,
    pub mit: bool,

    pub trajectory_trapezoidal: bool,
    pub trajectory_s_curve: bool,
    pub trajectory_filtered: bool,

    pub max_speed: bool,
    pub acceleration: bool,
    pub deceleration: bool,
    pub filter_time: bool,

    pub run: bool,
    pub stop: bool,
    pub enable: bool,
    pub disable: bool,

    pub position_target: bool,
    pub speed_target: bool,
    pub torque_target: bool,
    pub torque_ramp: bool,
    pub sensorless_startup_current: bool,
    pub sensorless_entry_speed: bool,
}

impl MotionCapabilities {
    pub fn from_schema(schema: &HostSchema) -> Self {
        let mode = schema.parameter_by_key("PARAM_MOTOR_MODE");

        let has_mode = |symbol: &str| {
            mode.is_some_and(|parameter| parameter.allowed_symbols.iter().any(|item| item == symbol))
        };

        let has_parameter = |symbol: &str| schema.parameter_by_key(symbol).is_some();
        let has_action = |symbol: &str| schema.action_by_key(symbol).is_some();

        let position_target = has_parameter("PARAM_TARGET_POSITION");
        let speed_target = has_parameter("PARAM_TARGET_SPEED");
        let torque_target = has_parameter("PARAM_TARGET_TORQUE");
        let torque_ramp = has_parameter("PARAM_MOTION_TE_RATE");
        let sensorless_startup_current = has_parameter("PARAM_MOTOR_IF_CURRENT");
        let sensorless_entry_speed = has_parameter("PARAM_SENSORLESS_ENTRY_SPEED");

        let acceleration = has_parameter("PARAM_MOTION_WM_ACC");
        let deceleration = has_parameter("PARAM_MOTION_WM_DEC");
        let max_speed = speed_target;

        // Current firmware selects trapezoidal/S-curve profiles through one enum.
        // Discover support from the actual exported profile symbols.
        let profile = schema.parameter_by_key("PARAM_MOTION_PROFILE");
        let trajectory_trapezoidal = acceleration && deceleration;
        let trajectory_s_curve = profile.is_some_and(|parameter|
            parameter.allowed_symbols.iter().any(|value|
                value == "MOTION_S_TIME" || value == "MOTION_S_PEAK"));
        let filter_time = has_parameter("PARAM_MOTION_FILTER_TIME");
        let trajectory_filtered = filter_time;

        Self {
            position: has_mode("POSITION") && position_target,
            speed: has_mode("SPEED") && speed_target,
            sensorless_speed: has_mode("SENSORLESS_SPEED") && speed_target,
            torque: has_mode("TORQUE") && torque_target,
            mit: has_mode("MIT"),

            trajectory_trapezoidal,
            trajectory_s_curve,
            trajectory_filtered,

            max_speed,
            acceleration,
            deceleration,
            filter_time,

            run: has_action("ACTION_MOTOR_RUN"),
            stop: has_action("ACTION_MOTOR_STOP"),
            enable: has_action("ACTION_MOTOR_ENABLE"),
            disable: has_action("ACTION_MOTOR_DISABLE"),

            position_target,
            speed_target,
            torque_target,
            torque_ramp,
            sensorless_startup_current,
            sensorless_entry_speed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovers_firmware_motion_profile_and_shared_speed_target() {
        let schema = HostSchema::parse(
            r#"
schema_version = 1
protocol = "axdr-canfd-v1"

[source]
repository = "AxDr_L_Motor"
git_sha = "abc"
parameter_schema = 1

[[parameters]]
symbol = "PARAM_MOTOR_MODE"
label = "Motor Mode"
id = 1793
type = "u8"
access = "rw"
description = "mode"
allowed_symbols = ["TORQUE", "SPEED", "POSITION", "SENSORLESS_SPEED"]

[[parameters]]
symbol = "PARAM_TARGET_POSITION"
label = "Position Target"
id = 1796
type = "position"
access = "rw"
description = "position"

[[parameters]]
symbol = "PARAM_TARGET_SPEED"
label = "Speed Target"
id = 1795
type = "f32"
access = "rw"
description = "speed"

[[parameters]]
symbol = "PARAM_TARGET_TORQUE"
label = "Torque Target"
id = 1794
type = "f32"
access = "rw"
description = "torque"

[[parameters]]
symbol = "PARAM_MOTION_TE_RATE"
label = "Torque slew rate"
id = 1540
type = "f32"
access = "rw"
description = "torque slew"

[[parameters]]
symbol = "PARAM_MOTOR_IF_CURRENT"
label = "I/F Current"
id = 280
type = "f32"
access = "rw"
description = "startup current"

[[parameters]]
symbol = "PARAM_MOTION_PROFILE"
label = "Motion Profile"
id = 1541
type = "u8"
access = "rw"
description = "profile"
allowed_symbols = ["MOTION_TRAPEZOID", "MOTION_S_TIME", "MOTION_S_PEAK"]

[[parameters]]
symbol = "PARAM_MOTION_WM_ACC"
label = "Acceleration"
id = 1538
type = "f32"
access = "rw"
description = "acc"

[[parameters]]
symbol = "PARAM_MOTION_WM_DEC"
label = "Deceleration"
id = 1539
type = "f32"
access = "rw"
description = "dec"

[[actions]]
symbol = "ACTION_MOTOR_RUN"
label = "Run"
id = 4098
description = "run"

[[actions]]
symbol = "ACTION_MOTOR_STOP"
label = "Stop"
id = 4099
description = "stop"
"#,
        ).unwrap();

        let capabilities = MotionCapabilities::from_schema(&schema);
        assert!(capabilities.position);
        assert!(capabilities.speed);
        assert!(capabilities.sensorless_speed);
        assert!(capabilities.torque);
        assert!(capabilities.trajectory_trapezoidal);
        assert!(capabilities.trajectory_s_curve);
        assert!(capabilities.max_speed);
        assert!(capabilities.torque_ramp);
        assert!(capabilities.sensorless_startup_current);
        assert!(!capabilities.trajectory_filtered);
        assert!(capabilities.run);
        assert!(capabilities.stop);
    }
}
