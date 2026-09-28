use std::error::Error;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::mpsc::RecvTimeoutError;
use std::time::Duration;

use clap::{Parser, Subcommand, ValueEnum};
use nmixx_app::{
    ActionCompletionWaiter, ActionHandle, ActionMetadata, ApplicationSession, AxdrStatus,
    DEFAULT_USB_BAUD, HostSchema, IdentificationKind, MotionMode, MotionRuntimeStatus,
    ParameterMetadata, ParameterType, ParameterValue, PositionCommand, PositionMotionRequest,
    PositionValue, SchemaNumber, SchemaStore, SpeedMotionRequest,
};
use nmixx_app::raw::{DeviceSession, SessionEvent};

#[derive(Debug, Parser)]
#[command(name = "nmixxctl", version, about = "NMIXX Motor Studio CLI")]
struct Cli {
    #[arg(long, global = true)]
    port: Option<String>,

    #[arg(long, global = true, default_value_t = DEFAULT_USB_BAUD)]
    baud: u32,

    /// Firmware Host schema exported from Parameter/parameter.yaml.
    #[arg(long, global = true)]
    schema: Option<PathBuf>,

    /// Override the local NMIXX schema-store directory.
    #[arg(long, global = true)]
    schema_store: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Device {
        #[command(subcommand)]
        command: DeviceCommand,
    },
    Param {
        #[command(subcommand)]
        command: ParamCommand,
    },
    Motor {
        #[command(subcommand)]
        command: MotorCommand,
    },
    Motion {
        #[command(subcommand)]
        command: MotionCommand,
    },
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    Preflight {
        #[command(subcommand)]
        command: PreflightCommand,
    },
    Action {
        #[command(subcommand)]
        command: ActionCommand,
    },
    /// Manage locally imported Host schemas.
    Schema {
        #[command(subcommand)]
        command: SchemaCommand,
    },
}

#[derive(Debug, Subcommand)]
enum DeviceCommand {
    List,
}

#[derive(Debug, Subcommand)]
enum ParamCommand {
    List,
    Info { key: String },
    Get {
        key: String,
        #[arg(long = "type", value_enum)]
        ty: Option<CliParameterType>,
    },
    Set {
        key: String,
        value: String,
        #[arg(long = "type", value_enum)]
        ty: Option<CliParameterType>,
    },
    ReadAll,
}

#[derive(Debug, Subcommand)]
enum MotorCommand {
    Enable,
    Disable,
    ClearFault,
    Stop {
        #[arg(long, default_value_t = 60)]
        timeout: u64,
    },
    PhaseSearch {
        #[arg(long, default_value_t = 30)]
        timeout: u64,
    },
}

