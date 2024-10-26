use crate::platform::Platform;
use anyhow::Context;
use directories::BaseDirs;
use indoc::indoc;
use log::debug;
use std::env;
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::process::Command;

pub struct LinuxPlatform;

impl LinuxPlatform {
    fn auto_start_path(&self) -> anyhow::Result<PathBuf> {
        let base_dirs = BaseDirs::new().context("Base directories not available")?;
        let mut auto_start_path = base_dirs.config_dir().to_path_buf();
        auto_start_path.push("autostart");
        auto_start_path.push("vrc-osc-manager.desktop");
        debug!("Auto start path: {:?}", auto_start_path);

        Ok(auto_start_path)
    }
}

#[async_trait::async_trait]
impl Platform for LinuxPlatform {
    fn open_folder(&self, path: &Path) {
        let _ = Command::new("xdg-open").arg(path).spawn();
    }

    fn has_auto_start(&self) -> bool {
        self.auto_start_path().unwrap().exists()
    }

    async fn add_auto_start(&self) -> anyhow::Result<()> {
        let path = self.auto_start_path()?;
        fs::create_dir_all(&path.parent().unwrap()).await?;

        let exec_path = env::current_exe()?
            .to_str()
            .context("Invalid executable path")?
            .to_string();

        let desktop_entry = indoc! {"
            [Desktop Entry]
            Type=Application
            Name=VRC OSC Manager
            Exec={exec_path}
            X-GNOME-Autostart-enabled=true
        "};
        let desktop_entry = desktop_entry.replace("{exec_path}", &exec_path);

        fs::write(&path, desktop_entry).await?;

        Ok(())
    }

    async fn remove_auto_start(&self) -> anyhow::Result<()> {
        let path = self.auto_start_path()?;

        if path.exists() {
            fs::remove_file(&path).await?;
        }

        Ok(())
    }
}
