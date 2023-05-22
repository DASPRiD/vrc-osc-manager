use anyhow::Result;
use tokio::sync::mpsc;
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

#[derive(Debug)]
pub enum TrayMessage {
    ReloadPlugins,
    Exit,
}

pub struct Tray {
    tray: TrayItem,
    dark_mode_icons: bool,
}

impl Tray {
    pub fn new(message_tx: mpsc::Sender<TrayMessage>, dark_mode_icons: bool) -> Result<Self> {
        let mut tray = TrayItem::new("VRC OSC Manager", get_inactive_icon(dark_mode_icons))?;

        let reload_plugins_tx = message_tx.clone();
        tray.add_menu_item("Reload plugins", move || {
            reload_plugins_tx
                .blocking_send(TrayMessage::ReloadPlugins)
                .unwrap();
        })?;

        tray.add_menu_item("Exit", move || {
            message_tx.blocking_send(TrayMessage::Exit).unwrap();
        })?;

        Ok(Self {
            tray,
            dark_mode_icons,
        })
    }

    pub fn set_running(&mut self, running: bool) -> Result<()> {
        self.tray.set_icon(if running {
            get_active_icon(self.dark_mode_icons)
        } else {
            get_inactive_icon(self.dark_mode_icons)
        })?;
        Ok(())
    }
}
