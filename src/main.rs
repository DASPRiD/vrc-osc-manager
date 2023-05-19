mod config;
mod osc;
mod plugins;
mod tray;

use crate::config::{load_config, Config};
use anyhow::Result;
use async_osc::OscMessage;
use log::{debug, info};
use std::sync::Arc;
use std::time::Duration;
use sysinfo::{ProcessRefreshKind, RefreshKind, System, SystemExt};
use tokio::sync::{broadcast, mpsc};
use tokio::time::sleep;
use tokio_graceful_shutdown::{
    errors::CancelledByShutdown, FutureExt, NestedSubsystem, SubsystemHandle, Toplevel,
};

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

async fn run_plugins(
    subsys: SubsystemHandle,
    config: Arc<Config>,
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
            plugins::pishock::PiShock::new(sender_tx, receiver_rx, config).run(subsys)
        });
    }

    subsys.on_shutdown_requested().await;
    Ok(())
}

struct Launcher {
    rx: mpsc::Receiver<bool>,
    config: Arc<Config>,
    receiver_tx: broadcast::Sender<OscMessage>,
    sender_tx: mpsc::Sender<OscMessage>,
}

impl Launcher {
    fn new(
        rx: mpsc::Receiver<bool>,
        config: Arc<Config>,
        receiver_tx: broadcast::Sender<OscMessage>,
        sender_tx: mpsc::Sender<OscMessage>,
    ) -> Self {
        Self {
            rx,
            config,
            receiver_tx,
            sender_tx,
        }
    }

    async fn wait(&mut self, subsys: &SubsystemHandle) -> Result<()> {
        let mut tray = tray::Tray::new();
        let mut plugin_subsys: Option<NestedSubsystem> = None;

        while let Some(vrchat_running) = self.rx.recv().await {
            if vrchat_running {
                if plugin_subsys.is_none() {
                    tray.set_running(true);

                    let config = self.config.clone();
                    let receiver_tx = self.receiver_tx.clone();
                    let sender_tx = self.sender_tx.clone();

                    plugin_subsys = Some(subsys.start("Plugins", move |subsys| {
                        run_plugins(subsys, config, receiver_tx, sender_tx)
                    }));
                }
            } else if !vrchat_running {
                if let Some(plugin_subsys) = plugin_subsys {
                    tray.set_running(false);

                    subsys.perform_partial_shutdown(plugin_subsys).await?;
                }

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

    let (sender_tx, sender_rx) = mpsc::channel(64);
    let (receiver_tx, _) = broadcast::channel(64);
    let launcher_receiver_tx = receiver_tx.clone();

    let send_port = config.osc.send_port;
    let receive_port = config.osc.receive_port;

    Toplevel::new()
        .start("VrChatActivity", |subsys| {
            VrChatActivity::new(tx).run(subsys)
        })
        .start("Launcher", move |subsys| {
            Launcher::new(rx, config, launcher_receiver_tx, sender_tx).run(subsys)
        })
        .start("OscSender", move |subsys| {
            osc::Sender::new(sender_rx, send_port).run(subsys)
        })
        .start("OscReceiver", move |subsys| {
            osc::Receiver::new(receiver_tx, receive_port).run(subsys)
        })
        .catch_signals()
        .handle_shutdown_requests(Duration::from_millis(1000))
        .await
        .map_err(Into::into)
}
