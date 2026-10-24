use std::ffi::CStr;
use std::path::{Path, PathBuf};
use std::process::Command;
use anyhow::{bail, Context, Result};
use crate::lock::FileLock;
use crate::types::{BatteryInfo, CaseBattery, CaseCache, PodBattery, SettingsInfo};

pub fn user_home_dir() -> PathBuf {
    if let Some(home) = std::env::var_os("HOME") {
        if !home.is_empty() {
            return PathBuf::from(home);
        }
    }
    unsafe {
        let pw = libc::getpwuid(libc::getuid());
        if !pw.is_null() && !(*pw).pw_dir.is_null() {
            if let Ok(dir) = CStr::from_ptr((*pw).pw_dir).to_str() {
                if !dir.is_empty() {
                    return PathBuf::from(dir);
                }
            }
        }
    }
    if let Ok(user) = std::env::var("USER") {
        if !user.is_empty() {
            return PathBuf::from("/home").join(user);
        }
    }
    PathBuf::from(".")
}

pub struct PbpClient {
    pbpctrl_bin: PathBuf,
    lock_path: PathBuf,
    case_cache_path: PathBuf,
}

impl PbpClient {
    pub fn new(state_dir: &Path) -> Self {
        let pbpctrl_bin = which::which("pbpctrl")
            .unwrap_or_else(|_| user_home_dir().join(".cargo/bin/pbpctrl"));

        let lock_path = state_dir.join("pixel-buds.lock");
        let case_cache_path = state_dir.join("case_cache.json");

        Self {
            pbpctrl_bin,
            lock_path,
            case_cache_path,
        }
    }

    pub fn load_case_cache(&self) -> Option<CaseCache> {
        if !self.case_cache_path.exists() {
            return None;
        }
        let data = std::fs::read_to_string(&self.case_cache_path).ok()?;
        serde_json::from_str(&data).ok()
    }

    pub const CASE_CACHE_MAX_AGE_SECS: u64 = 4 * 3600; // 4 hours

    pub fn resolve_cached_case(&self, now: u64) -> CaseBattery {
        if let Some(cached) = self.load_case_cache() {
            let age = now.saturating_sub(cached.timestamp);
            if cached.level >= 0 && age < Self::CASE_CACHE_MAX_AGE_SECS {
                CaseBattery {
                    level: cached.level,
                    charging: cached.charging,
                    last_known: true,
                    timestamp: cached.timestamp,
                    stale: false,
                }
            } else {
                CaseBattery {
                    level: -1,
                    charging: false,
                    last_known: true,
                    timestamp: cached.timestamp,
                    stale: true,
                }
            }
        } else {
            CaseBattery {
                level: -1,
                charging: false,
                last_known: false,
                timestamp: 0,
                stale: false,
            }
        }
    }

    pub fn save_case_cache(&self, level: i32, charging: bool) {
        if level < 0 {
            return;
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let cache = CaseCache {
            level,
            charging,
            timestamp: now,
        };

        if let Ok(json) = serde_json::to_string_pretty(&cache) {
            let tmp_path = self.case_cache_path.with_extension("tmp");
            if std::fs::write(&tmp_path, json).is_ok() {
                let _ = std::fs::rename(&tmp_path, &self.case_cache_path);
            }
        }
    }

    pub fn run_locked(&self, args: &[&str]) -> Result<(String, i32)> {
        let _lock = FileLock::acquire(&self.lock_path)?;
        let output = Command::new(&self.pbpctrl_bin)
            .args(args)
            .output()
            .with_context(|| format!("Failed to execute {:?}", self.pbpctrl_bin))?;

        let code = output.status.code().unwrap_or(1);
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok((stdout, code))
    }

    pub fn find_connected_buds(&self) -> Option<(String, String)> {
        let output = Command::new("bluetoothctl")
            .args(["devices", "Connected"])
            .output()
            .ok()?;

        let text = String::from_utf8_lossy(&output.stdout);
        for line in text.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 3 && parts[0] == "Device" {
                let mac = parts[1];
                let name = parts[2..].join(" ");
                if name.to_lowercase().contains("pixel buds") {
                    return Some((mac.to_string(), name));
                }
            }
        }
        None
    }

