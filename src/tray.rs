use anyhow::Result;
use tray_item::{IconSource, TrayItem};

#[cfg(target_os = "linux")]
const INACTIVE_ICON: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/linux-inactive-icon"));

#[cfg(target_os = "linux")]
const ACTIVE_ICON: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/linux-active-icon"));

#[cfg(target_os = "linux")]
fn get_inactive_icon() -> IconSource {
    IconSource::Data {
        width: 64,
        height: 64,
        data: INACTIVE_ICON.to_vec(),
    }
}

#[cfg(target_os = "linux")]
fn get_active_icon() -> IconSource {
    IconSource::Data {
        width: 64,
        height: 64,
        data: ACTIVE_ICON.to_vec(),
    }
}

#[cfg(target_os = "windows")]
fn get_inactive_icon() -> IconSource {
    IconSource::Resource("inactive-icon")
}

#[cfg(target_os = "windows")]
fn get_active_icon() -> IconSource {
    IconSource::Resource("active-icon")
}

pub struct Tray {
    tray: TrayItem,
}

impl Tray {
    pub fn new() -> Result<Self> {
        let mut tray = TrayItem::new("VRC OSC Manager", get_inactive_icon())?;

        tray.add_menu_item("Exit", || {
            std::process::exit(0);
        })?;

        Ok(Self { tray })
    }

    pub fn set_running(&mut self, running: bool) -> Result<()> {
        self.tray.set_icon(if running {
            get_active_icon()
        } else {
            get_inactive_icon()
        })?;
        Ok(())
    }
}
