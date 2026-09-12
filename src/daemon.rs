use std::path::PathBuf;
use std::thread::sleep;
use std::time::{Duration, Instant};
use anyhow::Result;
use crate::pbp::PbpClient;
use crate::types::{PluginStatus, SettingsInfo};

const POLL_CONNECTED: Duration = Duration::from_millis(3000);
const POLL_IDLE: Duration = Duration::from_millis(5000);
const SETTINGS_POLL_INTERVAL: Duration = Duration::from_secs(15);

pub struct Daemon {
    _state_dir: PathBuf,
    status_path: PathBuf,
    client: PbpClient,
}

impl Daemon {
    pub fn new() -> Self {
        let state_home = std::env::var("XDG_STATE_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                let home = std::env::var("HOME").unwrap_or_else(|_| "/home/dan".to_string());
                PathBuf::from(home).join(".local/state")
            });

        let state_dir = state_home.join("pixel-buds");
        let _ = std::fs::create_dir_all(&state_dir);
        let status_path = state_dir.join("status.json");
        let client = PbpClient::new(&state_dir);

        Self {
            _state_dir: state_dir,
            status_path,
            client,
        }
    }

    pub fn client(&self) -> &PbpClient {
        &self.client
    }

    pub fn write_status(&self, status: &PluginStatus) -> Result<()> {
        let json = serde_json::to_string_pretty(status)?;
        let tmp_path = self.status_path.with_extension("tmp");
        std::fs::write(&tmp_path, json)?;
        std::fs::rename(&tmp_path, &self.status_path)?;
        Ok(())
    }

    pub fn read_status_file(&self) -> Option<PluginStatus> {
        let data = std::fs::read_to_string(&self.status_path).ok()?;
        serde_json::from_str(&data).ok()
    }

    pub fn poll_once(&self) -> Result<PluginStatus> {
        let status = if let Some((mac, name)) = self.client.find_connected_buds() {
            self.client.read_status(&mac, &name, None)?
        } else {
            self.client.disconnected_status()
        };
        self.write_status(&status)?;
        Ok(status)
    }

    pub fn run_loop(&self) -> Result<()> {
        let mut cached_settings: Option<SettingsInfo> = None;
        let mut last_settings_poll = Instant::now() - SETTINGS_POLL_INTERVAL;

        loop {
            if let Some((mac, name)) = self.client.find_connected_buds() {
                let now = Instant::now();
                if now.duration_since(last_settings_poll) > SETTINGS_POLL_INTERVAL {
                    cached_settings = None;
                    last_settings_poll = now;
                }

                match self.client.read_status(&mac, &name, cached_settings.as_ref()) {
                    Ok(status) => {
                        cached_settings = Some(status.settings.clone());
                        let _ = self.write_status(&status);
                    }
                    Err(_) => {
                        let fallback = self.client.disconnected_status();
                        let _ = self.write_status(&fallback);
                    }
                }
                sleep(POLL_CONNECTED);
            } else {
                cached_settings = None;
                let status = self.client.disconnected_status();
                let _ = self.write_status(&status);
                sleep(POLL_IDLE);
            }
        }
    }
}
