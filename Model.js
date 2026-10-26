// Model.js - Data structures and helpers for Pixel Buds Pro 2

var NOISE_OFF = "off"
var NOISE_ANC = "active"
var NOISE_TRANSPARENCY = "aware"
var NOISE_UNKNOWN = ""

var LEVEL_UNKNOWN = -1

var GLYPH_CHECK = "✓"

function defaultPod() {
  return { level: LEVEL_UNKNOWN, charging: false, inEar: false, inCase: false }
}

function defaultHearingWellness() {
  return {
    liveDb: LEVEL_UNKNOWN,
    avgDb: LEVEL_UNKNOWN,
    peakDb: LEVEL_UNKNOWN,
    weeklyAvgDb: LEVEL_UNKNOWN,
    dailyHistory: [],
    status: "idle",
    listeningSecsToday: 0,
    loudSecsToday: 0,
    dailyDosePercent: 0
  }
}

function defaultStatus() {
  return {
    ok: false,
    lastError: "",
    connected: false,
    deviceName: "Pixel Buds Pro 2",
    modelName: "Google Pixel Buds Pro 2",
    mac: "",
    left: defaultPod(),
    right: defaultPod(),
    caseBattery: { level: LEVEL_UNKNOWN, charging: false, lastKnown: false, timestamp: 0, stale: false },
    combinedBattery: LEVEL_UNKNOWN,
    noiseMode: NOISE_UNKNOWN,
    hearingWellness: defaultHearingWellness(),
    settings: {
      speech_detection: false,
      ohd: false,
      multipoint: false,
      gestures: false,
      volume_eq: false,
      volume_exposure_notifications: true
    }
  }
}

function noiseModeName(mode) {
  if (mode === NOISE_ANC) return "Noise Cancellation"
  if (mode === NOISE_TRANSPARENCY) return "Transparency"
  if (mode === NOISE_OFF) return "Off"
  return "Unknown"
}

function formatTimeAgo(timestampSecs) {
  if (!timestampSecs || timestampSecs <= 0) return ""
  var nowSecs = Math.floor(Date.now() / 1000)
  var diff = Math.max(0, nowSecs - timestampSecs)
  if (diff < 60) return "just now"
  var mins = Math.floor(diff / 60)
  if (mins < 60) return mins + "m ago"
  var hours = Math.floor(mins / 60)
  if (hours < 24) return hours + "h ago"
  var days = Math.floor(hours / 24)
  return days + "d ago"
}

function levelText(level) {
  if (level === undefined || level === null || level < 0) return "--"
  return level + "%"
}

function levelFraction(level) {
  if (level === undefined || level === null || level < 0) return 0.0
  return Math.max(0.0, Math.min(1.0, level / 100.0))
}

function dbText(db) {
  if (db === undefined || db === null || db < 0) return "-- dB"
  return db + " dB"
}

function dbFraction(db) {
  if (db === undefined || db === null || db < 0) return 0.0
  return Math.max(0.0, Math.min(1.0, db / 100.0))
}

function hearingStatusLabel(hw) {
  if (!hw || hw.liveDb === undefined || hw.liveDb < 0) return "Idle"
  if (hw.liveDb >= 80) return "Loud (>80 dB)"
  if (hw.liveDb >= 75) return "Moderate"
  return "Safe (<80 dB)"
}

function formatDurationShort(secs) {
  if (!secs || secs <= 0) return "0m"
  if (secs < 60) return secs + "s"
  var mins = Math.floor(secs / 60)
  if (mins < 60) return mins + "m"
  var hours = Math.floor(mins / 60)
  var remMins = mins % 60
  return remMins > 0 ? (hours + "h " + remMins + "m") : (hours + "h")
}

function podMeta(pod) {
  if (!pod || pod.level === LEVEL_UNKNOWN) return ""
  if (pod.lastKnown) {
    var timeStr = formatTimeAgo(pod.timestamp)
    var label = timeStr ? "Last known · " + timeStr : "Last known"
    return pod.charging ? "Charging · " + label : label
  }
  if (pod.charging) return "Charging"
  if (pod.inCase) return "In case"
  if (pod.inEar) return "In ear"
  return "Out of ear"
}

function parseStatus(raw) {
  var status = defaultStatus()
  if (!raw || typeof raw !== "string" || raw.trim() === "") {
    status.lastError = "No status data"
    return status
  }
  try {
    var data = JSON.parse(raw)
    status.ok = data.ok === true
    status.connected = data.connected === true
    status.lastError = data.error || ""
    if (data.device_name) status.deviceName = data.device_name
    if (data.model_name) status.modelName = data.model_name
    if (data.mac) status.mac = data.mac

    if (data.battery) {
      if (data.battery.left) {
        status.left.level = data.battery.left.level !== undefined ? data.battery.left.level : LEVEL_UNKNOWN
        status.left.charging = data.battery.left.charging === true
        status.left.inEar = data.battery.left.in_ear === true
        status.left.inCase = data.battery.left.in_case === true
      }
      if (data.battery.right) {
        status.right.level = data.battery.right.level !== undefined ? data.battery.right.level : LEVEL_UNKNOWN
        status.right.charging = data.battery.right.charging === true
        status.right.inEar = data.battery.right.in_ear === true
        status.right.inCase = data.battery.right.in_case === true
      }
      if (data.battery.case) {
        status.caseBattery.level = data.battery.case.level !== undefined ? data.battery.case.level : LEVEL_UNKNOWN
        status.caseBattery.charging = data.battery.case.charging === true
        status.caseBattery.lastKnown = data.battery.case.last_known === true
        status.caseBattery.timestamp = data.battery.case.timestamp || 0
        status.caseBattery.stale = data.battery.case.stale === true
      }
      if (data.battery.combined !== undefined) {
        status.combinedBattery = data.battery.combined
      }
    }

    if (data.noise_mode) status.noiseMode = data.noise_mode
    if (data.settings) status.settings = data.settings
    if (data.hearing_wellness) {
      var hw = data.hearing_wellness
      status.hearingWellness = {
        liveDb: hw.live_db !== undefined ? hw.live_db : LEVEL_UNKNOWN,
        avgDb: hw.avg_db !== undefined ? hw.avg_db : LEVEL_UNKNOWN,
        peakDb: hw.peak_db !== undefined ? hw.peak_db : LEVEL_UNKNOWN,
        weeklyAvgDb: hw.weekly_avg_db !== undefined ? hw.weekly_avg_db : LEVEL_UNKNOWN,
        dailyHistory: Array.isArray(hw.daily_history) ? hw.daily_history : [],
        status: hw.status || "idle",
        listeningSecsToday: hw.listening_secs_today || 0,
        loudSecsToday: hw.loud_secs_today || 0,
        dailyDosePercent: hw.daily_dose_percent || 0
      }
    }
  } catch (e) {
    status.lastError = "JSON parse error: " + e.message
  }
  return status
}

function elideError(text) {
  if (!text) return ""
  var s = String(text).trim()
  return s.length > 120 ? s.substring(0, 117) + "..." : s
}
