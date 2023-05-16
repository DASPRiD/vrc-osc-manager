mod config;
mod osc;
mod plugins;

use crate::config::{load_config, Config};
use anyhow::{Context, Result};
use image::Rgba;
use ksni::{Icon, MenuItem};
use log::{debug, info};
use std::sync::Arc;
use std::time::Duration;
use sysinfo::{ProcessRefreshKind, RefreshKind, System, SystemExt};
use tokio::sync::{broadcast, mpsc};
use tokio::time::sleep;
use tokio_graceful_shutdown::{
    errors::CancelledByShutdown, FutureExt, NestedSubsystem, SubsystemHandle, Toplevel,
};

fn convert(img: &[u8]) -> Result<Vec<u8>> {
    let img = image::load_from_memory(img)?;
    let mut img = img.to_rgba8();

    for Rgba(pixel) in img.pixels_mut() {
        *pixel = u32::from_be_bytes(*pixel).rotate_right(8).to_be_bytes();
    }

    Ok(img.into_raw())
}

const STANDARD_ICON: &[u8] = include_bytes!("../assets/icon.png");
const ACTIVE_ICON: &[u8] = include_bytes!("../assets/icon-active.png");

struct ApplicationTray {
    running: bool,
}

impl ksni::Tray for ApplicationTray {
    fn id(&self) -> String {
        "osx-manager".to_string()
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        vec![Icon {
            width: 64,
            height: 64,
            data: convert(if self.running {
                ACTIVE_ICON
            } else {
                STANDARD_ICON
            })
            .unwrap(),
        }]
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        use ksni::menu::*;

        vec![StandardItem {
            label: "Exit".into(),
            activate: Box::new(|_| std::process::exit(0)),
            ..Default::default()
        }
        .into()]
    }
}

struct VrChatActivity {
    tx: mpsc::Sender<bool>,
}

impl VrChatActivity {
    fn new(tx: mpsc::Sender<bool>) -> Self {
        Self { tx }
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

                info!(
                    "VRChat has {}",
                    if vrchat_running { "started" } else { "stopped" }
                );
            }

            sleep(Duration::from_secs(20)).await;
        }
    }

    async fn run(self, subsys: SubsystemHandle) -> Result<()> {
        match (self.check().cancel_on_shutdown(&subsys)).await {
            Ok(Ok(())) => subsys.request_shutdown(),
            Ok(Err(error)) => return Err(error),
            Err(CancelledByShutdown) => {}
        }

        Ok(())
    }
}

async fn run_plugins(subsys: SubsystemHandle, config: Arc<Config>) -> Result<()> {
    let (sender_tx, sender_rx) = mpsc::channel(64);
    let (receiver_tx, receiver_rx) = broadcast::channel(64);
    let plugin_watch_sender = sender_tx.clone();

    let send_port = config.osc.send_port;
    let receive_port = config.osc.receive_port;

    subsys.start("PluginWatch", |subsys| {
        plugins::watch::Watch::new(plugin_watch_sender).run(subsys)
    });
    subsys.start("PluginPiShock", |subsys| {
        plugins::pishock::PiShock::new(sender_tx, receiver_rx, config).run(subsys)
    });
    subsys.start("OscSender", move |subsys| {
        osc::Sender::new(sender_rx, send_port).run(subsys)
    });
    subsys.start("OscReceiver", move |subsys| {
        osc::Receiver::new(receiver_tx, receive_port).run(subsys)
    });

    subsys.on_shutdown_requested().await;
    Ok(())
}

struct Launcher {
    rx: mpsc::Receiver<bool>,
    config: Arc<Config>,
}

impl Launcher {
    fn new(rx: mpsc::Receiver<bool>, config: Arc<Config>) -> Self {
        Self { rx, config }
    }

    async fn wait(&mut self, subsys: &SubsystemHandle) -> Result<()> {
        let service = ksni::TrayService::new(ApplicationTray { running: false });
        let handle = service.handle();
        service.spawn();

        let mut plugin_subsys: Option<NestedSubsystem> = None;

        while let Some(vrchat_running) = self.rx.recv().await {
            if vrchat_running && plugin_subsys.is_none() {
                handle.update(|tray: &mut ApplicationTray| {
                    tray.running = true;
                });

                let config = self.config.clone();
                plugin_subsys = Some(subsys.start("Plugins", |subsys| run_plugins(subsys, config)));
            } else if !vrchat_running && plugin_subsys.is_some() {
                handle.update(|tray: &mut ApplicationTray| {
                    tray.running = false;
                });

                subsys
                    .perform_partial_shutdown(plugin_subsys.context("Plugin subsys not some")?)
                    .await?;
                plugin_subsys = None;
            }
        }

        Ok(())
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

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    let config = Arc::new(load_config().await?);
    let (tx, rx) = mpsc::channel(2);

    Toplevel::new()
        .start("VrChatActivity", |subsys| {
            VrChatActivity::new(tx).run(subsys)
        })
        .start("Launcher", |subsys| Launcher::new(rx, config).run(subsys))
        .catch_signals()
        .handle_shutdown_requests(Duration::from_millis(1000))
        .await
        .map_err(Into::into)
}
