use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PodBattery {
    pub level: i32,
    pub charging: bool,
    pub in_ear: bool,
    pub in_case: bool,
}

impl Default for PodBattery {
    fn default() -> Self {
        Self {
            level: -1,
            charging: false,
            in_ear: false,
            in_case: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaseBattery {
    pub level: i32,
    pub charging: bool,
    pub last_known: bool,
    pub timestamp: u64,
    pub stale: bool,
}

impl Default for CaseBattery {
    fn default() -> Self {
        Self {
            level: -1,
            charging: false,
            last_known: false,
            timestamp: 0,
            stale: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatteryInfo {
    pub left: PodBattery,
    pub right: PodBattery,
    pub case: CaseBattery,
    pub combined: i32,
}

impl Default for BatteryInfo {
    fn default() -> Self {
        Self {
            left: PodBattery::default(),
            right: PodBattery::default(),
            case: CaseBattery::default(),
            combined: -1,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingsInfo {
    pub speech_detection: bool,
    pub ohd: bool,
    pub multipoint: bool,
    pub gestures: bool,
    pub volume_eq: bool,
    #[serde(default = "default_true")]
    pub volume_exposure_notifications: bool,
}

fn default_true() -> bool {
    true
}

impl Default for SettingsInfo {
    fn default() -> Self {
        Self {
            speech_detection: false,
            ohd: false,
            multipoint: false,
            gestures: false,
            volume_eq: false,
            volume_exposure_notifications: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HearingWellness {
    pub live_db: i32,
    pub avg_db: i32,
    pub peak_db: i32,
    #[serde(default = "default_neg_one")]
    pub weekly_avg_db: i32,
    #[serde(default)]
    pub daily_history: Vec<i32>,
    pub status: String,
    pub listening_secs_today: u64,
    pub loud_secs_today: u64,
    pub daily_dose_percent: i32,
}

fn default_neg_one() -> i32 {
    -1
}

impl Default for HearingWellness {
    fn default() -> Self {
        Self {
            live_db: -1,
            avg_db: -1,
            peak_db: -1,
            weekly_avg_db: -1,
            daily_history: Vec::new(),
            status: "idle".to_string(),
            listening_secs_today: 0,
            loud_secs_today: 0,
            daily_dose_percent: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HearingWellnessCache {
    pub day_key: u64,
    pub sample_count: u64,
    pub db_sum: u64,
    pub peak_db: i32,
    #[serde(default = "default_neg_one")]
    pub hw_today_db: i32,
    #[serde(default = "default_neg_one")]
    pub weekly_avg_db: i32,
    #[serde(default)]
    pub daily_history: Vec<i32>,
    pub listening_secs_today: u64,
    pub loud_secs_today: u64,
    pub daily_dose_fraction: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginStatus {
    pub ok: bool,
    pub connected: bool,
    pub device_name: String,
    pub model_name: String,
    pub mac: String,
    pub battery: BatteryInfo,
    pub noise_mode: String,
    pub settings: SettingsInfo,
    #[serde(default)]
    pub hearing_wellness: HearingWellness,
    pub last_updated: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaseCache {
    pub level: i32,
    pub charging: bool,
    pub timestamp: u64,
}
