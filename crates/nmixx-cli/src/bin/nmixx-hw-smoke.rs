use std::error::Error;

use clap::Parser;
use nmixx_app::{DEFAULT_USB_BAUD, DeviceSession, ParameterType, ParameterValue};

/// Current AxDr_L read-only Parameters used only for first hardware bring-up.
/// Keep this smoke binary deliberately small; normal product code resolves
/// Parameters through HostSchema instead of duplicating IDs.
const PARAM_ADC_VBUS: u16 = 0x0004;
const PARAM_MOTOR_STATE: u16 = 0x0710;

#[derive(Debug, Parser)]
#[command(
    name = "nmixx-hw-smoke",
    about = "Minimal NMIXX Motor Studio ↔ AxDr_L USB CDC smoke test"
)]
struct Args {
    /// AxDr_L USB CDC serial port, e.g. /dev/ttyACM0 or COM7.
    #[arg(long)]
    port: String,

    /// Serial line rate. USB CDC ACM normally ignores this, but the host API requires one.
    #[arg(long, default_value_t = DEFAULT_USB_BAUD)]
    baud: u32,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("FAIL: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();

    println!("opening {} @ {}", args.port, args.baud);
    let session = DeviceSession::open_usb(&args.port, args.baud)?;

    let state = session.parameter_read(PARAM_MOTOR_STATE, ParameterType::U8)?;
    let ParameterValue::U8(state) = state else {
        return Err("PARAM_MOTOR_STATE returned an unexpected type".into());
    };
    println!("PARAM_MOTOR_STATE (0x{PARAM_MOTOR_STATE:04X}) = {state}");

    let vbus = session.parameter_read(PARAM_ADC_VBUS, ParameterType::F32)?;
    let ParameterValue::F32(vbus) = vbus else {
        return Err("PARAM_ADC_VBUS returned an unexpected type".into());
    };
    println!("PARAM_ADC_VBUS  (0x{PARAM_ADC_VBUS:04X}) = {vbus:.3} V");

    println!("PASS: USB CDC, AXDR framing, transaction matching, and Parameter READ are working");
    Ok(())
}
