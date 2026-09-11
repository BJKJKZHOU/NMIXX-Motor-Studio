use std::error::Error;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::mpsc::RecvTimeoutError;
use std::time::Duration;

use clap::{Parser, Subcommand, ValueEnum};
use nmixx_app::{
    ActionMetadata, AxdrStatus, DEFAULT_USB_BAUD, DeviceSession, HostSchema, ParameterMetadata,
    ParameterType, ParameterValue, PositionValue, SchemaNumber, SessionEvent,
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

    /// Firmware Host schema exported from Parameter/parameter.yaml.
    #[arg(long, global = true)]
    schema: Option<PathBuf>,

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
    /// Read, write, or inspect firmware Parameters.
    Param {
        #[command(subcommand)]
        command: ParamCommand,
    },
    /// Trigger a firmware Action by symbol, Host name, or numeric ID.
    Action {
        #[command(subcommand)]
        command: ActionCommand,
    },
}

#[derive(Debug, Subcommand)]
enum DeviceCommand {
    /// List serial ports visible to NMIXX Motor Studio.
    List,
}

#[derive(Debug, Subcommand)]
enum ParamCommand {
    /// List Parameters from the loaded Host schema.
    List,
    /// Show metadata for one Parameter.
    Info {
        /// Parameter key: schema symbol/name, decimal ID, or 0x-prefixed hexadecimal ID.
        key: String,
    },
    /// Read one Parameter.
    Get {
        /// Parameter key: schema symbol/name, decimal ID, or 0x-prefixed hexadecimal ID.
        key: String,

        /// Parameter type override. Required for numeric IDs when no schema is loaded.
        #[arg(long = "type", value_enum)]
        ty: Option<CliParameterType>,
    },
    /// Write one Parameter.
    Set {
        /// Parameter key: schema symbol/name, decimal ID, or 0x-prefixed hexadecimal ID.
        key: String,

        /// Value text. Position uses `turns,theta`, for example `-2,1.5`.
        value: String,

        /// Parameter type override. Required for numeric IDs when no schema is loaded.
        #[arg(long = "type", value_enum)]
        ty: Option<CliParameterType>,
    },
}

#[derive(Debug, Subcommand)]
enum ActionCommand {
    /// List Actions from the loaded Host schema.
    List,
    /// Show metadata for one Action.
    Info {
        /// Action key: schema symbol/name, decimal ID, or 0x-prefixed hexadecimal ID.
        key: String,
    },
    /// Start an Action.
    Start {
        /// Action key: schema symbol/name, decimal ID, or 0x-prefixed hexadecimal ID.
        key: String,

        /// Return after the Action is accepted instead of waiting for completion.
        #[arg(long)]
        no_wait: bool,

        /// Completion wait timeout in seconds.
        #[arg(long, default_value_t = 30)]
        timeout: u64,
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

struct ResolvedParameter<'a> {
    id: u16,
    ty: ParameterType,
    metadata: Option<&'a ParameterMetadata>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let Cli {
        port,
        baud,
        schema,
        command,
    } = Cli::parse();
    let schema = schema.as_deref().map(HostSchema::load).transpose()?;

    match command {
        Command::Device {
            command: DeviceCommand::List,
        } => {
            for port in DeviceSession::available_usb_ports()? {
                println!("{port}");
            }
        }
        Command::Param { command } => match command {
            ParamCommand::List => {
                let schema = require_schema(schema.as_ref())?;
                print_parameter_list(schema);
            }
            ParamCommand::Info { key } => {
                let schema = require_schema(schema.as_ref())?;
                let metadata = resolve_parameter_metadata(schema, &key)?;
                print_parameter_info(metadata);
            }
            ParamCommand::Get { key, ty } => {
                let session = open_session(port.as_deref(), baud)?;
                let parameter = resolve_parameter(schema.as_ref(), &key, ty)?;
                let value = session.parameter_read(parameter.id, parameter.ty)?;
                print_parameter_value(parameter.metadata, parameter.id, value);
            }
            ParamCommand::Set { key, value, ty } => {
                let session = open_session(port.as_deref(), baud)?;
                let parameter = resolve_parameter(schema.as_ref(), &key, ty)?;
                if let Some(metadata) = parameter.metadata {
                    if metadata.access != "rw" {
                        return Err(format!(
                            "parameter {} (0x{:04X}) is not writable (access={})",
                            metadata.symbol, metadata.id, metadata.access
                        )
                        .into());
                    }
                }
                let value = parse_parameter_value(parameter.ty, &value)?;
                session.parameter_write(parameter.id, value)?;
                println!("OK");
            }
        },
        Command::Action { command } => match command {
            ActionCommand::List => {
                let schema = require_schema(schema.as_ref())?;
                print_action_list(schema);
            }
            ActionCommand::Info { key } => {
                let schema = require_schema(schema.as_ref())?;
                let metadata = resolve_action_metadata(schema, &key)?;
                print_action_info(metadata);
            }
            ActionCommand::Start {
                key,
                no_wait,
                timeout,
            } => {
                let (action_id, action_label) = resolve_action(schema.as_ref(), &key)?;
                let session = open_session(port.as_deref(), baud)?;
                let events = if no_wait {
                    None
                } else {
                    Some(session.subscribe()?)
                };
                let handle = session.action_start(action_id)?;
                println!(
                    "accepted txn={} action={} (0x{action_id:04X})",
                    handle.txn.get(),
                    action_label
                );

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
                                    "action {action_label} (0x{action_id:04X}) completion timed out after {timeout}s"
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
        },
    }

    Ok(())
}

fn require_schema(schema: Option<&HostSchema>) -> Result<&HostSchema, Box<dyn Error>> {
    schema.ok_or_else(|| "--schema is required for this command".into())
}

fn open_session(port: Option<&str>, baud: u32) -> Result<DeviceSession, Box<dyn Error>> {
    let port = port.ok_or("--port is required for this command")?;
    Ok(DeviceSession::open_usb(port, baud)?)
}

fn resolve_parameter_metadata<'a>(
    schema: &'a HostSchema,
    key: &str,
) -> Result<&'a ParameterMetadata, Box<dyn Error>> {
    if let Some(metadata) = schema.parameter_by_key(key) {
        return Ok(metadata);
    }
    if let Ok(id) = parse_u16(key) {
        return schema
            .parameter_by_id(id)
            .ok_or_else(|| format!("parameter ID 0x{id:04X} is not present in the loaded schema").into());
    }
    Err(format!("unknown parameter key '{key}' in loaded schema").into())
}

