use crate::tasks::orchestrate::AppEvent;
use async_trait::async_trait;
use tokio::sync::mpsc;
use tokio_graceful_shutdown::errors::CancelledByShutdown;
use tokio_graceful_shutdown::{FutureExt, IntoSubsystem, SubsystemHandle};
use tray_item::{IconSource, TrayItem};

#[cfg(target_os = "linux")]
const DARK_INACTIVE_ICON: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/linux-dark-inactive-icon"));

#[cfg(target_os = "linux")]
const DARK_ACTIVE_ICON: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/linux-dark-active-icon"));

#[cfg(target_os = "linux")]
const LIGHT_INACTIVE_ICON: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/linux-light-inactive-icon"));

#[cfg(target_os = "linux")]
const LIGHT_ACTIVE_ICON: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/linux-light-active-icon"));

#[cfg(target_os = "linux")]
fn get_inactive_icon(dark_mode: bool) -> IconSource {
    IconSource::Data {
        width: 64,
        height: 64,
        data: if dark_mode {
            DARK_INACTIVE_ICON
        } else {
            LIGHT_INACTIVE_ICON
        }
        .to_vec(),
    }
}

#[cfg(target_os = "linux")]
fn get_active_icon(dark_mode: bool) -> IconSource {
    IconSource::Data {
        width: 64,
        height: 64,
        data: if dark_mode {
            DARK_ACTIVE_ICON
        } else {
            LIGHT_ACTIVE_ICON
        }
        .to_vec(),
    }
}

#[cfg(target_os = "windows")]
fn get_inactive_icon(dark_mode: bool) -> IconSource {
    IconSource::Resource(if dark_mode {
        "dark_inactive_icon"
    } else {
        "light_inactive_icon"
    })
}

#[cfg(target_os = "windows")]
fn get_active_icon(dark_mode: bool) -> IconSource {
    IconSource::Resource(if dark_mode {
        "dark_active_icon"
    } else {
        "light_active_icon"
    })
}

pub enum TrayProperty {
    Running(bool),
    DarkMode(bool),
}

pub struct TrayTask {
    rx: mpsc::Receiver<TrayProperty>,
    app_event_tx: mpsc::Sender<AppEvent>,
    running: bool,
    dark_mode: bool,
}

impl TrayTask {
    pub fn new(
        rx: mpsc::Receiver<TrayProperty>,
        app_event_tx: mpsc::Sender<AppEvent>,
        dark_mode: bool,
    ) -> Self {
        Self {
            rx,
            app_event_tx,
            running: false,
            dark_mode,
        }
    }

    async fn main_loop(&mut self) -> anyhow::Result<()> {
        let mut tray = TrayItem::new("VRC OSC Manager", get_inactive_icon(self.dark_mode))?;

        tray.add_menu_item("Open VRC OSC Manager", {
            let tx = self.app_event_tx.clone();
            move || {
                tx.blocking_send(AppEvent::AppWindowRequested).unwrap();
            }
        })?;

        tray.add_menu_item("Quit", {
            let tx = self.app_event_tx.clone();
            move || {
                tx.blocking_send(AppEvent::ShutdownRequested).unwrap();
            }
        })?;

        while let Some(property) = self.rx.recv().await {
            match property {
                TrayProperty::Running(running) => {
                    self.running = running;
                }
                TrayProperty::DarkMode(dark_mode) => {
                    self.dark_mode = dark_mode;
                }
            }

            match self.running {
                true => tray.set_icon(get_active_icon(self.dark_mode))?,
                false => tray.set_icon(get_inactive_icon(self.dark_mode))?,
            }
        }

        Ok(())
    }
}

#[async_trait]
impl IntoSubsystem<anyhow::Error> for TrayTask {
    async fn run(mut self, subsys: SubsystemHandle) -> anyhow::Result<()> {
        match self.main_loop().cancel_on_shutdown(&subsys).await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => return Err(error),
            Err(CancelledByShutdown) => {}
        }

        Ok(())
    }
}
