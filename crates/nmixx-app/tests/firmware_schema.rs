use nmixx_app::{CommissioningCapabilities, HostSchema, MotionCapabilities, SchemaError};

// Unmodified export from AxDr_L_Motor, the authoritative Host contract.
const FIRMWARE_SCHEMA: &str = include_str!("fixtures/axdr-host-schema.toml");

#[test]
fn loads_firmware_export_and_discovers_its_capabilities() {
    let schema = HostSchema::parse(FIRMWARE_SCHEMA).unwrap();
    assert_eq!(schema.protocol, "axdr-canfd");
    assert_eq!(schema.source.repository, "AxDr_L_Motor");
    assert_eq!(schema.parameters.len(), 121);
    assert_eq!(schema.actions.len(), 14);
    for parameter in &schema.parameters { parameter.parameter_type().unwrap(); }
    let controller = schema.parameter_by_symbol("PARAM_CTRL_CURRENT_CONTROLLER").unwrap();
    assert_eq!(controller.id, 0x0a08);
    assert_eq!(controller.enum_u8("CURRENT_CTRL_DPCC").unwrap(), 2);
    let motion = MotionCapabilities::from_schema(&schema);
    assert!(motion.position && motion.speed && motion.sensorless_speed && motion.torque);
    assert!(motion.trajectory_s_curve);
    let commissioning = CommissioningCapabilities::from_schema(&schema);
    assert!(commissioning.identification_rs_ls && commissioning.identification_flux
        && commissioning.identification_jb && commissioning.phase_search && commissioning.encoder_calibration);
    assert!(!commissioning.homing && !commissioning.position_set_zero);
}

#[test]
fn rejects_unrelated_protocols() {
    assert!(matches!(HostSchema::parse(&FIRMWARE_SCHEMA.replace("axdr-canfd", "unrelated-protocol")),
        Err(SchemaError::UnsupportedProtocol(_))));
}
