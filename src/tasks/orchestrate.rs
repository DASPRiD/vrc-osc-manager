use async_trait::async_trait;
use slint::{ComponentHandle, Weak};
use std::path::PathBuf;
use tokio::select;
use tokio::sync::{mpsc, Mutex};
use tokio_graceful_shutdown::errors::CancelledByShutdown;
use tokio_graceful_shutdown::{FutureExt, IntoSubsystem, SubsystemHandle};

use crate::config::{DarkLight, RootConfig};
use crate::platform::{get_platform, Platform};
use crate::tasks::plugin_manager::Command;
use crate::tasks::tray::TrayProperty;
use crate::utils::config::ConfigHandle;
use crate::AppWindow;

pub enum AppEvent {
    VrchatStarted,
    VrchatStopped,
    AppWindowRequested,
    ShutdownRequested,
}

pub enum UiEvent {
    PluginToggle(String, bool),
    TrayIconsToggle(DarkLight),
    AutoStartToggle(bool),
    OpenLogsFolder,
    StartPlugins,
}

pub struct OrchestrateTask {
    app_event_rx: mpsc::Receiver<AppEvent>,
    ui_event_rx: mpsc::Receiver<UiEvent>,
    plugin_manager_tx: mpsc::Sender<Command>,
    tray_property_tx: mpsc::Sender<TrayProperty>,
    app_window: Mutex<Weak<AppWindow>>,
    config: ConfigHandle<RootConfig>,
    logs_dir: PathBuf,
}

impl OrchestrateTask {
    pub fn new(
        app_event_rx: mpsc::Receiver<AppEvent>,
        ui_event_rx: mpsc::Receiver<UiEvent>,
        plugin_manager_tx: mpsc::Sender<Command>,
        tray_property_tx: mpsc::Sender<TrayProperty>,
        app_window: Weak<AppWindow>,
        config: ConfigHandle<RootConfig>,
        logs_dir: PathBuf,
    ) -> Self {
        Self {
            app_event_rx,
            ui_event_rx,
            plugin_manager_tx,
            tray_property_tx,
            app_window: Mutex::new(app_window),
            config,
            logs_dir,
        }
    }

    async fn main_loop(&mut self, subsys: &SubsystemHandle) -> anyhow::Result<()> {
        loop {
            select! {
                event = self.app_event_rx.recv() => {
                    match event {
                        Some(event) => self.handle_app_event(event, subsys).await?,
                        None => break,
                    }
                }
                event = self.ui_event_rx.recv() => {
                    match event {
                        Some(event) => self.handle_ui_event(event).await?,
                        None => break,
                    }
                }
            }
        }

        Ok(())
    }

    async fn handle_app_event(
        &mut self,
        event: AppEvent,
        subsys: &SubsystemHandle,
    ) -> anyhow::Result<()> {
        match event {
            AppEvent::VrchatStarted => {
                self.plugin_manager_tx.send(Command::StartPlugins).await?;
                self.tray_property_tx
                    .send(TrayProperty::Running(true))
                    .await?;
            }
            AppEvent::VrchatStopped => {
                self.plugin_manager_tx.send(Command::StopPlugins).await?;
                self.tray_property_tx
                    .send(TrayProperty::Running(false))
                    .await?;
            }
            AppEvent::AppWindowRequested => {
                self.app_window
                    .lock()
                    .await
                    .upgrade_in_event_loop(|handle| {
                        // @fixme workaround for the following issue:
                        // https://github.com/slint-ui/slint/issues/4382
                        if handle.window().is_visible() {
                            handle.hide().unwrap();
                        }

                        handle.show().unwrap();
                    })?;
            }
            AppEvent::ShutdownRequested => {
                let _ = slint::quit_event_loop();
                subsys.request_shutdown();
            }
        }

        Ok(())
    }

    async fn handle_ui_event(&mut self, event: UiEvent) -> anyhow::Result<()> {
        match event {
            UiEvent::PluginToggle(plugin_id, enabled) => {
                self.plugin_manager_tx
                    .send(if enabled {
                        Command::EnablePlugin(plugin_id)
                    } else {
                        Command::DisablePlugin(plugin_id)
                    })
                    .await?;
            }
            UiEvent::TrayIconsToggle(mode) => {
                self.tray_property_tx
                    .send(TrayProperty::DarkMode(mode.dark_mode()))
                    .await?;

                self.config
                    .update(|config| {
                        config.dark_light = mode;
                    })
                    .await?;
            }
            UiEvent::AutoStartToggle(enabled) => {
                if enabled {
                    get_platform().add_auto_start().await?;
                } else {
                    get_platform().remove_auto_start().await?;
                }
            }
            UiEvent::OpenLogsFolder => {
                get_platform().open_folder(&self.logs_dir);
            }
            UiEvent::StartPlugins => {
                self.plugin_manager_tx.send(Command::StartPlugins).await?;
                self.tray_property_tx
                    .send(TrayProperty::Running(true))
                    .await?;
            }
        }

        Ok(())
    }
}

#[async_trait]
impl IntoSubsystem<anyhow::Error> for OrchestrateTask {
    async fn run(mut self, subsys: SubsystemHandle) -> anyhow::Result<()> {
        match self.main_loop(&subsys).cancel_on_shutdown(&subsys).await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => return Err(error),
            Err(CancelledByShutdown) => {}
        }

        Ok(())
    }
}
