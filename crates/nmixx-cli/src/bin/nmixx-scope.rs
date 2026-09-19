use std::error::Error;
use std::fs::File;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::Parser;
use nmixx_app::{
    ApplicationSession, DEFAULT_USB_BAUD, DevicePlotCapabilities, HostSchema, MixedScopeConfig,
    MixedScopeSnapshot, ScopeRate, ScopeSelection,
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

    /// Parameter keys selected from runtime FAST capabilities. Omit to list available channels.
    #[arg(num_args = 0..=8)]
    channels: Vec<String>,

    /// Explicitly list runtime Plot capabilities and exit.
    #[arg(long)]
    list: bool,

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
    let app = ApplicationSession::open_usb(&args.port, args.baud, schema)?;

    if args.list || args.channels.is_empty() {
        let capabilities = app.plot_capabilities()?;
        print_capabilities(&capabilities, app.schema());
        if args.channels.is_empty() {
            println!();
            println!("select one or more FAST-capable parameters to start Scope");
        }
        return Ok(());
    }

    let parameter_ids = resolve_parameter_ids(app.schema(), &args.channels)?;
    let selections: Vec<ScopeSelection> = parameter_ids
        .iter()
        .copied()
        .map(|id| ScopeSelection { id, rate: ScopeRate::Fast })
        .collect();
    let config = app.scope_configure(
        &selections,
        Duration::from_secs_f64(args.history),
        args.config_id,
    )?;

    println!("NMIXX Scope");
    println!("port: {} @ {}", args.port, args.baud);
    println!(
        "Scope: {} channel(s), FAST block={}, {:.3} s RAM history",
        config.channels.len(),
        app.plot_capabilities()?.fast_block_samples,
        config.history.as_secs_f64()
    );
    for (index, channel) in config.channels.iter().enumerate() {
        println!(
            "  ch{}: {} (0x{:04X}) {} Hz{}",
            index,
            channel.label,
            channel.id,
            channel.sample_rate_hz,
            channel.unit.as_deref().map(|unit| format!(" {unit}")).unwrap_or_default()
        );
    }
    println!("type 'help' for commands");

    repl(&app)
}

fn print_capabilities(capabilities: &DevicePlotCapabilities, schema: &HostSchema) {
    println!("Plot capabilities");
    println!(
        "FAST:   max_channels={} rate={} Hz block_samples={}",
        capabilities.fast_max_channels,
        capabilities.fast_rate_hz,
        capabilities.fast_block_samples
    );
    println!(
        "NORMAL: max_channels={} rate={} Hz",
        capabilities.normal_max_channels,
        capabilities.normal_rate_hz
    );
    println!();
    println!("ID      Variable                     FAST       NORMAL  Unit");
    println!("---------------------------------------------------------------");
    for channel in capabilities.with_schema(schema) {
        let label = channel
            .label
            .unwrap_or_else(|| format!("0x{:04X}", channel.id));
        let fast = match channel.fast_scale {
            Some(scale) => format!("yes/{scale}"),
            None => "-".to_owned(),
        };
        println!(
            "0x{:04X}  {:<28} {:<10} {:<7} {}",
            channel.id,
            label,
            fast,
            if channel.supports_normal { "yes" } else { "-" },
            channel.unit.as_deref().unwrap_or("")
        );
    }
}

fn resolve_parameter_ids(schema: &HostSchema, keys: &[String]) -> Result<Vec<u16>, Box<dyn Error>> {
    let mut ids = Vec::with_capacity(keys.len());
    for key in keys {
        if let Some(parameter) = schema.parameter_by_key(key) {
            ids.push(parameter.id);
            continue;
        }
        if let Ok(id) = parse_u16(key) {
            ids.push(id);
            continue;
        }
        return Err(format!("unknown Scope parameter '{key}'").into());
    }
    Ok(ids)
}

fn repl(app: &ApplicationSession) -> Result<(), Box<dyn Error>> {
    let stdin = io::stdin();
    let mut lines = stdin.lock().lines();

    loop {
        print!("scope> ");
        io::stdout().flush()?;
        let Some(line) = lines.next() else { break; };
        let line = line?;
        let mut words = line.split_whitespace();
        let Some(command) = words.next() else { continue; };

        match command.to_ascii_lowercase().as_str() {
            "help" | "?" => print_help(),
            "live" | "resume" => {
                app.scope_resume()?;
                println!("LIVE");
            }
            "pause" => {
                app.scope_pause()?;
                println!("PAUSED");
            }
            "stop" => {
                app.scope_stop()?;
                println!("STOPPED");
            }
            "clear" => {
                app.scope_clear()?;
                println!("buffer cleared");
            }
            "capture" => {
                let text = words.next().ok_or("capture requires a duration, e.g. capture 2s")?;
                if words.next().is_some() {
                    return Err("capture accepts exactly one duration".into());
                }
                let duration = parse_duration(text)?;
                app.scope_capture(duration)?;
                println!("CAPTURING {:.3} s", duration.as_secs_f64());
            }
            "status" => print_status(app)?,
            "export" => {
                let path = words.next().ok_or("export requires a file path")?;
                if words.next().is_some() {
                    return Err("export accepts exactly one file path".into());
                }
                export_csv(app, Path::new(path))?;
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

fn print_status(app: &ApplicationSession) -> Result<(), Box<dyn Error>> {
    let status = app.scope_status()?;
    let config = app.scope_config()?;
    println!("state: {:?}", status.state);
    println!("stored samples: {} / {}", status.samples, status.capacity_samples);
    println!("history capacity: {:.3} s per active group", config.history.as_secs_f64());
    println!("lost frames: {}", status.lost_frames);
    Ok(())
}

fn export_csv(app: &ApplicationSession, path: &Path) -> Result<(), Box<dyn Error>> {
    let config = app.scope_config()?;
    let snapshot = app.scope_snapshot_tail(config.history)?;
    write_snapshot_csv(&snapshot, &config, path)?;
    let samples: usize = snapshot.series.iter().map(|series| series.values.len()).sum();
    println!("exported {} mixed-rate samples -> {}", samples, path.display());
    Ok(())
}

fn write_snapshot_csv(
    snapshot: &MixedScopeSnapshot,
    config: &MixedScopeConfig,
    path: &Path,
) -> Result<(), Box<dyn Error>> {
    let mut file = File::create(path)?;
    writeln!(file, "channel_id,label,sample_rate_hz,time_s,value")?;

    for series in &snapshot.series {
        let symbol = config
            .channels
            .iter()
            .find(|channel| channel.id == series.id)
            .map(|channel| channel.label.as_str())
            .unwrap_or("unknown");
        let count = series.values.len();
        for (index, value) in series.values.iter().enumerate() {
            let time = (index as f64 - count.saturating_sub(1) as f64)
                / f64::from(series.sample_rate_hz);
            writeln!(
                file,
                "0x{:04X},{},{},{:.9},{:.9}",
                series.id,
                symbol,
                series.sample_rate_hz,
                time,
                value
            )?;
        }
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
