use std::ffi::CStr;
use std::path::{Path, PathBuf};
use std::process::Command;
use anyhow::{bail, Context, Result};
use crate::lock::FileLock;
use crate::types::{
    BatteryInfo, CaseBattery, CaseCache, HearingWellness, HearingWellnessCache, PodBattery,
    SettingsInfo,
};

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
    hearing_cache_path: PathBuf,
}

impl PbpClient {
    pub fn new(state_dir: &Path) -> Self {
        let pbpctrl_bin = which::which("pbpctrl")
            .unwrap_or_else(|_| user_home_dir().join(".cargo/bin/pbpctrl"));

        let lock_path = state_dir.join("pixel-buds.lock");
        let case_cache_path = state_dir.join("case_cache.json");
        let hearing_cache_path = state_dir.join("hearing_wellness.json");

        Self {
            pbpctrl_bin,
            lock_path,
            case_cache_path,
            hearing_cache_path,
        }
    }

    pub fn load_hearing_cache(&self, now: u64) -> HearingWellnessCache {
        let day_key = now / 86400;
        if self.hearing_cache_path.exists() {
            if let Ok(data) = std::fs::read_to_string(&self.hearing_cache_path) {
                if let Ok(cached) = serde_json::from_str::<HearingWellnessCache>(&data) {
                    if cached.day_key == day_key {
                        return cached;
                    }
                }
            }
        }
        HearingWellnessCache {
            day_key,
            sample_count: 0,
            db_sum: 0,
            peak_db: -1,
            hw_today_db: -1,
            weekly_avg_db: -1,
            daily_history: Vec::new(),
            listening_secs_today: 0,
            loud_secs_today: 0,
            daily_dose_fraction: 0.0,
        }
    }

    pub fn save_hearing_cache(&self, cache: &HearingWellnessCache) {
        if let Ok(json) = serde_json::to_string_pretty(cache) {
            let tmp_path = self.hearing_cache_path.with_extension("tmp");
            if std::fs::write(&tmp_path, json).is_ok() {
                let _ = std::fs::rename(&tmp_path, &self.hearing_cache_path);
            }
        }
    }

    pub fn update_daily_summary(&self, summary: &maestro::protocol::types::DosimeterSummary, now: u64) {
        let mut cache = self.load_hearing_cache(now);
        if summary.unknown5.is_finite() && summary.unknown5 > 1.0 {
            let db = (summary.unknown5.log10() * 10.0).round() as i32;
            if (20..=125).contains(&db) {
                cache.hw_today_db = db;
            }
        }
        let mut history = Vec::new();
        let mut active_intensities = Vec::new();
        for entry in &summary.unknown2 {
            if entry.unknown6.is_finite() && entry.unknown6 > 1.0 {
                let db = (entry.unknown6.log10() * 10.0).round() as i32;
                if (20..=125).contains(&db) {
                    history.push(db);
                    active_intensities.push(entry.unknown6 as f64);
                } else {
                    history.push(0);
                }
            } else {
                history.push(0);
            }
        }
        if !history.is_empty() {
            cache.daily_history = history;
        }
        if !active_intensities.is_empty() {
            let mean_intensity =
                active_intensities.iter().sum::<f64>() / active_intensities.len() as f64;
            if mean_intensity > 1.0 {
                cache.weekly_avg_db = (mean_intensity.log10() * 10.0).round() as i32;
            }
        }
        self.save_hearing_cache(&cache);
    }

    pub fn resolve_hearing_wellness(&self, now: u64, live_db: i32) -> HearingWellness {
        let cache = self.load_hearing_cache(now);
        let sampled_avg = if cache.sample_count > 0 {
            (cache.db_sum as f64 / cache.sample_count as f64).round() as i32
        } else {
            -1
        };
        let avg_db = if cache.hw_today_db >= 0 {
            cache.hw_today_db
        } else {
            sampled_avg
        };
        let status = if live_db >= 80 {
            "loud".to_string()
        } else if live_db >= 75 {
            "caution".to_string()
        } else if live_db >= 0 {
            "ok".to_string()
        } else {
            "idle".to_string()
        };

        HearingWellness {
            live_db,
            avg_db,
            peak_db: cache.peak_db,
            weekly_avg_db: cache.weekly_avg_db,
            daily_history: cache.daily_history,
            status,
            listening_secs_today: cache.listening_secs_today,
            loud_secs_today: cache.loud_secs_today,
            daily_dose_percent: (cache.daily_dose_fraction * 100.0).round() as i32,
        }
    }

    pub fn record_live_db(&self, intensity: f32, dt_secs: u64, now: u64) -> HearingWellness {
        if !intensity.is_finite() || intensity <= 1.0 {
            return self.resolve_hearing_wellness(now, -1);
        }
        let db = (intensity.log10() * 10.0).round() as i32;
        if !(20..=125).contains(&db) {
            return self.resolve_hearing_wellness(now, -1);
        }

        let mut cache = self.load_hearing_cache(now);
        let step_secs = dt_secs.clamp(1, 5);
        cache.sample_count += 1;
        cache.db_sum += db as u64;
        cache.peak_db = cache.peak_db.max(db);
        cache.listening_secs_today += step_secs;
        if db >= 80 {
            cache.loud_secs_today += step_secs;
        }
        // WHO/ITU-T H.870 reference: 80 dB for 40h/week (~20,571s/day) with 3 dB exchange rate
        if db >= 65 {
            let allowed_secs = 20_571.0 * 2.0_f64.powf((80.0 - db as f64) / 3.0);
            if allowed_secs > 0.0 {
                cache.daily_dose_fraction += (step_secs as f64) / allowed_secs;
            }
        }
        self.save_hearing_cache(&cache);
        self.resolve_hearing_wellness(now, db)
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
                volume_exposure_notifications: true,
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
                let (ven, _) = self.run_locked(&["get", "volume-exposure-notifications"]).unwrap_or_default();

                SettingsInfo {
                    speech_detection: speech.trim() == "true",
                    ohd: ohd.trim() == "true",
                    multipoint: multi.trim() == "true",
                    gestures: gest.trim() == "true",
                    volume_eq: veq.trim() == "true",
                    volume_exposure_notifications: ven.trim() != "false",
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
            hearing_wellness: self.resolve_hearing_wellness(now, -1),
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
            hearing_wellness: self.resolve_hearing_wellness(now, -1),
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
            "volume_exposure_notifications" | "volume-exposure-notifications" => {
                "volume-exposure-notifications"
            }
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
