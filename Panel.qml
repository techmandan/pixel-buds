import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Quickshell
import Quickshell.Io
import qs.Commons
import qs.Ui
import "Model.js" as Model

Panel {
  id: root
  moduleName: "dan.pixel-buds"
  ipcTarget: "pixel-buds"

  property int cursorIndex: 0
  property bool cursorActive: false
  property bool findConfirmOpen: false
  property string pendingFindSide: ""

  readonly property bool hideWhenDisconnected: setting("hideWhenDisconnected", true) === true
  readonly property color foreground: bar ? bar.foreground : Color.foreground
  readonly property color urgent: bar ? bar.urgent : Color.urgent
  readonly property color dim: Qt.darker(foreground, 1.55)
  readonly property color barIconColor: service.connected ? foreground : dim
  readonly property string fontFamily: bar ? bar.fontFamily : Style.font.family
  readonly property int lowBatteryPercent: 20

  readonly property var cursorRows: {
    var rows = []
    if (service.hasDevice) {
      rows.push("mode:active")
      rows.push("mode:aware")
      rows.push("mode:off")
      rows.push("feat:speech_detection")
      rows.push("feat:ohd")
      rows.push("feat:multipoint")
      rows.push("feat:gestures")
      rows.push("feat:volume_eq")
      rows.push("find:left")
      rows.push("find:right")
      rows.push("find:both")
    }
    return rows
  }

  readonly property string cursorRow: cursorRows.length === 0
    ? ""
    : cursorRows[Math.max(0, Math.min(cursorIndex, cursorRows.length - 1))]

  function rowHasCursor(name) {
    return cursorActive && cursorRow === name
  }

  function moveCursor(dy) {
    cursorActive = true
    if (cursorRows.length === 0) return
    cursorIndex = Math.max(0, Math.min(cursorRows.length - 1, cursorIndex + dy))
  }

  function activateCursor() {
    if (cursorRow.indexOf("mode:") === 0) {
      service.setNoiseMode(cursorRow.substring(5))
    } else if (cursorRow.indexOf("feat:") === 0) {
      service.toggleSetting(cursorRow.substring(5))
    } else if (cursorRow.indexOf("find:") === 0) {
      requestFind(cursorRow.substring(5))
    }
  }

  function requestFind(side) {
    if (!service.hasDevice) return
    if (service.findingSide === side) {
      service.stopFind()
      return
    }
    pendingFindSide = side
    findConfirmOpen = true
  }

  function cancelFindConfirm() {
    findConfirmOpen = false
    pendingFindSide = ""
    Qt.callLater(function () { keyCatcher.forceActiveFocus() })
  }

  function confirmFind() {
    var side = pendingFindSide
    findConfirmOpen = false
    pendingFindSide = ""
    if (side) service.find(side)
    Qt.callLater(function () { keyCatcher.forceActiveFocus() })
  }

  visible: !hideWhenDisconnected || service.connected
  implicitWidth: button.implicitWidth
  implicitHeight: button.implicitHeight

  onOpenedChanged: {
    if (opened) {
      cursorActive = false
      cursorIndex = 0
      findConfirmOpen = false
      pendingFindSide = ""
      if (panelFlick) panelFlick.contentY = 0
      service.refresh()
      Qt.callLater(function () { keyCatcher.forceActiveFocus() })
    } else {
      service.stopFind()
    }
  }

  Service {
    id: service
    settings: root.settings
  }

  BarIconButton {
    id: button
    anchors.fill: parent
    bar: root.bar
    iconComponent: Component {
      Item {
        anchors.fill: parent
        PixelBudsIcon {
          anchors.centerIn: parent
          iconSize: Style.space(16)
          color: root.barIconColor
          opacity: service.connected ? 1.0 : 0.45
        }
      }
    }
    tooltipText: service.connected
      ? (service.deviceName + " (" + (service.leftPod.level >= 0 ? service.leftPod.level + "% · " : "") + (service.caseBattery.level >= 0 ? "Case " + service.caseBattery.level + "% · " : "") + Model.noiseModeName(service.noiseMode) + ")")
      : "Pixel Buds Pro 2 (Disconnected)"
    onPressed: function (b) {
      if (b === Qt.RightButton) service.cycleNoiseMode()
      else root.toggle()
    }
  }

  KeyboardPanel {
    id: panel
    anchorItem: button
    owner: root
    bar: root.bar
    open: root.opened
    focusTarget: keyCatcher
    contentWidth: panel.fittedContentWidth(Style.space(380))
    contentHeight: panel.fittedContentHeight(column.implicitHeight + Style.space(24), Style.space(780))

    PanelKeyCatcher {
      id: keyCatcher
      anchors.fill: parent
      onMoveRequested: function (dx, dy) {
        if (root.findConfirmOpen) return
        if (!root.cursorActive) { root.cursorActive = true; return }
        if (dy !== 0) root.moveCursor(dy)
      }
      onActivateRequested: {
        if (root.findConfirmOpen) return
        if (root.cursorActive) root.activateCursor()
      }
      onCloseRequested: {
        if (root.findConfirmOpen) { root.cancelFindConfirm(); return }
        root.close()
      }
      onTextKey: function (t) {
        if (root.findConfirmOpen) return
        var key = String(t).toLowerCase()
        if (key === "r") service.refresh()
        else if (!service.hasDevice) return
        else if (key === "n") service.setNoiseMode(Model.NOISE_ANC)
        else if (key === "t") service.setNoiseMode(Model.NOISE_TRANSPARENCY)
        else if (key === "o") service.setNoiseMode(Model.NOISE_OFF)
        else if (key === "f") {
          if (service.finding) service.stopFind()
          else root.requestFind("both")
        }
      }

      Flickable {
        id: panelFlick
        anchors.fill: parent
        contentWidth: width
        contentHeight: column.implicitHeight + Style.space(16)
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        flickableDirection: Flickable.VerticalFlick
        interactive: contentHeight > height
        ScrollBar.vertical: ScrollBar { policy: ScrollBar.AsNeeded }

        Column {
          id: column
          width: panelFlick.width
          spacing: Style.space(12)

          PanelHero {
            width: parent.width
            title: service.connected ? service.deviceName : "Pixel Buds Pro 2"
            meta: service.connected ? Model.noiseModeName(service.noiseMode) : "Disconnected"
            detail: service.connected && service.leftPod.level >= 0 ? service.leftPod.level + "%" : ""
            foreground: root.foreground
            fontFamily: root.fontFamily
            iconOpacity: service.connected ? 1.0 : 0.5
            iconComponent: Component {
              Item {
                width: Style.font.display * 1.3
                height: Style.font.display * 1.3
                PixelBudsIcon {
                  anchors.centerIn: parent
                  iconSize: Style.font.display * 1.25
                  color: service.connected ? Color.accent : root.dim
                }
              }
            }
          }

          Text {
            visible: service.actionStatus !== "" || (service.lastError !== "" && service.daemonReachable)
            width: parent.width
            text: service.actionStatus !== "" ? service.actionStatus : service.lastError
            color: root.urgent
            font.family: root.fontFamily
            font.pixelSize: Style.font.bodySmall
            wrapMode: Text.WordWrap
          }

          // Battery section
          Column {
            visible: service.hasBattery
            width: parent.width
            spacing: Style.space(10)

            PanelSectionHeader {
              text: "BATTERY"
              foreground: root.foreground
              fontFamily: root.fontFamily
            }

            Column {
              width: parent.width
              spacing: Style.space(6)

              PodRow {
                width: parent.width
                label: "Left"
                pod: service.leftPod
              }
              PodRow {
                width: parent.width
                label: "Right"
                pod: service.rightPod
              }
              PodRow {
                width: parent.width
                label: "Case"
                pod: service.caseBattery
              }
            }
          }

          PanelSeparator {
            visible: service.hasBattery && service.hasDevice
            foreground: root.foreground
          }

          // Noise Control section
          Column {
            visible: service.hasDevice
            width: parent.width
            spacing: Style.space(10)

            PanelSectionHeader {
              text: "NOISE CONTROL"
              foreground: root.foreground
              fontFamily: root.fontFamily
            }

            Column {
              width: parent.width
              spacing: Style.space(4)

              ModeRow {
                width: parent.width
                mode: Model.NOISE_ANC
                label: "Noise Cancellation"
                shortcut: "N"
              }
              ModeRow {
                width: parent.width
                mode: Model.NOISE_TRANSPARENCY
                label: "Transparency"
                shortcut: "T"
              }
              ModeRow {
                width: parent.width
                mode: Model.NOISE_OFF
                label: "Off"
                shortcut: "O"
              }
            }
          }

          PanelSeparator {
            visible: service.hasDevice
            foreground: root.foreground
          }

          // Features / Toggles section
          Column {
            visible: service.hasDevice
            width: parent.width
            spacing: Style.space(10)

            PanelSectionHeader {
              text: "FEATURES"
              foreground: root.foreground
              fontFamily: root.fontFamily
            }

            Column {
              width: parent.width
              spacing: Style.space(6)

              FeatureRow {
                width: parent.width
                key: "speech_detection"
                title: "Conversation Awareness"
                subtitle: "Auto-transparency when speaking"
                checked: service.speechDetection
              }

              FeatureRow {
                width: parent.width
                key: "ohd"
                title: "In-Ear Detection"
                subtitle: "Auto-pause when earbuds are removed"
                checked: service.ohd
              }

              FeatureRow {
                width: parent.width
                key: "multipoint"
                title: "Multipoint Audio"
                subtitle: "Seamless switching between paired devices"
                checked: service.multipoint
              }

              FeatureRow {
                width: parent.width
                key: "gestures"
                title: "Touch Gestures"
                subtitle: "Tap and hold controls on earbuds"
                checked: service.gestures
              }

              FeatureRow {
                width: parent.width
                key: "volume_eq"
                title: "Volume EQ"
                subtitle: "Dynamically adjust frequencies at low volume"
                checked: service.volumeEq
              }
            }
          }

          PanelSeparator {
            visible: service.hasDevice
            foreground: root.foreground
          }

          // Find My Buds section
          Column {
            visible: service.hasDevice
            width: parent.width
            spacing: Style.space(10)

            PanelSectionHeader {
              text: "FIND MY BUDS"
              foreground: root.foreground
              fontFamily: root.fontFamily
            }

            Text {
              width: parent.width
              text: service.finding
                ? "Playing locating chime. Click again to stop."
                : "Plays a locator chime to help find misplaced earbuds. Do not use while earbuds are in your ears."
              color: root.dim
              font.family: root.fontFamily
              font.pixelSize: Style.font.caption
              wrapMode: Text.WordWrap
            }

            FindRow {
              width: parent.width
              label: "Left Bud"
              side: "left"
            }
            FindRow {
              width: parent.width
              label: "Right Bud"
              side: "right"
            }
            FindRow {
              width: parent.width
              label: "Both Buds"
              side: "both"
            }
          }

          Item {
            width: parent.width
            height: Style.space(14)
          }
        }
      }

      ConfirmDialog {
        id: findConfirm
        anchors.fill: parent
        opened: root.findConfirmOpen
        message: "Locating chimes can be loud. Please ensure earbuds are NOT in your ears before ringing."
        confirmText: "Ring"
        cancelText: "Cancel"
        onConfirmed: root.confirmFind()
        onCanceled: root.cancelFindConfirm()
      }
    }
  }

  // --- Reusable Component Definitions ---

  component PodRow: Item {
    id: podRow
    property string label: ""
    property var pod: Model.defaultPod()
    property string meta: ""

    readonly property string metaText: meta !== "" ? meta : Model.podMeta(pod)
    readonly property bool low: pod && pod.level !== Model.LEVEL_UNKNOWN
      && pod.level <= root.lowBatteryPercent && !pod.charging

    implicitHeight: podLayout.implicitHeight

    RowLayout {
      id: podLayout
      anchors.left: parent.left
      anchors.right: parent.right
      spacing: Style.space(8)

      Text {
        text: podRow.label
        color: root.foreground
        opacity: 0.75
        font.family: root.fontFamily
        font.pixelSize: Style.font.bodySmall
        Layout.preferredWidth: Math.max(Style.space(48), implicitWidth + Style.space(8))
      }

      Rectangle {
        id: meterTrack
        Layout.fillWidth: true
        Layout.alignment: Qt.AlignVCenter
        implicitHeight: Style.space(6)
        radius: height / 2
        color: Qt.darker(root.foreground, 3.2)

        Rectangle {
          width: meterTrack.width * Model.levelFraction(podRow.pod ? podRow.pod.level : Model.LEVEL_UNKNOWN)
          height: parent.height
          radius: parent.radius
          color: podRow.low ? root.urgent : Color.accent
        }
      }

      Text {
        text: Model.levelText(podRow.pod ? podRow.pod.level : Model.LEVEL_UNKNOWN)
        color: root.foreground
        font.family: root.fontFamily
        font.pixelSize: Style.font.bodySmall
        horizontalAlignment: Text.AlignRight
        Layout.preferredWidth: Style.space(38)
      }

      Text {
        text: podRow.metaText !== "" ? podRow.metaText : (podRow.pod && podRow.pod.level === Model.LEVEL_UNKNOWN ? "Dock to read" : "")
        color: root.dim
        font.family: root.fontFamily
        font.pixelSize: Style.font.caption
        elide: Text.ElideRight
        Layout.preferredWidth: Math.max(Style.space(85), implicitWidth)
      }
    }
  }

  component ModeRow: Item {
    property string mode: ""
    property string label: ""
    property string shortcut: ""
    implicitHeight: Style.space(36)
    readonly property bool selected: service.noiseMode === mode
    readonly property bool hovered: rowHasCursor("mode:" + mode)

    Rectangle {
      anchors.fill: parent
      radius: Style.cornerRadius
      color: hovered ? Qt.rgba(root.foreground.r, root.foreground.g, root.foreground.b, 0.08) : "transparent"
    }

    Row {
      anchors.left: parent.left
      anchors.leftMargin: Style.space(8)
      anchors.verticalCenter: parent.verticalCenter
      spacing: Style.space(8)

      Text {
        text: label
        color: root.foreground
        font.family: root.fontFamily
        font.pixelSize: Style.font.body
      }

      Text {
        visible: shortcut !== ""
        text: "[" + shortcut + "]"
        color: root.dim
        font.family: root.fontFamily
        font.pixelSize: Style.font.caption
      }
    }

    Text {
      visible: selected
      anchors.right: parent.right
      anchors.rightMargin: Style.space(8)
      anchors.verticalCenter: parent.verticalCenter
      text: Model.GLYPH_CHECK
      color: Color.accent
      font.family: root.fontFamily
      font.pixelSize: Style.font.body
      font.bold: true
    }

    MouseArea {
      anchors.fill: parent
      hoverEnabled: true
      onClicked: service.setNoiseMode(mode)
    }
  }

  component FeatureRow: Item {
    id: featRow
    property string key: ""
    property string title: ""
    property string subtitle: ""
    property bool checked: false
    implicitHeight: Style.space(42)
    readonly property bool hovered: rowHasCursor("feat:" + key)

    Rectangle {
      anchors.fill: parent
      radius: Style.cornerRadius
      color: hovered ? Qt.rgba(root.foreground.r, root.foreground.g, root.foreground.b, 0.08) : "transparent"
    }

    Column {
      anchors.left: parent.left
      anchors.leftMargin: Style.space(8)
      anchors.right: toggleSwitch.left
      anchors.rightMargin: Style.space(8)
      anchors.verticalCenter: parent.verticalCenter
      spacing: Style.space(1)

      Text {
        text: featRow.title
        color: root.foreground
        font.family: root.fontFamily
        font.pixelSize: Style.font.body
      }
      Text {
        visible: featRow.subtitle !== ""
        text: featRow.subtitle
        color: root.dim
        font.family: root.fontFamily
        font.pixelSize: Style.font.caption
      }
    }

    ToggleSwitch {
      id: toggleSwitch
      anchors.right: parent.right
      anchors.rightMargin: Style.space(8)
      anchors.verticalCenter: parent.verticalCenter
      checked: featRow.checked
      interactive: false
      foreground: root.foreground
      accent: Color.accent
    }

    MouseArea {
      anchors.fill: parent
      hoverEnabled: true
      onClicked: service.toggleSetting(featRow.key)
    }
  }

  component FindRow: Item {
    property string label: ""
    property string side: ""
    implicitHeight: Style.space(36)
    readonly property bool active: service.findingSide === side
    readonly property bool hovered: rowHasCursor("find:" + side)

    Rectangle {
      anchors.fill: parent
      radius: Style.cornerRadius
      color: hovered ? Qt.rgba(root.foreground.r, root.foreground.g, root.foreground.b, 0.08) : "transparent"
    }

    Text {
      anchors.left: parent.left
      anchors.leftMargin: Style.space(8)
      anchors.verticalCenter: parent.verticalCenter
      text: label
      color: root.foreground
      font.family: root.fontFamily
      font.pixelSize: Style.font.body
    }

    Text {
      anchors.right: parent.right
      anchors.rightMargin: Style.space(8)
      anchors.verticalCenter: parent.verticalCenter
      text: active ? "Stop" : "Ring"
      color: active ? root.urgent : Color.accent
      font.family: root.fontFamily
      font.pixelSize: Style.font.bodySmall
      font.bold: true
    }

    MouseArea {
      anchors.fill: parent
      hoverEnabled: true
      onClicked: root.requestFind(side)
    }
  }
}
