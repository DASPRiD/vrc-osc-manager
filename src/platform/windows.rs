use crate::platform::Platform;
use anyhow::Context;
use std::env;
use std::path::Path;
use tokio::process::Command;
use winreg::enums::*;
use winreg::RegKey;

pub struct WindowsPlatform;

#[async_trait::async_trait]
impl Platform for WindowsPlatform {
    fn open_folder(&self, path: &Path) {
        let _ = Command::new("explorer").arg(path).spawn();
    }

    fn has_auto_start(&self) -> bool {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let path = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";

        if let Ok(key) = hkcu.open_subkey(path) {
            key.get_value::<String, _>("vrc-osc-manager").is_ok()
        } else {
            false
        }
    }

    async fn add_auto_start(&self) -> anyhow::Result<()> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let path = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";

        let exec_path = env::current_exe()?
            .to_str()
            .context("Invalid executable path")?
            .to_string();

        let (key, _) = hkcu.create_subkey(path)?;
        key.set_value("vrc-osc-manager", &exec_path)?;

        Ok(())
    }

    async fn remove_auto_start(&self) -> anyhow::Result<()> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let path = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
        let key = hkcu.open_subkey(path)?;

        key.delete_value("vrc-osc-manager")?;

        Ok(())
    }
}
