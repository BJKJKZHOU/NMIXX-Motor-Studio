use std::error::Error;
use std::fs::File;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::Parser;
use nmixx_app::{
    DEFAULT_USB_BAUD, DeviceSession, HostSchema, ScopeChannel, ScopeConfig, ScopeSession,
    StreamSnapshot, StreamState,
};

#[derive(Debug, Parser)]
#[command(
    name = "nmixx-scope",
    about = "Interactive RAM Scope client for NMIXX Motor Studio"
)]
struct Args {
    /// AxDr_L USB CDC serial port, e.g. /dev/ttyACM1 or COM7.
    #[arg(long)]
    port: String,

    /// Firmware Host schema exported from Parameter/parameter.yaml.
    #[arg(long)]
    schema: PathBuf,

    /// FAST Plot parameter keys. Maximum 8 channels.
    #[arg(required = true, num_args = 1..=8)]
    channels: Vec<String>,

    /// FAST sample rate used by the firmware.
    #[arg(long, default_value_t = 20_000)]
    sample_rate: u32,

    /// RAM rolling-history capacity in seconds.
    #[arg(long, default_value_t = 10.0)]
    history: f64,

    /// Plot Config_ID used for this Scope session.
    #[arg(long, default_value_t = 1)]
    config_id: u8,

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
    if !args.history.is_finite() || args.history <= 0.0 {
        return Err("--history must be a positive finite number of seconds".into());
    }

    let schema = HostSchema::load(&args.schema)?;
    let channels = resolve_channels(&schema, &args.channels)?;
    let session = DeviceSession::open_usb(&args.port, args.baud)?;
    let scope = ScopeSession::new(
        session,
        ScopeConfig {
            config_id: args.config_id,
            sample_rate_hz: args.sample_rate,
            history: Duration::from_secs_f64(args.history),
            channels,
        },
    )?;

    println!("NMIXX Scope");
    println!("port: {} @ {}", args.port, args.baud);
    println!(
        "FAST: {} channel(s) @ {} Hz, {:.3} s RAM history",
        scope.config().channels.len(),
        scope.config().sample_rate_hz,
        scope.config().history.as_secs_f64()
    );
    for (index, channel) in scope.config().channels.iter().enumerate() {
        println!(
            "  ch{}: {} (0x{:04X}) scale={}{}",
            index,
            channel.symbol,
            channel.id,
            channel.scale,
            channel
                .unit
                .as_deref()
                .map(|unit| format!(" {unit}"))
                .unwrap_or_default()
        );
    }
    println!("type 'help' for commands");

    repl(&scope)
}

fn resolve_channels(schema: &HostSchema, keys: &[String]) -> Result<Vec<ScopeChannel>, Box<dyn Error>> {
    let mut channels = Vec::with_capacity(keys.len());
    for key in keys {
        let parameter = schema
            .parameter_by_key(key)
            .or_else(|| parse_u16(key).ok().and_then(|id| schema.parameter_by_id(id)))
            .ok_or_else(|| format!("unknown Scope parameter '{key}'"))?;
        let scale = parameter
            .plot_scale
            .ok_or_else(|| format!("{} has no plot_scale in HostSchema", parameter.symbol))?;
        if !scale.is_finite() || scale <= 0.0 || scale > f32::MAX as f64 {
            return Err(format!("{} has invalid FAST plot_scale {scale}", parameter.symbol).into());
        }
        channels.push(ScopeChannel {
            id: parameter.id,
            symbol: parameter.symbol.clone(),
            unit: parameter.unit.clone(),
            scale: scale as f32,
        });
    }
    Ok(channels)
}

