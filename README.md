# Google Pixel Buds Pro 2 Plugin for Omarchy

A native system integration plugin for Google Pixel Buds Pro 2 on Omarchy / Quickshell.

## Features
- **Top Bar Integration**: Adaptive themed vector icon matching native system icon style.
- **Battery Telemetry**: Real-time Left Pod, Right Pod, and Case battery levels (with last-known case caching).
- **Noise Control**: Active Noise Cancellation (ANC), Transparency, and Off switching / cycling.
- **Bud Settings**: Toggle Conversation Awareness, In-Ear Detection, Multipoint Audio, Touch Gestures, and Volume EQ.
- **Find My Buds**: Directional 3 kHz locator chime with MPRIS audio pause/resume.
- **Native Rust Engine**: High-performance Rust backend daemon and CLI controller with zero runtime dependencies.

## Architecture
- `src/`: Native Rust daemon and CLI tool (`pixel-buds`).
- `Panel.qml`: Quickshell popup panel for battery, controls, and settings.
- `PixelBudsIcon.qml`: Vector icon conforming to active desktop themes.
- `Service.qml`: Reactive interface connecting the Quickshell UI to the Rust bridge.
- `Model.js`: Data parsing, state contracts, and helper functions.