#[derive(Debug, Subcommand)]
enum MotionCommand {
    Status,
    Mode {
        #[arg(value_enum)]
        mode: CliMotionMode,
    },
    Speed {
        /// Mechanical speed target in rad/s.
        target: f32,
        #[arg(long)]
        max_speed: Option<f32>,
        #[arg(long)]
        acc: Option<f32>,
        #[arg(long)]
        dec: Option<f32>,
    },
    Position {
        /// Absolute target turns or incremental delta turns, selected by --command.
        target_turn: f64,
        #[arg(long, value_enum, default_value = "incremental")]
        command: CliPositionCommand,
        #[arg(long)]
        max_speed: Option<f32>,
        #[arg(long)]
        acc: Option<f32>,
        #[arg(long)]
        dec: Option<f32>,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CliMotionMode {
    Position,
    Speed,
}

impl From<CliMotionMode> for MotionMode {
    fn from(value: CliMotionMode) -> Self {
        match value {
            CliMotionMode::Position => Self::Position,
            CliMotionMode::Speed => Self::Speed,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CliPositionCommand {
    Absolute,
    Incremental,
}

impl From<CliPositionCommand> for PositionCommand {
    fn from(value: CliPositionCommand) -> Self {
        match value {
            CliPositionCommand::Absolute => Self::Absolute,
            CliPositionCommand::Incremental => Self::Incremental,
        }
    }
}

#[derive(Debug, Subcommand)]
enum ConfigCommand {
    Save,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CliIdentificationKind {
    RsLs,
    Flux,
    Jb,
}

impl From<CliIdentificationKind> for IdentificationKind {
    fn from(value: CliIdentificationKind) -> Self {
        match value {
            CliIdentificationKind::RsLs => Self::RsLs,
            CliIdentificationKind::Flux => Self::Flux,
            CliIdentificationKind::Jb => Self::Jb,
        }
    }
}

#[derive(Debug, Subcommand)]
enum PreflightCommand {
    Identification {
        #[arg(value_enum)]
        kind: CliIdentificationKind,
    },
    PhaseSearch,
}

#[derive(Debug, Subcommand)]
enum ActionCommand {
    List,
    Info { key: String },
    /// Expert/raw Action dispatch kept for protocol bring-up and compatibility.
    Start {
        key: String,
        #[arg(long)]
        no_wait: bool,
        #[arg(long, default_value_t = 30)]
        timeout: u64,
    },
}

#[derive(Debug, Subcommand)]
enum SchemaCommand {
    /// Validate and import one Host schema TOML into the local store.
    Import {
        path: PathBuf,
        #[arg(long)]
        replace: bool,
    },
    /// List locally imported schemas.
    List,
    /// Show metadata for one imported schema key.
    Info { key: String },
    /// Export one imported schema without rewriting its TOML contents.
    Export { key: String, destination: PathBuf },
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
        schema_store,
        command,
    } = Cli::parse();
    let schema = schema.as_deref().map(HostSchema::load).transpose()?;

    match command {
        Command::Schema { command } => {
            let store = match schema_store {
                Some(root) => SchemaStore::new(root),
                None => SchemaStore::default()?,
            };
            match command {
                SchemaCommand::Import { path, replace } => {
                    let stored = store.import(&path, replace)?;
                    println!("imported {}", stored.key);
                    println!("path: {}", stored.path.display());
                }
                SchemaCommand::List => {
                    for stored in store.list()? {
                        println!(
                            "{:<48} protocol={} parameters={} actions={}",
                            stored.key,
                            stored.schema.protocol,
                            stored.schema.parameters.len(),
                            stored.schema.actions.len()
                        );
                    }
                }
                SchemaCommand::Info { key } => {
                    let stored = store.get(&key)?;
                    print_schema_info(&stored.key, &stored.path, &stored.schema);
                }
                SchemaCommand::Export { key, destination } => {
                    store.export(&key, &destination)?;
                    println!("exported {key} -> {}", destination.display());
                }
            }
        }
        Command::Device {
            command: DeviceCommand::List,
        } => {
            for port in ApplicationSession::available_usb_ports()? {
                println!("{port}");
            }
        }
        Command::Param { command } => match command {
            ParamCommand::List => print_parameter_list(require_schema(schema.as_ref())?),
            ParamCommand::Info { key } => {
                let metadata = resolve_parameter_metadata(require_schema(schema.as_ref())?, &key)?;
                print_parameter_info(metadata);
            }
            ParamCommand::Get { key, ty } => {
                let schema = require_schema(schema.as_ref())?;
                let parameter = resolve_parameter(schema, &key, ty)?;
                let app = open_application(port.as_deref(), baud, schema.clone())?;
                let value = app.parameter_read(parameter.id)?;
                print_parameter_value(parameter.metadata, parameter.id, value);
            }
            ParamCommand::Set { key, value, ty } => {
                let schema = require_schema(schema.as_ref())?;
                let parameter = resolve_parameter(schema, &key, ty)?;
                let value = parse_parameter_value(parameter.ty, &value)?;
                let app = open_application(port.as_deref(), baud, schema.clone())?;
                app.parameter_write(parameter.id, value)?;
                println!("OK");
            }
            ParamCommand::ReadAll => {
                let schema = require_schema(schema.as_ref())?;
                let app = open_application(port.as_deref(), baud, schema.clone())?;
                let results = app.parameter_refresh_all()?;
                let mut failed = 0usize;
                for (id, result) in results {
                    match result {
                        Ok(value) => {
                            let metadata = schema.parameter_by_id(id);
                            print_parameter_value(metadata, id, value);
                        }
                        Err(error) => {
                            failed += 1;
                            eprintln!("0x{id:04X}: {error}");
                        }
                    }
                }
                if failed != 0 {
                    return Err(format!("{failed} parameter reads failed").into());
                }
            }
        },
        Command::Motor { command } => {
            let schema = require_schema(schema.as_ref())?;
            let app = open_application(port.as_deref(), baud, schema.clone())?;
            match command {
                MotorCommand::Enable => {
                    let handle = app.motor_enable()?;
                    println!("completed txn={} motor enable", handle.txn.get());
                }
                MotorCommand::Disable => {
                    let handle = app.motor_disable()?;
                    println!("completed txn={} motor disable", handle.txn.get());
                }
                MotorCommand::ClearFault => {
                    let handle = app.protection_clear()?;
                    println!("completed txn={} clear fault", handle.txn.get());
                }
                MotorCommand::Stop { timeout } => {
                    let handle = app.motor_stop()?;
                    println!("accepted txn={} motor stop", handle.txn.get());
                    let status = app.motor_wait_stopped(Duration::from_secs(timeout))?;
                    println!("stopped state={}", status.motor_state);
                }
                MotorCommand::PhaseSearch { timeout } => {
                    let completion = app.action_completion_waiter()?;
                    let handle = app.phase_search_start()?;
                    println!("accepted txn={} phase search", handle.txn.get());
                    wait_for_action(&completion, handle, "phase search", timeout)?;
                }
            }
        }
        Command::Motion { command } => {
            let schema = require_schema(schema.as_ref())?;
            let app = open_application(port.as_deref(), baud, schema.clone())?;
            match command {
                MotionCommand::Status => print_motion_status(&app.motion_status()?),
                MotionCommand::Mode { mode } => {
                    let mode = app.motion_set_mode(mode.into())?;
                    println!("mode: {}", motion_mode_name(mode));
                }
                MotionCommand::Speed {
                    target,
                    max_speed,
                    acc,
                    dec,
                } => {
                    let handle = app.motion_run_speed(SpeedMotionRequest {
                        target_rad_s: target,
                        max_speed_rad_s: max_speed,
                        acceleration_rad_s2: acc,
                        deceleration_rad_s2: dec,
                    })?;
                    println!(
                        "started txn={} speed target={target:.6} rad/s",
                        handle.txn.get()
                    );
                }
                MotionCommand::Position {
                    target_turn,
                    command,
                    max_speed,
                    acc,
                    dec,
                } => {
                    let command = PositionCommand::from(command);
                    let handle = app.motion_run_position(PositionMotionRequest {
                        command,
                        target_turn,
                        max_speed_rad_s: max_speed,
                        acceleration_rad_s2: acc,
                        deceleration_rad_s2: dec,
                    })?;
                    println!(
                        "started txn={} position {:?} target={target_turn:.9} turn",
                        handle.txn.get(),
                        command
                    );
                }
            }
        }
        Command::Config { command } => {
            let schema = require_schema(schema.as_ref())?;
            let app = open_application(port.as_deref(), baud, schema.clone())?;
            match command {
                ConfigCommand::Save => {
                    let handle = app.config_save()?;
                    println!("completed txn={} config save", handle.txn.get());
                }
            }
        }
        Command::Preflight { command } => {
            let schema = require_schema(schema.as_ref())?;
            let app = open_application(port.as_deref(), baud, schema.clone())?;
            match command {
                PreflightCommand::Identification { kind } => {
                    let issues = app.preflight_identification(kind.into())?;
                    print_preflight_issues(&issues);
                }
                PreflightCommand::PhaseSearch => {
                    let issues = app.preflight_phase_search()?;
                    print_preflight_issues(&issues);
                }
            }
        }
        Command::Action { command } => match command {
            ActionCommand::List => print_action_list(require_schema(schema.as_ref())?),
            ActionCommand::Info { key } => {
                let metadata = resolve_action_metadata(require_schema(schema.as_ref())?, &key)?;
                print_action_info(metadata);
            }
            ActionCommand::Start { key, no_wait, timeout } => {
                let schema = require_schema(schema.as_ref())?;
                let action = resolve_action_metadata(schema, &key)?;
                let session = open_raw_session(port.as_deref(), baud)?;
                let events = if no_wait { None } else { Some(session.subscribe()?) };
                let handle = session.action_start(action.id)?;
                println!(
                    "accepted txn={} action={} (0x{:04X})",
                    handle.txn.get(),
                    action.label,
                    action.id
                );
                wait_for_raw_action(events, handle, &action.label, action.id, timeout)?;
            }
        },
    }
    Ok(())
}

fn print_motion_status(status: &MotionRuntimeStatus) {
    println!("state: {}", status.motor_state);
    println!("mode: {}", motion_mode_name(status.mode));

    print_optional_f32("speed target", status.speed_target_rad_s, "rad/s");
    print_optional_f32("speed ref", status.speed_ref_rad_s, "rad/s");
    print_optional_f32("speed feedback", status.speed_feedback_rad_s, "rad/s");
    print_optional_f32("encoder speed", status.encoder_speed_rad_s, "rad/s");
    print_optional_f32("speed limit", status.speed_limit_rad_s, "rad/s");

    print_optional_position("position target", status.position_target);
    print_optional_position("position ref", status.position_ref);
    print_optional_position("position feedback", status.position_feedback);

    if status.encoder_ready.is_some()
        || status.encoder_valid.is_some()
        || status.encoder_fault.is_some()
    {
        println!(
            "encoder: ready={} valid={} fault={}",
            status.encoder_ready.map_or("-".to_owned(), |value| value.to_string()),
            status.encoder_valid.map_or("-".to_owned(), |value| value.to_string()),
            status.encoder_fault.map_or("-".to_owned(), |value| value.to_string())
        );
    }
}

fn motion_mode_name(mode: MotionMode) -> &'static str {
    match mode {
        MotionMode::Position => "POSITION",
        MotionMode::Speed => "SPEED",
        MotionMode::SensorlessSpeed => "SENSORLESS_SPEED",
        MotionMode::Torque => "TORQUE",
    }
}

fn print_optional_f32(label: &str, value: Option<f32>, unit: &str) {
    if let Some(value) = value {
        println!("{label}: {value:.6} {unit}");
    }
}

fn print_optional_position(label: &str, value: Option<PositionValue>) {
    if let Some(value) = value {
        let turns = f64::from(value.turns) + f64::from(value.theta) / std::f64::consts::TAU;
        println!(
            "{label}: {:.9} turn (turn={}, theta={:.9} rad)",
            turns, value.turns, value.theta
        );
    }
}

fn print_preflight_issues(issues: &[nmixx_app::PreflightIssue]) {
    if issues.is_empty() {
        println!("ready");
        return;
    }

    for issue in issues {
        match issue.parameter_id {
            Some(id) => println!("0x{id:04X}: {}", issue.reason),
            None => println!("{}", issue.reason),
        }
    }
}

fn wait_for_action(
    completion: &ActionCompletionWaiter,
    handle: ActionHandle,
    label: &str,
    timeout: u64,
) -> Result<(), Box<dyn Error>> {
    match completion.wait(handle, Some(Duration::from_secs(timeout)))? {
        AxdrStatus::Ok => {
            println!("completed OK");
            Ok(())
        }
        status => Err(format!("{label} completed {status:?}").into()),
    }
}

fn wait_for_raw_action(
    events: Option<std::sync::mpsc::Receiver<SessionEvent>>,
    handle: ActionHandle,
    label: &str,
    action_id: u16,
    timeout: u64,
) -> Result<(), Box<dyn Error>> {
    let Some(events) = events else { return Ok(()); };
    loop {
        match events.recv_timeout(Duration::from_secs(timeout)) {
            Ok(SessionEvent::ActionCompleted { handle: completed, status }) if completed == handle => {
                return match status {
                    AxdrStatus::Ok => { println!("completed OK"); Ok(()) }
                    other => Err(format!("{label} (0x{action_id:04X}) completed {other:?}").into()),
                };
            }
            Ok(_) => continue,
            Err(RecvTimeoutError::Timeout) => {
                return Err(format!("{label} (0x{action_id:04X}) completion timed out after {timeout}s").into());
            }
            Err(RecvTimeoutError::Disconnected) => {
                return Err("device session closed while waiting for action".into());
            }
        }
    }
}

fn open_raw_session(port: Option<&str>, baud: u32) -> Result<DeviceSession, Box<dyn Error>> {
    let port = port.ok_or("--port is required for this command")?;
    Ok(DeviceSession::open_usb(port, baud)?)
}

fn require_schema(schema: Option<&HostSchema>) -> Result<&HostSchema, Box<dyn Error>> {
    schema.ok_or_else(|| "--schema is required for this command".into())
}

fn open_application(
    port: Option<&str>,
    baud: u32,
    schema: HostSchema,
) -> Result<ApplicationSession, Box<dyn Error>> {
    let port = port.ok_or("--port is required for this command")?;
    Ok(ApplicationSession::open_usb(port, baud, schema)?)
}

fn resolve_parameter_metadata<'a>(schema: &'a HostSchema, key: &str) -> Result<&'a ParameterMetadata, Box<dyn Error>> {
    if let Some(metadata) = schema.parameter_by_key(key) {
        return Ok(metadata);
    }
    if let Ok(id) = parse_u16(key) {
        return schema.parameter_by_id(id)
            .ok_or_else(|| format!("parameter ID 0x{id:04X} is not present in the loaded schema").into());
    }
    Err(format!("unknown parameter key '{key}' in loaded schema").into())
}

fn resolve_action_metadata<'a>(schema: &'a HostSchema, key: &str) -> Result<&'a ActionMetadata, Box<dyn Error>> {
    if let Some(metadata) = schema.action_by_key(key) {
        return Ok(metadata);
    }
    if let Ok(id) = parse_u16(key) {
        return schema.action_by_id(id)
            .ok_or_else(|| format!("action ID 0x{id:04X} is not present in the loaded schema").into());
    }
    Err(format!("unknown action key '{key}' in loaded schema").into())
}

