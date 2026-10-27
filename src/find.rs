use std::fs::{File, OpenOptions, Permissions};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use anyhow::{bail, Context, Result};
use hound::{SampleFormat, WavSpec, WavWriter};

pub struct Finder {
    pid_file: PathBuf,
    sink_file: PathBuf,
    wav_file: PathBuf,
    players_file: PathBuf,
}

impl Finder {
    pub fn new() -> Self {
        let base_dir = std::env::var("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                std::env::var("XDG_STATE_HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|_| crate::pbp::user_home_dir().join(".local/state"))
            });
        let runtime_dir = base_dir.join("pixel-buds");
        crate::pbp::ensure_private_state_dir(&runtime_dir);

        Self {
            pid_file: runtime_dir.join("pixel-buds-find.pid"),
            sink_file: runtime_dir.join("pixel-buds-find.sink"),
            wav_file: runtime_dir.join("pixel-buds-find.wav"),
            players_file: runtime_dir.join("pixel-buds-find.players"),
        }
    }

    pub fn stop(&self) {
        if self.pid_file.exists() {
            if let Ok(content) = std::fs::read_to_string(&self.pid_file) {
                if let Ok(pid) = content.trim().parse::<i32>() {
                    unsafe {
                        libc::kill(pid, libc::SIGTERM);
                    }
                    let _ = Command::new("pkill")
                        .args(["-P", &pid.to_string()])
                        .output();
                }
            }
            let _ = std::fs::remove_file(&self.pid_file);
        }

        let _ = Command::new("pkill")
            .args(["-f", "pixel-buds-find-tone"])
            .output();

        let _ = std::fs::remove_file(&self.sink_file);
        let _ = std::fs::remove_file(&self.wav_file);

        self.resume_playback();
    }

    fn pause_playback(&self) {
        let Ok(output) = Command::new("busctl")
            .args(["--user", "list", "--no-legend"])
            .output() else { return; };

        let text = String::from_utf8_lossy(&output.stdout);
        let mut paused_players = Vec::new();

        for line in text.lines() {
            let dest = line.split_whitespace().next().unwrap_or("");
            if !dest.starts_with("org.mpris.MediaPlayer2.") {
                continue;
            }

            let status_out = Command::new("busctl")
                .args([
                    "--user", "get-property", dest,
                    "/org/mpris/MediaPlayer2",
                    "org.mpris.MediaPlayer2.Player", "PlaybackStatus",
                ])
                .output();

            if let Ok(out) = status_out {
                let status_str = String::from_utf8_lossy(&out.stdout);
                if status_str.contains("Playing") {
                    paused_players.push(dest.to_string());
                    let _ = Command::new("busctl")
                        .args([
                            "--user", "call", dest,
                            "/org/mpris/MediaPlayer2",
                            "org.mpris.MediaPlayer2.Player", "Pause",
                        ])
                        .output();
                }
            }
        }

        if !paused_players.is_empty() {
            let mut file = OpenOptions::new()
                .create(true)
                .append(true)
                .mode(0o600)
                .open(&self.players_file)
                .ok();

            if let Some(f) = file.as_mut() {
                let _ = f.set_permissions(Permissions::from_mode(0o600));
                for p in paused_players {
                    let _ = writeln!(f, "{}", p);
                }
            }
        }
    }

    fn resume_playback(&self) {
        if !self.players_file.exists() {
            return;
        }

        if let Ok(file) = File::open(&self.players_file) {
            let reader = BufReader::new(file);
            for line in reader.lines().flatten() {
                let dest = line.trim();
                if !dest.is_empty() {
                    let _ = Command::new("busctl")
                        .args([
                            "--user", "call", dest,
                            "/org/mpris/MediaPlayer2",
                            "org.mpris.MediaPlayer2.Player", "Play",
                        ])
                        .output();
                }
            }
        }

        let _ = std::fs::remove_file(&self.players_file);
    }

    fn find_sink(&self) -> Result<String> {
        let output = Command::new("pactl")
            .args(["list", "sinks"])
            .output()
            .context("Failed to run pactl list sinks")?;

        let text = String::from_utf8_lossy(&output.stdout);
        let mut current_name = String::new();
        let mut fallback_sink: Option<String> = None;

        for line in text.lines() {
            let line = line.trim();
            if line.starts_with("Name:") {
                current_name = line.trim_start_matches("Name:").trim().to_string();
                if current_name.starts_with("bluez_output.") && fallback_sink.is_none() {
                    fallback_sink = Some(current_name.clone());
                }
            } else if line.starts_with("Description:") {
                let desc = line.to_lowercase();
                if current_name.starts_with("bluez_output.") && (desc.contains("pixel buds") || desc.contains("buds")) {
                    return Ok(current_name);
                }
            }
        }

        if let Some(fb) = fallback_sink {
            return Ok(fb);
        }

        bail!("Pixel Buds audio sink not found");
    }

    fn generate_wav(&self, side: &str) -> Result<()> {
        let spec = WavSpec {
            channels: 2,
            sample_rate: 48000,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };

        let mut writer = WavWriter::create(&self.wav_file, spec)
            .context("Failed to create find tone wav file")?;

        let sr = 48000;
        let dur = 15;
        let n = sr * dur;
        let period = sr / 6;
        let on = (sr as f32 * 0.08) as usize;

        for i in 0..n {
            let s = if (i % period) >= on {
                0i16
            } else {
                let t = i as f32 / sr as f32;
                let freq = 3000.0 + 400.0 * (2.0 * std::f32::consts::PI * 12.0 * t).sin();
                (0.85 * (2.0 * std::f32::consts::PI * freq * t).sin() * 32767.0) as i16
            };

            match side {
                "left" => {
                    writer.write_sample(s)?;
                    writer.write_sample(0i16)?;
                }
                "right" => {
                    writer.write_sample(0i16)?;
                    writer.write_sample(s)?;
                }
                _ => {
                    writer.write_sample(s)?;
                    writer.write_sample(s)?;
                }
            }
        }

        writer.finalize().context("Failed to finalize wav writer")?;
        let _ = std::fs::set_permissions(&self.wav_file, Permissions::from_mode(0o600));
        Ok(())
    }

    pub fn start(&self, side: &str) -> Result<()> {
        let side = side.to_lowercase();
        if side != "left" && side != "right" && side != "both" {
            bail!("Usage: find left|right|both|stop");
        }

        let sink = self.find_sink()?;
        self.stop();
        self.pause_playback();

        let _ = crate::pbp::write_private_atomic(&self.sink_file, sink.as_bytes());
        self.generate_wav(&side)?;

        let mut child = Command::new("paplay")
            .arg(format!("--device={}", sink))
            .arg("--stream-name=pixel-buds-find-tone")
            .arg(&self.wav_file)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context("Failed to run paplay")?;

        let pid = child.id();
        let _ = crate::pbp::write_private_atomic(&self.pid_file, pid.to_string().as_bytes());

        // Wait for child in background or synchronously
        let _ = child.wait();
        self.stop();

        Ok(())
    }
}
