use anyhow::{Context, Result};
use directories::BaseDirs;
use serde::Deserialize;
use tokio::fs::File;
use tokio::io::AsyncReadExt;

#[derive(Debug, Clone, Deserialize)]
pub struct OscConfig {
    pub send_port: u16,
    pub receive_port: u16,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PiShockConfig {
    pub username: String,
    pub api_key: String,
    pub code: String,
    pub duration: u8,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub osc: OscConfig,
    pub pishock: PiShockConfig,
}

pub async fn load_config() -> Result<Config> {
    let base_dirs = BaseDirs::new().context("Base directories not available")?;
    let home_dir = base_dirs.config_dir();

    let path = home_dir.join("osc-manager.toml");
    let mut file = File::open(&path)
        .await
        .with_context(|| format!("Failed to open {}", path.display()))?;
    let mut toml_config = String::new();
    file.read_to_string(&mut toml_config).await?;
    let config: Config = toml::from_str(&toml_config)?;
    Ok(config)
}