fn resolve_parameter<'a>(
    schema: &'a HostSchema,
    key: &str,
    type_override: Option<CliParameterType>,
) -> Result<ResolvedParameter<'a>, Box<dyn Error>> {
    let metadata = if let Some(metadata) = schema.parameter_by_key(key) {
        metadata
    } else if let Ok(id) = parse_u16(key) {
        schema.parameter_by_id(id)
            .ok_or_else(|| format!("parameter ID 0x{id:04X} is not present in the loaded schema"))?
    } else {
        return Err(format!("unknown parameter key '{key}' in loaded schema").into());
    };

    let schema_type = metadata.parameter_type()?;
    if let Some(override_type) = type_override {
        let override_type: ParameterType = override_type.into();
        if override_type != schema_type {
            return Err(format!(
                "parameter {} type mismatch: schema={}, CLI={override_type:?}",
                metadata.symbol, metadata.type_name
            ).into());
        }
    }
    Ok(ResolvedParameter { id: metadata.id, ty: schema_type, metadata: Some(metadata) })
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

fn print_schema_info(key: &str, path: &std::path::Path, schema: &HostSchema) {
    println!("key: {key}");
    println!("path: {}", path.display());
    println!("schema_version: {}", schema.schema_version);
    println!("protocol: {}", schema.protocol);
    println!("source_repository: {}", schema.source.repository);
    println!("source_git_sha: {}", schema.source.git_sha);
    println!("parameter_schema: {}", schema.source.parameter_schema);
    println!("parameters: {}", schema.parameters.len());
    println!("actions: {}", schema.actions.len());
}

