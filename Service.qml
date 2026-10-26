import QtQuick
import Quickshell
import Quickshell.Io
import "Model.js" as Model

Item {
  id: root

  property var settings: ({})
  property bool daemonReachable: false
  property bool connected: false
  property string deviceName: "Pixel Buds Pro 2"
  property string modelName: "Google Pixel Buds Pro 2"
  property string mac: ""

  property var leftPod: Model.defaultPod()
  property var rightPod: Model.defaultPod()
  property var caseBattery: ({ level: Model.LEVEL_UNKNOWN, charging: false, lastKnown: false, timestamp: 0, stale: false })
  property int combinedBattery: Model.LEVEL_UNKNOWN

  property string noiseMode: Model.NOISE_ANC

  property bool speechDetection: false
  property bool ohd: false
  property bool multipoint: false
  property bool gestures: false
  property bool volumeEq: false
  property bool volumeExposureNotifications: true
  property var hearingWellness: Model.defaultHearingWellness()

  property string lastError: ""
  property string actionStatus: ""
  property string findingSide: ""
  property bool bootstrapped: false

  readonly property bool hasDevice: connected
  readonly property bool finding: findingSide !== ""
  readonly property bool hasBattery: daemonReachable
    && (leftPod.level !== Model.LEVEL_UNKNOWN
      || rightPod.level !== Model.LEVEL_UNKNOWN
      || caseBattery.level !== Model.LEVEL_UNKNOWN
      || combinedBattery !== Model.LEVEL_UNKNOWN)

  readonly property string stateDir: (Quickshell.env("XDG_STATE_HOME")
    || Quickshell.env("HOME") + "/.local/state")
  readonly property string statePath: stateDir + "/pixel-buds/status.json"

  readonly property string pluginDir: {
    var path = Qt.resolvedUrl(".").toString()
    if (path.indexOf("file://") === 0) path = path.slice(7)
    if (path.length > 1 && path.charAt(path.length - 1) === "/")
      path = path.slice(0, path.length - 1)
    return path
  }
  readonly property string bridge: pluginDir + "/bin/pixel-buds"

  function setting(name, fallback) {
    var value = settings ? settings[name] : undefined
    return value === undefined || value === null ? fallback : value
  }

  function refresh() {
    stateFile.reload()
    if (connected && !commandProcess.running && !pollProcess.running) {
      pollProcess.running = true
    }
  }

  function applyLine(raw) {
    var status = Model.parseStatus(raw)
    if (!status.ok) {
      daemonReachable = false
      connected = false
      lastError = status.lastError
      return
    }
    daemonReachable = true
    lastError = ""
    connected = status.connected
    if (!status.connected) return

    deviceName = status.deviceName
    modelName = status.modelName
    mac = status.mac

    leftPod = status.left
    rightPod = status.right
    caseBattery = status.caseBattery
    combinedBattery = status.combinedBattery

    noiseMode = status.noiseMode
    hearingWellness = status.hearingWellness || Model.defaultHearingWellness()

    if (status.settings) {
      speechDetection = status.settings.speech_detection === true
      ohd = status.settings.ohd === true
      multipoint = status.settings.multipoint === true
      gestures = status.settings.gestures === true
      volumeEq = status.settings.volume_eq === true
      volumeExposureNotifications = status.settings.volume_exposure_notifications !== false
    }
  }

  function stateGone() {
    daemonReachable = false
    connected = false
    lastError = ""
    if (!bootstrapped && !bootstrapProcess.running) {
      bootstrapped = true
      bootstrapProcess.running = true
    }
  }

  function setNoiseMode(mode) {
    if (noiseMode === mode && !commandProcess.running) return
    noiseMode = mode
    _sendCmd(["set", "anc", mode])
  }

  function cycleNoiseMode() {
    if (noiseMode === Model.NOISE_ANC) noiseMode = Model.NOISE_TRANSPARENCY
    else if (noiseMode === Model.NOISE_TRANSPARENCY) noiseMode = Model.NOISE_OFF
    else noiseMode = Model.NOISE_ANC
    _sendCmd(["cycle-anc"])
  }

  function toggleSetting(key) {
    var curVal = false
    if (key === "speech_detection") curVal = speechDetection
    else if (key === "ohd") curVal = ohd
    else if (key === "multipoint") curVal = multipoint
    else if (key === "gestures") curVal = gestures
    else if (key === "volume_eq") curVal = volumeEq
    else if (key === "volume_exposure_notifications") curVal = volumeExposureNotifications

    var newVal = !curVal
    if (key === "speech_detection") speechDetection = newVal
    else if (key === "ohd") ohd = newVal
    else if (key === "multipoint") multipoint = newVal
    else if (key === "gestures") gestures = newVal
    else if (key === "volume_eq") volumeEq = newVal
    else if (key === "volume_exposure_notifications") volumeExposureNotifications = newVal

    _sendCmd(["set", key, newVal ? "true" : "false"])
  }

  function find(side) {
    if (findingSide === side) {
      stopFind()
      return
    }
    findingSide = side
    findProcess.command = [bridge, "find", side]
    findProcess.running = true
  }

  function stopFind() {
    findingSide = ""
    stopProcess.command = [bridge, "stop-find"]
    stopProcess.running = true
  }

  function _sendCmd(args) {
    var cmd = [bridge]
    for (var i = 0; i < args.length; i++) cmd.push(args[i])
    commandProcess.command = cmd
    commandProcess.running = true
  }

  Timer {
    id: actionStatusTimer
    interval: 2500
    repeat: false
    onTriggered: root.actionStatus = ""
  }

  FileView {
    id: stateFile
    path: root.statePath
    watchChanges: true
    printErrors: false
    onFileChanged: reload()
    onLoaded: root.applyLine(text())
    onLoadFailed: root.stateGone()
  }

  Process {
    id: commandProcess
    running: false
    command: []
    stderr: StdioCollector { id: commandErr; waitForEnd: true }
    onExited: function (exitCode) {
      if (exitCode !== 0) {
        root.refresh()
        root.actionStatus = Model.elideError(commandErr.text || "Command failed")
        actionStatusTimer.restart()
      } else {
        root.refresh()
      }
    }
  }

  Process {
    id: findProcess
    running: false
    command: []
    stderr: StdioCollector { id: findErr; waitForEnd: true }
    onExited: function (exitCode) {
      root.findingSide = ""
      if (exitCode !== 0) {
        root.actionStatus = Model.elideError(findErr.text || "Could not play locating tone")
        actionStatusTimer.restart()
      }
    }
  }

  Process {
    id: stopProcess
    running: false
    command: []
  }

  Process {
    id: pollProcess
    running: false
    command: [bridge, "poll"]
    onExited: function (exitCode) {
      stateFile.reload()
    }
  }

  Process {
    id: bootstrapProcess
    running: false
    command: [root.pluginDir + "/setup"]
    onExited: function (exitCode) {
      stateFile.reload()
    }
  }
}
