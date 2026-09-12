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
}

impl Default for CaseBattery {
    fn default() -> Self {
        Self {
            level: -1,
            charging: false,
            last_known: false,
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
}

impl Default for SettingsInfo {
    fn default() -> Self {
        Self {
            speech_detection: false,
            ohd: false,
            multipoint: false,
            gestures: false,
            volume_eq: false,
        }
    }
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
    pub last_updated: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaseCache {
    pub level: i32,
    pub charging: bool,
    pub timestamp: u64,
}
