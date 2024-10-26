# VRChat OSC Manager

[![Release](https://github.com/DASPRiD/vrc-osc-manager/actions/workflows/release.yml/badge.svg)](https://github.com/DASPRiD/vrc-osc-manager/actions/workflows/release.yml)

This is an OSC manager for handling multiple VRChat OSC plugins at once.

## Compiling

When compiling the application, it will include all plugins by default. You can opt into only including specific plugins
by only enabling the features you want. For a full list of features, have a look at the `Cargo.toml` file.

In order to cross-compile from Linux for Windows, run the following command:

```bash
cross build --target x86_64-pc-windows-gnu --release
```

## Usage

Download the latest binary for your system from the [Releases](https://github.com/DASPRiD/vrc-osc-manager/releases)
section. Place it in a permanent location and start it up.

You should see an OSC tray icon appearing in your tray bar. Click on it and select "Open VRC OSC Manager". You can then
enable and configure plugins.

If you want the application to automatically start with your system, go to settings and toggle "Auto start" on.

## Activity check

By default, plugins will only be started when VRChat is detected to be running. If you need them running for testing
outside VRChat, you can force start them through the settings panel. Plugins are running when the tray icon turns
green.

## Logging

The application normally logs all messages with info level and higher to the console as well as to a rotating log file.
In case you experience any unexpected crashes or behaviours, you should create a bug report with the latest log file
attached. If you need more verbose logs, you can run the application from a terminal with `RUST_LOG=debug`.

Please note that on Windows you will not see any debug output on the console with a release build.

To find the log folder, simply open the settings panel and click "Open logs folder".

## Dark mode

The application tries to auto-detect whether it needs to use light or dark icons in your tray bar. If this method fails,
you can force a specific icon style in the settings.

## OS support

Both Linux and Windows are supported, though Linux is the primarily tested platform.

## Plugins

### Media Control

This plugin allows you to control your local media player from VRChat without relying on overlays. All you need is to
set up is a menu within your avatar with buttons controlling the following booleans:

- `MC_PrevTrack`
- `MC_NextTrack`
- `MC_PlayPause`
- `MC_Stop`

### Watch

This plugin drives the [OSC Watch VRChat accessory](https://booth.pm/en/items/3687002) component. It implements the
same functionality as the original application minus the functionality of toggling the Discord microphone.

### PiShock

This plugin controls a user configurable [PiShock](https://pishock.com) instance. It is driven through the following
VRChat parameters:

| Parameter               | Type    | Description                                                                                       |
|-------------------------|---------|---------------------------------------------------------------------------------------------------|
| `PS_Minus_Pressed`      | `bool`  | Intensity decrease button pressed                                                                 |
| `PS_Plus_Pressed`       | `bool`  | Intensity increase button pressed                                                                 |
| `PS_ShockLeft_Pressed`  | `bool`  | Left shock button pressed                                                                         |
| `PS_ShockRight_Pressed` | `bool`  | Right shock button pressed                                                                        |
| `PS_Intensity`          | `float` | Intensity going from 0.0 to 1.0                                                                   |
| `PS_IntensityCap`       | `float` | Intensity cap going from 0.0 to 1.0                                                               |
| `PS_QuickShock`         | `float` | Triggers a short shock with the given intensity once. Reset it by setting it to a negative value. |
| `PS_ShockActive`        | `bool`  | Set to true while a shock is active, then automatically reset to false.                           |

You can configure the duration (default 4) through the configuration file. You must also set your credentials in there.
The configuration allows you to configure one or more codes to be triggered for each shock.

The intensity and intensity cap are periodically saved after 10 seconds of being changed. When an avatar loads in, it
will automatically be populated with the last values.

Quick shocks are always send with a duration of 1 second. You can trigger these with your own contact receivers, e.g.
by driving the variable through an animation controller.

If you are looking for a ready-made UI, you can use my [VRChat PiShock Controller](https://dasprid.gumroad.com/l/llfyq).
Otherwise, you can implement your own UI on your avatar. The pressed parameters are only read by the plugin, while the
intensity parameter is both read and written, so you can also control it via a radial menu.

In order to initiate the shock, both shock buttons have to be pressed at the same time.