fn print_parameter_list(schema: &HostSchema) {
    for parameter in &schema.parameters {
        let key = parameter.label.as_str();
        let unit = parameter.unit.as_deref().unwrap_or("");
        println!("0x{:04X}  {:<28} {:<8} {:<2} {}", parameter.id, key, parameter.type_name, parameter.access, unit);
    }
}

fn print_action_list(schema: &HostSchema) {
    for action in &schema.actions {
        let key = action.label.as_str();
        println!("0x{:04X}  {}", action.id, key);
    }
}

fn print_parameter_info(parameter: &ParameterMetadata) {
    println!("symbol: {}", parameter.symbol);
    println!("label: {}", parameter.label);
    println!("id: 0x{:04X}", parameter.id);
    println!("type: {}", parameter.type_name);
    println!("access: {}", parameter.access);
    if let Some(unit) = parameter.unit.as_deref() { println!("unit: {unit}"); }
    if let Some(write_state) = parameter.write_state.as_deref() { println!("write_state: {write_state}"); }
    if let Some(range) = parameter.range.as_ref() {
        if let Some(min) = range.min { println!("min: {}{}", format_schema_number(min), if range.exclusive_min { " (exclusive)" } else { "" }); }
        if let Some(max) = range.max { println!("max: {}{}", format_schema_number(max), if range.exclusive_max { " (exclusive)" } else { "" }); }
        if let Some(symbol) = range.max_symbol.as_deref() { println!("max_symbol: {symbol}"); }
        if let Some(binding) = range.max_binding.as_deref() { println!("max_binding: {binding}"); }
        if !range.max_bindings.is_empty() { println!("max_bindings: {}", range.max_bindings.join(", ")); }
    }
    if !parameter.allowed.is_empty() {
        println!("allowed: {}", parameter.allowed.iter().copied().map(format_schema_number).collect::<Vec<_>>().join(", "));
    }
    if !parameter.allowed_symbols.is_empty() { println!("allowed_symbols: {}", parameter.allowed_symbols.join(", ")); }
    if let Some(scale) = parameter.plot_scale { println!("plot_scale: {scale}"); }
    println!("description: {}", parameter.description);
}

fn print_action_info(action: &ActionMetadata) {
    println!("symbol: {}", action.symbol);
    println!("label: {}", action.label);
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
            let label = metadata.label.as_str();
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
