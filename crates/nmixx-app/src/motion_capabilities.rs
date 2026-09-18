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

        let acceleration = has_parameter("PARAM_MOTION_WM_ACC");
        let deceleration = has_parameter("PARAM_MOTION_WM_DEC");
        let max_speed = has_parameter("PARAM_MOTION_WM_MAX");

        // Current AxDr_L represents its implemented position/speed motion
        // planner through Wm_Max/Acc/Dec. More advanced trajectory types must
        // be explicitly advertised by dedicated schema entries; do not infer
        // them merely because the GUI knows how to preview them.
        let trajectory_trapezoidal = acceleration && deceleration;
        let trajectory_s_curve =
            has_parameter("PARAM_MOTION_SCURVE_MODE")
            || has_parameter("PARAM_MOTION_SCURVE_ENABLE");
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
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovers_axdr_like_motion_surface_without_inventing_advanced_profiles() {
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
id = 1793
type = "u8"
access = "rw"
description = "mode"
allowed_symbols = ["TORQUE", "SPEED", "POSITION", "SENSORLESS_SPEED"]

[[parameters]]
symbol = "PARAM_TARGET_POSITION"
id = 1796
type = "position"
access = "rw"
description = "position"

[[parameters]]
symbol = "PARAM_TARGET_SPEED"
id = 1795
type = "f32"
access = "rw"
description = "speed"

[[parameters]]
symbol = "PARAM_TARGET_TORQUE"
id = 1794
type = "f32"
access = "rw"
description = "torque"

[[parameters]]
symbol = "PARAM_MOTION_WM_MAX"
id = 1537
type = "f32"
access = "rw"
description = "max speed"

[[parameters]]
symbol = "PARAM_MOTION_WM_ACC"
id = 1538
type = "f32"
access = "rw"
description = "acc"

[[parameters]]
symbol = "PARAM_MOTION_WM_DEC"
id = 1539
type = "f32"
access = "rw"
description = "dec"

[[actions]]
symbol = "ACTION_MOTOR_RUN"
id = 4098
description = "run"

[[actions]]
symbol = "ACTION_MOTOR_STOP"
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
        assert!(!capabilities.trajectory_s_curve);
        assert!(!capabilities.trajectory_filtered);
        assert!(capabilities.run);
        assert!(capabilities.stop);
    }
}
