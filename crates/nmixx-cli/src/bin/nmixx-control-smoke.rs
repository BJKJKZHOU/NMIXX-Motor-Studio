use std::error::Error;
use std::time::Duration;

use clap::Parser;
use nmixx_app::{
    AxdrStatus, DEFAULT_USB_BAUD, DeviceSession, ParameterType, ParameterValue, SessionEvent,
};

/// Current AxDr_L IDs used only by this hardware bring-up smoke test.
const PARAM_MOTOR_PP: u16 = 0x0101;
const PARAM_MOTOR_STATE: u16 = 0x0710;
const ACTION_PROTECTION_CLEAR: u16 = 0x1201;
const MOTOR_STATE_DISABLED: u8 = 0;

#[derive(Debug, Parser)]
#[command(
    name = "nmixx-control-smoke",
    about = "Safe Parameter WRITE and Action lifecycle smoke test for AxDr_L"
)]
struct Args {
    /// AxDr_L USB CDC serial port, e.g. /dev/ttyACM0 or COM7.
    #[arg(long)]
    port: String,

    /// Serial line rate. USB CDC ACM normally ignores this, but the host API requires one.
    #[arg(long, default_value_t = DEFAULT_USB_BAUD)]
    baud: u32,

    /// Maximum time to wait for asynchronous Action completion.
    #[arg(long, default_value_t = 2000)]
    action_timeout_ms: u64,
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
    let events = session.subscribe()?;

    let state = read_u8(&session, PARAM_MOTOR_STATE)?;
    println!("PARAM_MOTOR_STATE (0x{PARAM_MOTOR_STATE:04X}) = {state}");
    if state != MOTOR_STATE_DISABLED {
        return Err(format!(
            "motor must be DISABLED ({MOTOR_STATE_DISABLED}) before this smoke test, got {state}"
        )
        .into());
    }

    let original_pp = read_u8(&session, PARAM_MOTOR_PP)?;
    let test_pp = if original_pp == u8::MAX {
        original_pp - 1
    } else {
        original_pp + 1
    };
    println!(
        "Parameter WRITE: PARAM_MOTOR_PP 0x{PARAM_MOTOR_PP:04X}: {original_pp} -> {test_pp} -> {original_pp}"
    );

    session.parameter_write(PARAM_MOTOR_PP, ParameterValue::U8(test_pp))?;
    let readback = read_u8(&session, PARAM_MOTOR_PP)?;
    if readback != test_pp {
        // Best effort restore before returning the mismatch.
        let _ = session.parameter_write(PARAM_MOTOR_PP, ParameterValue::U8(original_pp));
        return Err(format!("write readback mismatch: expected {test_pp}, got {readback}").into());
    }

    session.parameter_write(PARAM_MOTOR_PP, ParameterValue::U8(original_pp))?;
    let restored = read_u8(&session, PARAM_MOTOR_PP)?;
    if restored != original_pp {
        return Err(format!(
            "restore readback mismatch: expected {original_pp}, got {restored}"
        )
        .into());
    }
    println!("PASS: Parameter WRITE/readback/restore");

    println!(
        "Action lifecycle: ACTION_PROTECTION_CLEAR (0x{ACTION_PROTECTION_CLEAR:04X})"
    );
    let handle = session.action_start(ACTION_PROTECTION_CLEAR)?;
    println!("accepted: txn={} action=0x{:04X}", handle.txn.get(), handle.action_id);

    let timeout = Duration::from_millis(args.action_timeout_ms);
    loop {
        match events.recv_timeout(timeout)? {
            SessionEvent::ActionCompleted {
                handle: completed,
                status,
            } if completed == handle => {
                if status != AxdrStatus::Ok {
                    return Err(format!("Action completed with status {status:?}").into());
                }
                println!("completed: status={status:?}");
                break;
            }
            SessionEvent::ActionCompleted { .. }
            | SessionEvent::DeviceEvent(_)
            | SessionEvent::FastData(_)
            | SessionEvent::NormalData(_) => {
                // Ignore unrelated asynchronous traffic while waiting for this Action.
            }
        }
    }

    let state_after = read_u8(&session, PARAM_MOTOR_STATE)?;
    if state_after != MOTOR_STATE_DISABLED {
        return Err(format!(
            "motor state changed unexpectedly after protection clear: {state_after}"
        )
        .into());
    }

    println!("PASS: Action accepted/completed lifecycle");
    println!("PASS: safe Parameter WRITE and Action lifecycle are working");
    Ok(())
}

fn read_u8(session: &DeviceSession, id: u16) -> Result<u8, Box<dyn Error>> {
    match session.parameter_read(id, ParameterType::U8)? {
        ParameterValue::U8(value) => Ok(value),
        other => Err(format!("parameter 0x{id:04X} returned unexpected value {other:?}").into()),
    }
}
