use std::error::Error;
use std::str::FromStr;
use std::sync::mpsc::RecvTimeoutError;
use std::time::Duration;

use clap::{Parser, Subcommand, ValueEnum};
use nmixx_app::{
    AxdrStatus, DEFAULT_USB_BAUD, DeviceSession, ParameterType, ParameterValue,
    PositionValue, SessionEvent,
};

#[derive(Debug, Parser)]
#[command(name = "nmixxctl", version, about = "NMIXX Motor Studio CLI")]
struct Cli {
    /// USB CDC serial port, for commands that access a device.
    #[arg(long, global = true)]
    port: Option<String>,

    /// Serial line rate used when opening the USB CDC port.
    #[arg(long, global = true, default_value_t = DEFAULT_USB_BAUD)]
    baud: u32,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Device discovery and connection utilities.
    Device {
        #[command(subcommand)]
        command: DeviceCommand,
    },
    /// Read or write a firmware Parameter by ID.
    Param {
        #[command(subcommand)]
        command: ParamCommand,
    },
    /// Trigger a firmware Action by ID.
    Action {
        /// Action ID, decimal or 0x-prefixed hexadecimal.
        #[arg(value_parser = parse_u16)]
        id: u16,

        /// Return after the Action is accepted instead of waiting for completion.
        #[arg(long)]
        no_wait: bool,

        /// Completion wait timeout in seconds.
        #[arg(long, default_value_t = 30)]
        timeout: u64,
    },
}

#[derive(Debug, Subcommand)]
enum DeviceCommand {
    /// List serial ports visible to NMIXX Motor Studio.
    List,
}

#[derive(Debug, Subcommand)]
enum ParamCommand {
    /// Read one Parameter.
    Get {
        #[arg(value_parser = parse_u16)]
        id: u16,
        #[arg(value_enum)]
        ty: CliParameterType,
    },
    /// Write one Parameter.
    Set {
        #[arg(value_parser = parse_u16)]
        id: u16,
        #[arg(value_enum)]
        ty: CliParameterType,
        /// Value text. Position uses `turns,theta`, for example `-2,1.5`.
        value: String,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CliParameterType {
    U8,
    I8,
    F32,
    I32,
    U32,
    Position,
}

impl From<CliParameterType> for ParameterType {
    fn from(value: CliParameterType) -> Self {
        match value {
            CliParameterType::U8 => Self::U8,
            CliParameterType::I8 => Self::I8,
            CliParameterType::F32 => Self::F32,
            CliParameterType::I32 => Self::I32,
            CliParameterType::U32 => Self::U32,
            CliParameterType::Position => Self::Position,
        }
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let Cli { port, baud, command } = Cli::parse();

    match command {
        Command::Device {
            command: DeviceCommand::List,
        } => {
            for port in DeviceSession::available_usb_ports()? {
                println!("{port}");
            }
        }
        Command::Param { command } => {
            let session = open_session(port.as_deref(), baud)?;
            match command {
                ParamCommand::Get { id, ty } => {
                    let value = session.parameter_read(id, ty.into())?;
                    println!("{}", format_value(value));
                }
                ParamCommand::Set { id, ty, value } => {
                    let value = parse_parameter_value(ty, &value)?;
                    session.parameter_write(id, value)?;
                    println!("OK");
                }
            }
        }
        Command::Action {
            id,
            no_wait,
            timeout,
        } => {
            let session = open_session(port.as_deref(), baud)?;
            let events = if no_wait { None } else { Some(session.subscribe()?) };
            let handle = session.action_start(id)?;
            println!("accepted txn={} action=0x{id:04X}", handle.txn.get());

            if let Some(events) = events {
                loop {
                    match events.recv_timeout(Duration::from_secs(timeout)) {
                        Ok(SessionEvent::ActionCompleted {
                            handle: completed,
                            status,
                        }) if completed == handle => {
                            match status {
                                AxdrStatus::Ok => println!("completed OK"),
                                other => println!("completed {other:?}"),
                            }
                            break;
                        }
                        Ok(_) => continue,
                        Err(RecvTimeoutError::Timeout) => {
                            return Err(format!(
                                "action 0x{id:04X} completion timed out after {timeout}s"
                            )
                            .into());
                        }
                        Err(RecvTimeoutError::Disconnected) => {
                            return Err("device session closed while waiting for action".into());
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

fn open_session(port: Option<&str>, baud: u32) -> Result<DeviceSession, Box<dyn Error>> {
    let port = port.ok_or("--port is required for this command")?;
    Ok(DeviceSession::open_usb(port, baud)?)
}

fn parse_u16(text: &str) -> Result<u16, String> {
    if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        u16::from_str_radix(hex, 16).map_err(|error| error.to_string())
    } else {
        text.parse::<u16>().map_err(|error| error.to_string())
    }
}

fn parse_parameter_value(ty: CliParameterType, text: &str) -> Result<ParameterValue, String> {
    let parse = |name: &str| format!("invalid {name} value '{text}'");
    Ok(match ty {
        CliParameterType::U8 => ParameterValue::U8(u8::from_str(text).map_err(|_| parse("u8"))?),
        CliParameterType::I8 => ParameterValue::I8(i8::from_str(text).map_err(|_| parse("i8"))?),
        CliParameterType::F32 => ParameterValue::F32(f32::from_str(text).map_err(|_| parse("f32"))?),
        CliParameterType::I32 => ParameterValue::I32(i32::from_str(text).map_err(|_| parse("i32"))?),
        CliParameterType::U32 => ParameterValue::U32(u32::from_str(text).map_err(|_| parse("u32"))?),
        CliParameterType::Position => {
            let Some((turns, theta)) = text.split_once(',') else {
                return Err("position value must use turns,theta (for example -2,1.5)".into());
            };
            ParameterValue::Position(PositionValue {
                turns: i32::from_str(turns.trim()).map_err(|_| parse("position turns"))?,
                theta: f32::from_str(theta.trim()).map_err(|_| parse("position theta"))?,
            })
        }
    })
}

fn format_value(value: ParameterValue) -> String {
    match value {
        ParameterValue::U8(value) => value.to_string(),
        ParameterValue::I8(value) => value.to_string(),
        ParameterValue::F32(value) => format!("{value:.9}"),
        ParameterValue::I32(value) => value.to_string(),
        ParameterValue::U32(value) => value.to_string(),
        ParameterValue::Position(value) => format!("{},{}", value.turns, value.theta),
    }
}
