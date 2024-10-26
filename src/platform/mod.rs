#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "windows")]
mod windows;

use std::path::Path;

#[async_trait::async_trait]
pub trait Platform {
    fn open_folder(&self, path: &Path);

    fn has_auto_start(&self) -> bool;

    async fn add_auto_start(&self) -> anyhow::Result<()>;

    async fn remove_auto_start(&self) -> anyhow::Result<()>;
}

#[cfg(target_os = "linux")]
pub type PlatformImpl = linux::LinuxPlatform;

#[cfg(target_os = "windows")]
pub type PlatformImpl = windows::WindowsPlatform;

pub fn get_platform() -> PlatformImpl {
    PlatformImpl {}
}
