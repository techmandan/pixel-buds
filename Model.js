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
    caseBattery: { level: LEVEL_UNKNOWN, charging: false, lastKnown: false },
    combinedBattery: LEVEL_UNKNOWN,
    noiseMode: NOISE_UNKNOWN,
    settings: {
      speech_detection: false,
      ohd: false,
      multipoint: false,
      gestures: false,
      volume_eq: false
    }
  }
}

function noiseModeName(mode) {
  if (mode === NOISE_ANC) return "Noise Cancellation"
  if (mode === NOISE_TRANSPARENCY) return "Transparency"
  if (mode === NOISE_OFF) return "Off"
  return "Unknown"
}

function levelText(level) {
  if (level === undefined || level === null || level < 0) return "--"
  return level + "%"
}

function levelFraction(level) {
  if (level === undefined || level === null || level < 0) return 0.0
  return Math.max(0.0, Math.min(1.0, level / 100.0))
}

function podMeta(pod) {
  if (!pod || pod.level === LEVEL_UNKNOWN) return ""
  if (pod.lastKnown) return pod.charging ? "Charging · Last known" : "Last known"
  if (pod.charging) return "Charging"
  if (pod.inCase) return "In case"
  if (pod.inEar) return "In ear"
  return ""
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
      }
      if (data.battery.combined !== undefined) {
        status.combinedBattery = data.battery.combined
      }
    }

    if (data.noise_mode) status.noiseMode = data.noise_mode
    if (data.settings) status.settings = data.settings
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
