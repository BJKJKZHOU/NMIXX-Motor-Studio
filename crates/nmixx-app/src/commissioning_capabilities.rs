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
    pub homing: bool,
    pub position_set_zero: bool,
}

impl CommissioningCapabilities {
    pub fn from_schema(schema: &HostSchema) -> Self {
        let has_action = |symbol: &str| schema.action_by_key(symbol).is_some();
        let phase_search = schema
            .parameter_by_key("PARAM_MOTOR_MODE")
            .is_some_and(|mode| mode.enum_u8("PHASE_SEARCH").is_ok())
            && has_action("ACTION_MOTOR_ENABLE")
            && has_action("ACTION_MOTOR_RUN");

        Self {
            identification_rs_ls: has_action("ACTION_IDENT_RS_LS_START"),
            identification_flux: has_action("ACTION_IDENT_FLUX_START"),
            identification_jb: has_action("ACTION_IDENT_JB_START"),
            identification_apply: has_action("ACTION_IDENT_APPLY"),
            phase_search,
            homing: has_action("ACTION_HOME_START"),
            position_set_zero: has_action("ACTION_POSITION_SET_ZERO"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_search_requires_the_semantic_composition() {
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
allowed_symbols = ["TORQUE", "PHASE_SEARCH"]

[[actions]]
symbol = "ACTION_MOTOR_ENABLE"
label = "Enable"
id = 4097
description = "enable"

[[actions]]
symbol = "ACTION_MOTOR_RUN"
label = "Run"
id = 4098
description = "run"
"#).unwrap();

        let caps = CommissioningCapabilities::from_schema(&schema);
        assert!(caps.phase_search);
        assert!(!caps.identification_rs_ls);
        assert!(!caps.homing);
    }
}