fn resolve_action_metadata<'a>(
    schema: &'a HostSchema,
    key: &str,
) -> Result<&'a ActionMetadata, Box<dyn Error>> {
    if let Some(metadata) = schema.action_by_key(key) {
        return Ok(metadata);
    }
    if let Ok(id) = parse_u16(key) {
        return schema
            .action_by_id(id)
            .ok_or_else(|| format!("action ID 0x{id:04X} is not present in the loaded schema").into());
    }
    Err(format!("unknown action key '{key}' in loaded schema").into())
}

fn resolve_parameter<'a>(
    schema: Option<&'a HostSchema>,
    key: &str,
    type_override: Option<CliParameterType>,
) -> Result<ResolvedParameter<'a>, Box<dyn Error>> {
    if let Some(schema) = schema {
        if let Some(metadata) = schema.parameter_by_key(key) {
            let schema_type = metadata.parameter_type()?;
            if let Some(override_type) = type_override {
                let override_type: ParameterType = override_type.into();
                if override_type != schema_type {
                    return Err(format!(
                        "parameter {} type mismatch: schema={}, CLI={override_type:?}",
                        metadata.symbol, metadata.type_name
                    )
                    .into());
                }
            }
            return Ok(ResolvedParameter {
                id: metadata.id,
                ty: schema_type,
                metadata: Some(metadata),
            });
        }

        if let Ok(id) = parse_u16(key) {
            if let Some(metadata) = schema.parameter_by_id(id) {
                let schema_type = metadata.parameter_type()?;
                return Ok(ResolvedParameter {
                    id,
                    ty: schema_type,
                    metadata: Some(metadata),
                });
            }
            if let Some(override_type) = type_override {
                return Ok(ResolvedParameter {
                    id,
                    ty: override_type.into(),
                    metadata: None,
                });
            }
            return Err(format!(
                "parameter ID 0x{id:04X} is not present in the loaded schema; use --type for raw access"
            )
            .into());
        }

        return Err(format!("unknown parameter key '{key}' in loaded schema").into());
    }

    let id = parse_u16(key).map_err(|_| {
        format!("parameter '{key}' requires --schema; without a schema only numeric IDs are accepted")
    })?;
    let ty = type_override.ok_or("--type is required for raw numeric parameter access without --schema")?;
    Ok(ResolvedParameter {
        id,
        ty: ty.into(),
        metadata: None,
    })
}

fn resolve_action(schema: Option<&HostSchema>, key: &str) -> Result<(u16, String), Box<dyn Error>> {
    if let Some(schema) = schema {
        if let Some(action) = schema.action_by_key(key) {
            return Ok((
                action.id,
                action.name.as_deref().unwrap_or(&action.symbol).to_owned(),
            ));
        }
        if let Ok(id) = parse_u16(key) {
            if let Some(action) = schema.action_by_id(id) {
                return Ok((
                    id,
                    action.name.as_deref().unwrap_or(&action.symbol).to_owned(),
                ));
            }
            return Ok((id, format!("0x{id:04X}")));
        }
        return Err(format!("unknown action key '{key}' in loaded schema").into());
    }

    let id = parse_u16(key).map_err(|_| {
        format!("action '{key}' requires --schema; without a schema only numeric IDs are accepted")
    })?;
    Ok((id, format!("0x{id:04X}")))
}