fn repl(scope: &ScopeSession) -> Result<(), Box<dyn Error>> {
    let stdin = io::stdin();
    let mut lines = stdin.lock().lines();

    loop {
        print!("scope> ");
        io::stdout().flush()?;
        let Some(line) = lines.next() else {
            break;
        };
        let line = line?;
        let mut words = line.split_whitespace();
        let Some(command) = words.next() else {
            continue;
        };

        match command.to_ascii_lowercase().as_str() {
            "help" | "?" => print_help(),
            "live" | "resume" => {
                scope.resume()?;
                println!("LIVE");
            }
            "pause" => {
                scope.pause()?;
                println!("PAUSED");
            }
            "stop" => {
                scope.stop()?;
                println!("STOPPED");
            }
            "clear" => {
                scope.clear()?;
                println!("buffer cleared");
            }
            "capture" => {
                let text = words.next().ok_or("capture requires a duration, e.g. capture 2s")?;
                if words.next().is_some() {
                    return Err("capture accepts exactly one duration".into());
                }
                let duration = parse_duration(text)?;
                scope.capture(duration)?;
                println!("CAPTURING {:.3} s", duration.as_secs_f64());
            }
            "status" => print_status(scope)?,
            "export" => {
                let path = words.next().ok_or("export requires a file path")?;
                if words.next().is_some() {
                    return Err("export accepts exactly one file path".into());
                }
                export_csv(scope, Path::new(path))?;
            }
            "quit" | "exit" => break,
            other => println!("unknown command '{other}'; type 'help'"),
        }
    }

    Ok(())
}

fn print_help() {
    println!("live | resume       start rolling RAM acquisition");
    println!("pause               stop firmware Plot and freeze RAM buffer");
    println!("stop                stop firmware Plot and set Scope STOPPED");
    println!("clear               clear RAM history without changing state");
    println!("capture <duration>  clear, acquire finite interval, auto-pause (e.g. 2s, 500ms)");
    println!("status              show state, samples, duration, capacity and frame loss");
    println!("export <file.csv>   export current ordered RAM snapshot");
    println!("quit | exit         stop Plot and close Scope");
}

fn print_status(scope: &ScopeSession) -> Result<(), Box<dyn Error>> {
    let status = scope.status()?;
    let duration = status.samples as f64 / f64::from(scope.config().sample_rate_hz);
    let capacity_s = status.capacity_samples as f64 / f64::from(scope.config().sample_rate_hz);
    println!("state: {:?}", status.state);
    println!("samples: {} / {}", status.samples, status.capacity_samples);
    println!("duration: {:.6} / {:.3} s", duration, capacity_s);
    println!("lost frames: {}", status.lost_frames);
    Ok(())
}

fn export_csv(scope: &ScopeSession, path: &Path) -> Result<(), Box<dyn Error>> {
    let snapshot = scope.snapshot()?;
    write_snapshot_csv(&snapshot, scope.config(), path)?;
    println!(
        "exported {} samples ({:.6} s) -> {}",
        snapshot.sample_count(),
        snapshot.duration().as_secs_f64(),
        path.display()
    );
    Ok(())
}

fn write_snapshot_csv(
    snapshot: &StreamSnapshot,
    config: &ScopeConfig,
    path: &Path,
) -> Result<(), Box<dyn Error>> {
    let mut file = File::create(path)?;
    write!(file, "time_s")?;
    for channel in &config.channels {
        write!(file, ",{}", channel.symbol)?;
    }
    writeln!(file)?;

    let sample_rate = f64::from(config.sample_rate_hz);
    for index in 0..snapshot.sample_count() {
        write!(file, "{:.9}", index as f64 / sample_rate)?;
        let sample = snapshot.sample(index).ok_or("snapshot sample indexing failed")?;
        for value in sample {
            write!(file, ",{value:.9}")?;
        }
        writeln!(file)?;
    }
    Ok(())
}

fn parse_duration(text: &str) -> Result<Duration, Box<dyn Error>> {
    let (number, scale) = if let Some(value) = text.strip_suffix("ms") {
        (value, 0.001)
    } else if let Some(value) = text.strip_suffix('s') {
        (value, 1.0)
    } else {
        (text, 1.0)
    };
    let value: f64 = number.parse()?;
    let seconds = value * scale;
    if !seconds.is_finite() || seconds <= 0.0 {
        return Err("duration must be positive and finite".into());
    }
    Ok(Duration::from_secs_f64(seconds))
}

fn parse_u16(text: &str) -> Result<u16, Box<dyn Error>> {
    if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        Ok(u16::from_str_radix(hex, 16)?)
    } else {
        Ok(text.parse()?)
    }
}

#[allow(dead_code)]
fn _state_name(state: StreamState) -> &'static str {
    match state {
        StreamState::Stopped => "STOPPED",
        StreamState::Live => "LIVE",
        StreamState::Capturing => "CAPTURING",
        StreamState::Paused => "PAUSED",
    }
}
