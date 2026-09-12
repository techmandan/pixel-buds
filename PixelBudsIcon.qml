import QtQuick
import QtQuick.Shapes
import qs.Commons

Item {
  id: root

  property real iconSize: Style.font.icon
  property color color: Color.foreground

  width: iconSize
  height: iconSize
  implicitWidth: iconSize
  implicitHeight: iconSize

  Item {
    id: scaler
    anchors.centerIn: parent
    width: 192
    height: 192
    scale: Math.min(root.width, root.height) / 192.0
    transformOrigin: Item.Center

    Shape {
      anchors.fill: parent
      antialiasing: true
      layer.enabled: true
      layer.samples: 4

      // Solid filled pebble case body
      ShapePath {
        strokeWidth: 0
        fillColor: root.color
        PathSvg {
          path: "M38 84 C38 79 42 75 47 75 L145 75 C150 75 154 79 154 84 L154 113 C154 146 128 172 96 172 C64 172 38 146 38 113 Z"
        }
      }

      // Open arched lid and unfilled earbud domes
      ShapePath {
        strokeWidth: 13
        strokeColor: root.color
        fillColor: "transparent"
        capStyle: ShapePath.RoundCap
        joinStyle: ShapePath.RoundJoin
        PathSvg {
          path: "M46 62 C46 32 68 18 96 18 C124 18 146 32 146 62 M48 77 C46 73 45 69 45 65 C45 54 54 45 65 45 C76 45 85 54 85 65 C85 69 84 73 82 77 M110 77 C108 73 107 69 107 65 C107 54 116 45 127 45 C138 45 147 54 147 65 C147 69 146 73 144 77"
        }
      }
    }
  }
}
