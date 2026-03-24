# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

VRC OSC Manager is a Rust desktop application for managing multiple VRChat OSC plugins. It runs as a system tray application on Linux and Windows, with a Slint-based GUI for configuration.

## Build & Development Commands

```bash
# Build (requires system libs on Linux: libdbus-1-dev libssl-dev libxdo-dev)
cargo build

# Run
cargo run

# Check without building
cargo check

# Lint
cargo clippy -- -D warnings

# Format check
cargo fmt --all -- --check

# Format fix
cargo fmt --all

# Cross-compile for Windows from Linux
cross build --target x86_64-pc-windows-gnu --release
```

There are no tests in this project.

## Architecture

The application has two main threads:
1. **UI thread** — runs the Slint event loop (`src/ui.rs`, `src/main.rs`)
2. **Tokio runtime** — runs all background tasks (`src/background.rs`)

Communication between UI and background uses `tokio::sync::mpsc` channels (`UiEvent` for UI→background, `AppEvent` for background→UI).

### Background Task System

Background tasks are managed via `tokio-graceful-shutdown` subsystems, all started in `src/background.rs`:

- **OrchestrateTask** — central coordinator handling app/UI events and routing commands
- **VrchatMonitorTask** — detects VRChat process start/stop via `sysinfo`
- **PluginManagerTask** — starts/stops plugins based on VRChat activity and user config
- **OscReceiverTask** / **OscSenderTask** — UDP OSC message I/O
- **OscQueryTask** — HTTP server for VRChat's OSCQuery protocol
- **TrayTask** — system tray icon management
- **ConfigWriterTask** — debounced config file persistence

### Plugin System

Plugins implement the `Plugin` trait (`src/plugins/mod.rs`). The `define_plugins!` macro registers all plugins. Each plugin:
- Has its own config managed via `ConfigManager::with_plugin_id()`
- Receives/sends OSC messages through `ChannelManager` (broadcast receiver + mpsc sender)
- Registers OSCQuery endpoints
- Can optionally provide a settings UI panel

Current plugins: `media_control`, `watch`, `pishock`.

### UI Layer

- UI is defined in Slint (`.slint` files in `ui/`)
- `build.rs` compiles Slint files and converts tray icons (RGBA pixel reorder for Linux, `.rc` embedding for Windows)
- `slint::include_modules!()` in `main.rs` generates Rust bindings from Slint

### Config System

`src/utils/config.rs` provides `ConfigManager` and `ConfigHandle<T>`. Configs are TOML files stored in the OS config directory under `vrc-osc-manager/`. `ConfigHandle` wraps `Arc<RwLock<T>>` and triggers async writes through the `ConfigWriterTask`.

### Platform Abstraction

`src/platform/` contains `linux.rs` and `windows.rs` behind `cfg` gates, implementing the `Platform` trait for auto-start and folder-open functionality.

### mDNS/Service Discovery

The app broadcasts OSCQuery and OSC services via mDNS (`searchlight` crate) so VRChat can discover it automatically.
