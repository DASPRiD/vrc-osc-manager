#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

mod config;
mod osc;
mod plugins;
mod tray;

use crate::config::{load_config, Config};
use crate::tray::TrayMessage;
use anyhow::{bail, Context, Result};
use async_osc::OscMessage;
use clap::Parser;
use directories::BaseDirs;
use file_rotate::compression::Compression;
use file_rotate::suffix::{AppendTimestamp, FileLimit};
use file_rotate::{ContentLimit, FileRotate, TimeFrequency};
use log::{debug, error, info, LevelFilter};
use simplelog::{ColorChoice, CombinedLogger, TermLogger, TerminalMode, WriteLogger};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use sysinfo::{ProcessRefreshKind, RefreshKind, System, SystemExt};
use tokio::select;
use tokio::sync::{broadcast, mpsc};
use tokio::time::sleep;
use tokio_graceful_shutdown::{
    errors::CancelledByShutdown, FutureExt, NestedSubsystem, SubsystemHandle, Toplevel,
};

struct VrChatActivity {
    tx: mpsc::Sender<bool>,
    disabled: bool,
}

impl VrChatActivity {
    fn new(tx: mpsc::Sender<bool>, disabled: bool) -> Self {
        Self { tx, disabled }
    }

    async fn check(&self) -> Result<()> {
        let mut vrchat_running = false;
        let mut sys = System::new();
        let refresh_kind = RefreshKind::new().with_processes(ProcessRefreshKind::new());

        loop {
            debug!("Checking if VRChat is running");
            sys.refresh_specifics(refresh_kind);
            let running = sys.processes_by_name("VRChat").next().is_some();

            if running != vrchat_running {
                vrchat_running = running;
                self.tx.send(vrchat_running).await?;

                debug!(
                    "VRChat has {}",
                    if vrchat_running { "started" } else { "stopped" }
                );
            }

            sleep(Duration::from_secs(20)).await;
        }
    }

    async fn run(self, subsys: SubsystemHandle) -> Result<()> {
        if self.disabled {
            self.tx.send(true).await?;
            subsys.on_shutdown_requested().await;
            return Ok(());
        }

        match (self.check().cancel_on_shutdown(&subsys)).await {
            Ok(Ok(())) => subsys.request_shutdown(),
            Ok(Err(error)) => return Err(error),
            Err(CancelledByShutdown) => {}
        }

        Ok(())
    }
}

async fn run_plugins(
    subsys: SubsystemHandle,
    config: Arc<Config>,
    data_dir: PathBuf,
    receiver_tx: broadcast::Sender<OscMessage>,
    sender_tx: mpsc::Sender<OscMessage>,
) -> Result<()> {
    #[cfg(feature = "watch")]
    {
        let sender_tx = sender_tx.clone();
        subsys.start("PluginWatch", |subsys| {
            plugins::watch::Watch::new(sender_tx).run(subsys)
        });
    }

    #[cfg(feature = "pishock")]
    {
        let sender_tx = sender_tx.clone();
        let receiver_rx = receiver_tx.subscribe();
        subsys.start("PluginPiShock", |subsys| {
            plugins::pishock::PiShock::new(sender_tx, receiver_rx, config, data_dir).run(subsys)
        });
    }

    subsys.on_shutdown_requested().await;
    Ok(())
}

struct Launcher {
    rx: mpsc::Receiver<bool>,
    config: Arc<Config>,
    data_dir: PathBuf,
    receiver_tx: broadcast::Sender<OscMessage>,
    sender_tx: mpsc::Sender<OscMessage>,
    dark_mode_icons: bool,
}

impl Launcher {
    fn new(
        rx: mpsc::Receiver<bool>,
        config: Arc<Config>,
        data_dir: PathBuf,
        receiver_tx: broadcast::Sender<OscMessage>,
        sender_tx: mpsc::Sender<OscMessage>,
        dark_mode_icons: bool,
    ) -> Self {
        Self {
            rx,
            config,
            data_dir,
            receiver_tx,
            sender_tx,
            dark_mode_icons,
        }
    }