    pub fn get_bluez_battery(&self, mac: &str) -> Option<i32> {
        let output = Command::new("bluetoothctl")
            .args(["info", mac])
            .output()
            .ok()?;

        let text = String::from_utf8_lossy(&output.stdout);
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with("Battery Percentage:") {
                // e.g. "Battery Percentage: 0x64 (100)"
                if let Some(start) = line.rfind('(') {
                    if let Some(end) = line.rfind(')') {
                        if start < end {
                            let num_str = &line[start + 1..end];
                            if let Ok(lvl) = num_str.parse::<i32>() {
                                return Some(lvl);
                            }
                        }
                    }
                }
            }
        }
        None
    }

    pub fn parse_runtime(&self, text: &str) -> BatteryInfo {
        let mut left = PodBattery::default();
        let mut right = PodBattery::default();
        let mut case = CaseBattery::default();

        let mut section: Option<&str> = None;

        for line in text.lines() {
            let s = line.trim();
            if s == "battery:" {
                section = Some("battery");
                continue;
            } else if s == "placement:" {
                section = Some("placement");
                continue;
            } else if s.ends_with(':') {
                section = None;
                continue;
            }

            if let Some(sec) = section {
                if let Some((k, v)) = s.split_once(':') {
                    let k = k.trim().to_lowercase();
                    let v = v.trim().to_lowercase();

                    if sec == "battery" {
                        let mut level = -1;
                        if let Some(pos) = v.find('%') {
                            let num_part = v[..pos].trim();
                            if let Some(start) = num_part.rfind(' ') {
                                if let Ok(l) = num_part[start + 1..].parse::<i32>() {
                                    level = l;
                                }
                            } else if let Ok(l) = num_part.parse::<i32>() {
                                level = l;
                            }
                        }

                        let is_charging = v.contains("charging") && !v.contains("not charging");

                        if k.contains("left") {
                            left.level = level;
                            left.charging = is_charging;
                        } else if k.contains("right") {
                            right.level = level;
                            right.charging = is_charging;
                        } else if k.contains("case") {
                            case.level = level;
                            case.charging = is_charging;
                        }
                    } else if sec == "placement" {
                        let in_case = v.contains("in case");
                        let in_ear = v.contains("out of case");

                        if k.contains("left") {
                            left.in_case = in_case;
                            left.in_ear = in_ear;
                        } else if k.contains("right") {
                            right.in_case = in_case;
                            right.in_ear = in_ear;
                        }
                    }
                }
            }
        }

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        if case.level >= 0 {
            self.save_case_cache(case.level, case.charging);
            case.last_known = false;
            case.timestamp = now;
            case.stale = false;
        } else {
            case = self.resolve_cached_case(now);
        }

        BatteryInfo {
            left,
            right,
            case,
            combined: -1,
        }
    }

    pub fn read_status(&self, mac: &str, name: &str, cached_settings: Option<&SettingsInfo>) -> Result<crate::types::PluginStatus> {
        let (runtime_out, rc) = self.run_locked(&["show", "runtime"])?;
        let combined = self.get_bluez_battery(mac).unwrap_or(-1);

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let (battery, noise_mode, settings) = if rc != 0 {
            let case = self.resolve_cached_case(now);

            let batt = BatteryInfo {
                left: PodBattery { level: combined, charging: false, in_ear: true, in_case: false },
                right: PodBattery { level: combined, charging: false, in_ear: true, in_case: false },
                case,
                combined,
            };

            let sets = cached_settings.cloned().unwrap_or(SettingsInfo {
                speech_detection: true,
                ohd: true,
                multipoint: true,
                gestures: true,
                volume_eq: true,
            });

            (batt, "active".to_string(), sets)
        } else {
            let mut batt = self.parse_runtime(&runtime_out);
            batt.combined = combined;
            if batt.left.level < 0 && combined >= 0 {
                batt.left.level = combined;
            }
            if batt.right.level < 0 && combined >= 0 {
                batt.right.level = combined;
            }

            let (anc_out, _) = self.run_locked(&["get", "anc"]).unwrap_or_else(|_| ("active".to_string(), 0));
            let anc_mode = if anc_out.is_empty() { "active".to_string() } else { anc_out.to_lowercase() };

            let sets = if let Some(s) = cached_settings {
                s.clone()
            } else {
                let (speech, _) = self.run_locked(&["get", "speech-detection"]).unwrap_or_default();
                let (ohd, _) = self.run_locked(&["get", "ohd"]).unwrap_or_default();
                let (multi, _) = self.run_locked(&["get", "multipoint"]).unwrap_or_default();
                let (gest, _) = self.run_locked(&["get", "gestures"]).unwrap_or_default();
                let (veq, _) = self.run_locked(&["get", "volume-eq"]).unwrap_or_default();

                SettingsInfo {
                    speech_detection: speech.trim() == "true",
                    ohd: ohd.trim() == "true",
                    multipoint: multi.trim() == "true",
                    gestures: gest.trim() == "true",
                    volume_eq: veq.trim() == "true",
                }
            };

            (batt, anc_mode, sets)
        };

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Ok(crate::types::PluginStatus {
            ok: true,
            connected: true,
            device_name: name.to_string(),
            model_name: "Google Pixel Buds Pro 2".to_string(),
            mac: mac.to_string(),
            battery,
            noise_mode,
            settings,
            last_updated: now,
        })
    }

    pub fn disconnected_status(&self) -> crate::types::PluginStatus {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let case = self.resolve_cached_case(now);

        crate::types::PluginStatus {
            ok: true,
            connected: false,
            device_name: "Pixel Buds Pro 2".to_string(),
            model_name: "Google Pixel Buds Pro 2".to_string(),
            mac: String::new(),
            battery: BatteryInfo {
                left: PodBattery::default(),
                right: PodBattery::default(),
                case,
                combined: -1,
            },
            noise_mode: "off".to_string(),
            settings: SettingsInfo::default(),
            last_updated: now,
        }
    }

    pub fn set_anc(&self, mode: &str) -> Result<()> {
        let mode = mode.to_lowercase();
        if mode != "active" && mode != "aware" && mode != "off" {
            bail!("Invalid ANC mode: {}", mode);
        }
        let (out, rc) = self.run_locked(&["set", "anc", &mode])?;
        if rc != 0 {
            bail!("Failed to set ANC mode: {}", out);
        }
        Ok(())
    }

    pub fn cycle_anc(&self) -> Result<String> {
        let (_, rc) = self.run_locked(&["set", "anc", "cycle-next"])?;
        if rc != 0 {
            bail!("Failed to cycle ANC");
        }
        let (anc_out, _) = self.run_locked(&["get", "anc"])?;
        Ok(anc_out.trim().to_lowercase())
    }

    pub fn set_setting(&self, key: &str, val: bool) -> Result<()> {
        let pbp_key = match key {
            "speech_detection" | "speech-detection" => "speech-detection",
            "ohd" => "ohd",
            "multipoint" => "multipoint",
            "gestures" => "gestures",
            "volume_eq" | "volume-eq" => "volume-eq",
            _ => bail!("Unknown setting: {}", key),
        };

        let val_str = if val { "true" } else { "false" };
        let (out, rc) = self.run_locked(&["set", pbp_key, val_str])?;
        if rc != 0 {
            bail!("Failed to set {}: {}", pbp_key, out);
        }
        Ok(())
    }
}
