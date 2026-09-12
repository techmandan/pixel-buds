mod daemon;
mod find;
mod lock;
mod pbp;
mod types;

use clap::{Parser, Subcommand};
use anyhow::Result;
use daemon::Daemon;
use find::Finder;

#[derive(Parser)]
#[command(name = "pixel-buds")]
#[command(author = "Tech Man Dan <dan.inman@hotmail.co.uk>")]
#[command(version = "0.1.0")]
#[command(about = "Native Rust status bridge and controller for Google Pixel Buds Pro 2 on Omarchy")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Run the background status synchronization daemon
    Daemon,
    /// Run the background status synchronization daemon (alias)
    Run,
    /// Poll device status once and output JSON
    Poll,
    /// Set ANC mode or feature settings
    Set {
        /// "anc" or a setting name (e.g. speech_detection, ohd, multipoint, gestures, volume_eq)
        target: String,
        /// value to set (e.g. active/aware/off or true/false)
        value: String,
    },
    /// Cycle ANC mode between active, aware, and off
    CycleAnc,
    /// Play locating chime on earbuds (left, right, both, or stop)
    Find {
        #[arg(default_value = "both")]
        side: String,
    },
    /// Stop locating chime and restore audio playback
    StopFind,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let daemon = Daemon::new();

    match cli.command {
        None | Some(Commands::Daemon) | Some(Commands::Run) => {
            daemon.run_loop()?;
        }
        Some(Commands::Poll) => {
            let status = daemon.poll_once()?;
            println!("{}", serde_json::to_string_pretty(&status)?);
        }
        Some(Commands::Set { target, value }) => {
            let target_lower = target.to_lowercase();
            if target_lower == "anc" {
                daemon.client().set_anc(&value)?;
                if let Some(mut curr) = daemon.read_status_file() {
                    curr.noise_mode = value.to_lowercase();
                    let _ = daemon.write_status(&curr);
                }
                println!("ANC set to {}", value);
            } else {
                let val_bool = matches!(value.to_lowercase().as_str(), "true" | "1" | "yes" | "on");
                daemon.client().set_setting(&target, val_bool)?;
                if let Some(mut curr) = daemon.read_status_file() {
                    match target.replace('-', "_").as_str() {
                        "speech_detection" => curr.settings.speech_detection = val_bool,
                        "ohd" => curr.settings.ohd = val_bool,
                        "multipoint" => curr.settings.multipoint = val_bool,
                        "gestures" => curr.settings.gestures = val_bool,
                        "volume_eq" => curr.settings.volume_eq = val_bool,
                        _ => {}
                    }
                    let _ = daemon.write_status(&curr);
                }
                println!("Set {} to {}", target, val_bool);
            }
        }
        Some(Commands::CycleAnc) => {
            let new_mode = daemon.client().cycle_anc()?;
            if let Some(mut curr) = daemon.read_status_file() {
                curr.noise_mode = new_mode.clone();
                let _ = daemon.write_status(&curr);
            }
            println!("ANC cycled to {}", new_mode);
        }
        Some(Commands::Find { side }) => {
            let finder = Finder::new();
            if side.to_lowercase() == "stop" {
                finder.stop();
                println!("Finder tone stopped");
            } else {
                finder.start(&side)?;
            }
        }
        Some(Commands::StopFind) => {
            let finder = Finder::new();
            finder.stop();
            println!("Finder tone stopped");
        }
    }

    Ok(())
}
