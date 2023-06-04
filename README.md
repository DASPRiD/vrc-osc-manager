# VRChat OSC Manager

[![Release](https://github.com/DASPRiD/vrc-osc-manager/actions/workflows/release.yml/badge.svg)](https://github.com/DASPRiD/vrc-osc-manager/actions/workflows/release.yml)

This is an OSC manager for handling multiple VRChat OSC plugins at once. 

## Configuration

Before you run the program, you should create a configuration file named `vrc-osc-manager.toml` in your config 
directory. On Linux, that'd be `~/.config`, on Windows, that'd be `C:\Users\username\Application Data`. If the file does 
not exist, the OSC Manager will create it with default values.

You can find the skeleton for that config file in the `examples` folder.

## Compiling

When compiling the application, it will include all plugins by default. You can opt into only including specific plugins
by only enabling the features you want. For a full list of features, have a look at the `Cargo.toml` file.

## Usage

Simply place the binary in your user autostart. It will check the process list every 20 seconds and automatically boot
up all plugins once it detects VRChat to be running. After VRChat has stopped, all plugins will be stopped again as
well.

This is indicated in your tray bar through the `OSC` icon. When it's inactive, it will be gray, otherwise green.

Via the tray icon menu you also have two options available:

- Exit the application
- Reload plugins: This will reload the entire plugin config in case you changed it on disk.

## Activity check

By default, plugins will only be started when VRChat is detected to be running. If you need them running for testing
outside VRChat, you can disable the activity check by passing `--disable-activity-check` as command line argument.

## Logging

The application normally logs all messages with info level and higher to the console as well as to a rotating log file.
In case you experience any unexpected crashes or behaviours, you should create a bug report with the latest log file
attached. To generate more verbose logging, you can pass the `--debug` command line argument.

Please note that on Windows oyu will not see any debug output on the console with a release build.

Log files can be found on Linux under `~/.local/share/vrc-osc-manager\logs`. On Windows they should be located under
`C:\Users\username\Application Data\vrc-osc-manager\logs`. The latest log file is always called `log`, while older
ones are suffixed with a timestamp. Log files are rotated every hour and a maximum of 12 log files is every kept.

## Dark mode

Depending on your operating system theme, the default light icons might not be visible in your tray bar. You can switch
to icons for dark mode by passing `--dark-mode-icons` as command line argument.

## OS support

Both Linux and Windows are supported, though Linux is the primarily tested platform.

## Plugins

### Watch

This plugin drives the [OSC Watch VRChat accessory](https://booth.pm/en/items/3687002) component.  It implements the
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

The intensity and intensity cap are periodically saved after 10 seconds of being changed. When an avatar loads in, it
will automatically be populated with the last values.

Quick shocks are always send with a duration of 1 second. You can trigger these with your own contact receivers, e.g.
by driving the variable through an animation controller.

If you are looking for a ready-made UI, you can use my [VRChat PiShock Controller](https://dasprid.gumroad.com/l/llfyq). 
Otherwise, you can implement your own UI on your avatar. The pressed parameters are only read by the plugin, while the
intensity parameter is both read and written, so you can also control it via a radial menu.

In order to initiate the shock, both shock buttons have to be pressed at the same time.
