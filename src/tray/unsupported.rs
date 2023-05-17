use log::warn;

pub struct Tray {}

impl Tray {
    pub fn new() -> Self {
        warn!("Tray not supported on this platform");
        Self {}
    }

    pub fn set_running(&mut self, _running: bool) {}
}
