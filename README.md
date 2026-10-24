# Google Pixel Buds Pro 2 Plugin for Omarchy

A native system integration plugin for Google Pixel Buds Pro 2 on Omarchy / Quickshell.

## Features
- **Top Bar Integration**: Adaptive themed vector icon matching native system icon style.
- **Battery Telemetry**: Real-time Left Pod, Right Pod, and Case battery levels (with 4-hour last-known case caching).
- **In-Ear Detection (Auto-Pause / Auto-Resume)**: Automatically pauses active media playback when an earbud is removed and resumes playback when placed back in the ear.
- **Noise Control**: Active Noise Cancellation (ANC), Transparency, and Off switching / cycling.
- **Bud Settings**: Toggle Conversation Detection, In-Ear Detection, Multipoint Audio, Touch Gestures, and Volume EQ.
- **Find My Buds**: Directional 3 kHz locator chime with automatic MPRIS audio pause/resume.
- **Native Rust Engine**: Self-contained Rust daemon speaking Google's Maestro protocol over Bluetooth RFCOMM with zero external CLI dependencies.

## Dependencies
- **Omarchy / Quickshell** desktop environment
- **Rust toolchain (`cargo`)** to compile the native Bluetooth RFCOMM bridge on first setup
- **BlueZ (`bluetooth.service` and `mpris-proxy.service`)** for Bluetooth RFCOMM and AVRCP media status
- **PipeWire (`pw-play`)** for the Find My Buds locator chime

No `sudo` or `pkexec` privileges are required.

## Installation

Install directly with Omarchy's plugin manager:

```bash
omarchy plugin add https://github.com/techmandan/pixel-buds.git --enable
```

On first load, the plugin automatically runs `./setup` to build the release binary and enable the background user systemd service (`pixel-buds.service`). You can also run `./setup` manually at any time:

```bash
~/.config/omarchy/plugins/pixel-buds/setup
```

### Optional Hyprland Media Binding Tip
By default, Hyprland/Omarchy maps `XF86AudioPause` to `playPause` (a toggle). So that hardware Bluetooth AVRCP pause signals sent by the earbuds only ever pause and never toggle already-paused media back on, you can optionally add this to `~/.config/hypr/bindings.lua`:

```lua
hl.unbind("XF86AudioPause")
o.bind("XF86AudioPause", "Pause", "omarchy-shell media pause", { locked = true })
```

## Removal

Stop and remove the user systemd service, then remove the plugin from Omarchy:

```bash
systemctl --user disable --now pixel-buds.service
rm -f ~/.config/systemd/user/pixel-buds.service
systemctl --user daemon-reload
omarchy plugin remove pixel-buds
```

## Architecture
- `src/`: Native Rust daemon (`Maestro` RFCOMM + MPRIS media controller) and CLI tool (`pixel-buds`).
- `Panel.qml`: Quickshell popup panel for battery, controls, and settings.
- `PixelBudsIcon.qml`: Vector icon conforming to active desktop themes.
- `Service.qml`: Reactive interface connecting the Quickshell UI to the Rust bridge.
- `Model.js`: Data parsing, state contracts, and helper functions.

## License
Released under the [MIT License](LICENSE).