fn parse_u16(text: &str) -> Result<u16, String> {
    if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        u16::from_str_radix(hex, 16).map_err(|error| error.to_string())
    } else {
        text.parse::<u16>().map_err(|error| error.to_string())
    }
}

fn parse_parameter_value(ty: ParameterType, text: &str) -> Result<ParameterValue, String> {
    let parse = |name: &str| format!("invalid {name} value '{text}'");
    Ok(match ty {
        ParameterType::U8 => ParameterValue::U8(u8::from_str(text).map_err(|_| parse("u8"))?),
        ParameterType::I8 => ParameterValue::I8(i8::from_str(text).map_err(|_| parse("i8"))?),
        ParameterType::F32 => ParameterValue::F32(f32::from_str(text).map_err(|_| parse("f32"))?),
        ParameterType::I32 => ParameterValue::I32(i32::from_str(text).map_err(|_| parse("i32"))?),
        ParameterType::U32 => ParameterValue::U32(u32::from_str(text).map_err(|_| parse("u32"))?),
        ParameterType::Position => {
            let Some((turns, theta)) = text.split_once(',') else {
                return Err("position value must use turns,theta (for example -2,1.5)".into());
            };
            ParameterValue::Position(PositionValue {
                turns: i32::from_str(turns.trim()).map_err(|_| parse("position turns"))?,
                theta: f32::from_str(theta.trim()).map_err(|_| parse("position theta"))?,
            })
        }
        ParameterType::Action => return Err("Action is not a value Parameter type".into()),
    })
}

fn print_parameter_list(schema: &HostSchema) {
    for parameter in &schema.parameters {
        let key = parameter.name.as_deref().unwrap_or(&parameter.symbol);
        let unit = parameter.unit.as_deref().unwrap_or("");
        println!(
            "0x{:04X}  {:<28} {:<8} {:<2} {}",
            parameter.id, key, parameter.type_name, parameter.access, unit
        );
    }
}

fn print_action_list(schema: &HostSchema) {
    for action in &schema.actions {
        let key = action.name.as_deref().unwrap_or(&action.symbol);
        println!("0x{:04X}  {}", action.id, key);
    }
}

fn print_parameter_info(parameter: &ParameterMetadata) {
    println!("symbol: {}", parameter.symbol);
    if let Some(name) = parameter.name.as_deref() {
        println!("name: {name}");
    }
    println!("id: 0x{:04X}", parameter.id);
    println!("type: {}", parameter.type_name);
    println!("access: {}", parameter.access);
    if let Some(unit) = parameter.unit.as_deref() {
        println!("unit: {unit}");
    }
    if let Some(write_state) = parameter.write_state.as_deref() {
        println!("write_state: {write_state}");
    }
    if let Some(range) = parameter.range.as_ref() {
        if let Some(min) = range.min {
            println!("min: {}{}", format_schema_number(min), if range.exclusive_min { " (exclusive)" } else { "" });
        }
        if let Some(max) = range.max {
            println!("max: {}{}", format_schema_number(max), if range.exclusive_max { " (exclusive)" } else { "" });
        }
        if let Some(symbol) = range.max_symbol.as_deref() {
            println!("max_symbol: {symbol}");
        }
        if let Some(binding) = range.max_binding.as_deref() {
            println!("max_binding: {binding}");
        }
        if !range.max_bindings.is_empty() {
            println!("max_bindings: {}", range.max_bindings.join(", "));
        }
    }
    if !parameter.allowed.is_empty() {
        let values = parameter.allowed.iter().copied().map(format_schema_number).collect::<Vec<_>>();
        println!("allowed: {}", values.join(", "));
    }
    if !parameter.allowed_symbols.is_empty() {
        println!("allowed_symbols: {}", parameter.allowed_symbols.join(", "));
    }
    if let Some(scale) = parameter.plot_scale {
        println!("plot_scale: {scale}");
    }
    println!("description: {}", parameter.description);
}

fn print_action_info(action: &ActionMetadata) {
    println!("symbol: {}", action.symbol);
    if let Some(name) = action.name.as_deref() {
        println!("name: {name}");
    }
    println!("id: 0x{:04X}", action.id);
    println!("description: {}", action.description);
}

fn format_schema_number(value: SchemaNumber) -> String {
    match value {
        SchemaNumber::Integer(value) => value.to_string(),
        SchemaNumber::Float(value) => value.to_string(),
    }
}

fn print_parameter_value(metadata: Option<&ParameterMetadata>, id: u16, value: ParameterValue) {
    match metadata {
        Some(metadata) => {
            let label = metadata.name.as_deref().unwrap_or(&metadata.symbol);
            match metadata.unit.as_deref() {
                Some(unit) => println!("{label} (0x{id:04X}) = {} {unit}", format_value(value)),
                None => println!("{label} (0x{id:04X}) = {}", format_value(value)),
            }
        }
        None => println!("0x{id:04X} = {}", format_value(value)),
    }
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
