use serde::Serialize;

use crate::HostSchema;

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommissioningCapabilities {
    pub identification_rs_ls: bool,
    pub identification_flux: bool,
    pub identification_jb: bool,
    pub identification_apply: bool,
    pub phase_search: bool,
    pub encoder_calibration: bool,
    pub homing: bool,
    pub position_set_zero: bool,
    pub protection_clear: bool,
}

impl CommissioningCapabilities {
    pub fn from_schema(schema: &HostSchema) -> Self {
        let has_action = |symbol: &str| schema.action_by_key(symbol).is_some();
        let encoder_mode = schema.parameter_by_key("PARAM_MOTOR_MODE")
            .is_some_and(|mode| mode.enum_u8("ENCODER_CAL").is_ok())
            && has_action("ACTION_MOTOR_DISABLE");

        Self {
            identification_rs_ls: has_action("ACTION_IDENT_RS_LS_START"),
            identification_flux: has_action("ACTION_IDENT_FLUX_START"),
            identification_jb: has_action("ACTION_IDENT_JB_START"),
            identification_apply: has_action("ACTION_IDENT_APPLY"),
            phase_search: encoder_mode && has_action("ACTION_ENC_PHASE_START"),
            encoder_calibration: encoder_mode && has_action("ACTION_ENC_CAL_START"),
            homing: has_action("ACTION_HOME_START"),
            position_set_zero: has_action("ACTION_POSITION_SET_ZERO"),
            protection_clear: has_action("ACTION_PROTECTION_CLEAR"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoder_tasks_require_mode_and_their_dedicated_actions() {
        let schema = HostSchema::parse(r#"
protocol = "axdr-canfd"

[source]
repository = "fixture"
git_sha = "abc"

[[parameters]]
symbol = "PARAM_MOTOR_MODE"
label = "Motor Mode"
id = 1793
type = "u8"
access = "rw"
description = "mode"
allowed_symbols = ["TORQUE", "ENCODER_CAL"]

[[actions]]
symbol = "ACTION_MOTOR_DISABLE"
label = "Disable"
id = 4100
description = "disable"

[[actions]]
symbol = "ACTION_ENC_PHASE_START"
label = "Phase Search"
id = 5122
description = "phase search"
"#).unwrap();

        let caps = CommissioningCapabilities::from_schema(&schema);
        assert!(caps.phase_search);
        assert!(!caps.encoder_calibration);
        assert!(!caps.identification_rs_ls);
        assert!(!caps.homing);
    }
}