    async fn wait(&mut self, subsys: &SubsystemHandle) -> Result<()> {
        let (tray_tx, mut tray_rx) = mpsc::channel(4);
        let mut tray = tray::Tray::new(tray_tx, self.dark_mode_icons)?;
        let mut maybe_plugin_subsys: Option<NestedSubsystem> = None;

        loop {
            select! {
                Some(message) = tray_rx.recv() => {
                    match message {
                        TrayMessage::ReloadPlugins => {
                            info!("Reloading plugins");
                            self.config = Arc::new(load_config().await?);

                            if let Some(plugin_subsys) = maybe_plugin_subsys {
                                subsys.perform_partial_shutdown(plugin_subsys).await?;

                                let config = self.config.clone();
                                let receiver_tx = self.receiver_tx.clone();
                                let sender_tx = self.sender_tx.clone();
                                let data_dir = self.data_dir.clone();

                                maybe_plugin_subsys = Some(subsys.start("Plugins", move |subsys| {
                                    run_plugins(subsys, config, data_dir, receiver_tx, sender_tx)
                                }));
                            }
                        }
                        TrayMessage::Exit => {
                            subsys.request_shutdown();
                        }
                    }
                }
                Some(vrchat_running) = self.rx.recv() => {
                    if vrchat_running {
                        if maybe_plugin_subsys.is_none() {
                            info!("Starting plugins");
                            tray.set_running(true)?;

                            let config = self.config.clone();
                            let receiver_tx = self.receiver_tx.clone();
                            let sender_tx = self.sender_tx.clone();
                            let data_dir = self.data_dir.clone();

                            maybe_plugin_subsys = Some(subsys.start("Plugins", move |subsys| {
                                run_plugins(subsys, config, data_dir, receiver_tx, sender_tx)
                            }));
                        }
                    } else if !vrchat_running {
                        if let Some(plugin_subsys) = maybe_plugin_subsys {
                            info!("Stopping plugins");
                            tray.set_running(false)?;

                            subsys.perform_partial_shutdown(plugin_subsys).await?;
                            maybe_plugin_subsys = None;
                        }
                    }
                }
                else => {
                    bail!("Select yielded an unexpected result while waiting for activity message")
                }
            }
        }
    }

    async fn run(mut self, subsys: SubsystemHandle) -> Result<()> {
        match (self.wait(&subsys).cancel_on_shutdown(&subsys)).await {
            Ok(Ok(())) => subsys.request_shutdown(),
            Ok(Err(error)) => return Err(error),
            Err(CancelledByShutdown) => {}
        }

        Ok(())
    }
}

#[derive(Parser)]
struct Args {
    /// Use icons optimized for dark mode
    #[arg(long, default_value_t = false)]
    dark_mode_icons: bool,

    /// Run all plugins, even when VRChat is not running
    #[arg(long, default_value_t = false)]
    disable_activity_check: bool,

    /// Enable debug logging
    #[arg(long, default_value_t = false)]
    debug: bool,
}

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    let base_dirs = BaseDirs::new().context("Base directories not available")?;
    let data_dir = base_dirs.data_dir().join("vrc-osc-manager");
    let log_dir = data_dir.join("logs/log");

    let log_file = FileRotate::new(
        log_dir,
        AppendTimestamp::default(FileLimit::MaxFiles(12)),
        ContentLimit::Time(TimeFrequency::Hourly),
        Compression::None,
        #[cfg(unix)]
        None,
    );

    let log_filter = if args.debug {
        LevelFilter::Debug
    } else {
        LevelFilter::Info
    };

    CombinedLogger::init(vec![
        TermLogger::new(
            log_filter,
            simplelog::Config::default(),
            TerminalMode::Mixed,
            ColorChoice::Auto,
        ),
        WriteLogger::new(log_filter, simplelog::Config::default(), log_file),
    ])?;

    info!("Starting VRC OSC Manager v{}", VERSION);

    let config = Arc::new(load_config().await?);
    let (tx, rx) = mpsc::channel(2);

    let (sender_tx, sender_rx) = mpsc::channel(128);
    let (receiver_tx, _) = broadcast::channel(128);
    let launcher_receiver_tx = receiver_tx.clone();

    let send_port = config.osc.send_port;
    let receive_port = config.osc.receive_port;

    let result = Toplevel::new()
        .start("VrChatActivity", move |subsys| {
            VrChatActivity::new(tx, args.disable_activity_check).run(subsys)
        })
        .start("Launcher", move |subsys| {
            Launcher::new(
                rx,
                config,
                data_dir,
                launcher_receiver_tx,
                sender_tx,
                args.dark_mode_icons,
            )
            .run(subsys)
        })
        .start("OscSender", move |subsys| {
            osc::Sender::new(sender_rx, send_port).run(subsys)
        })
        .start("OscReceiver", move |subsys| {
            osc::Receiver::new(receiver_tx, receive_port).run(subsys)
        })
        .catch_signals()
        .handle_shutdown_requests(Duration::from_millis(1000))
        .await;

    if let Err(error) = result {
        error!("Program crash occurred: {}", error);
        return Err(error.into());
    }

    Ok(())
}
