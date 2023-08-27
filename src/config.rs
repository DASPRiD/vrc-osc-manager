use anyhow::{Context, Result};
use directories::BaseDirs;
use serde::{Deserialize, Serialize};
use tokio::fs::{metadata, File};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OscConfig {
    pub send_port: u16,
}

impl Default for OscConfig {
    fn default() -> Self {
        Self { send_port: 9000 }
    }
}

#[cfg(feature = "pishock")]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PiShockConfig {
    pub username: String,
    pub api_key: String,
    pub code: String,
    pub duration: u8,
}

impl Default for PiShockConfig {
    fn default() -> Self {
        Self {
            username: "".to_string(),
            api_key: "".to_string(),
            code: "".to_string(),
            duration: 4,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Config {
    pub osc: OscConfig,

    #[cfg(feature = "pishock")]
    pub pishock: PiShockConfig,
}

pub async fn load_config() -> Result<Config> {
    let base_dirs = BaseDirs::new().context("Base directories not available")?;
    let config_dir = base_dirs.config_dir();
    let path = config_dir.join("vrc-osc-manager.toml");

    if metadata(&path).await.is_err() {
        let config: Config = Default::default();
        let mut file = File::create(&path)
            .await
            .with_context(|| format!("Failed to open {}", path.display()))?;
        file.write_all(toml::to_string(&config)?.as_bytes()).await?;
        return Ok(config);
    }

    let mut file = File::open(&path)
        .await
        .with_context(|| format!("Failed to open {}", path.display()))?;
    let mut toml_config = String::new();
    file.read_to_string(&mut toml_config).await?;
    let config: Config = toml::from_str(&toml_config)?;

    Ok(config)
}
