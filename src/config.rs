use std::collections::HashSet;

use serde::{Deserialize, Serialize};

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DarkLight {
    Dark,
    Light,
    #[default]
    Default,
}

impl DarkLight {
    pub fn dark_mode(&self) -> bool {
        match self {
            DarkLight::Dark => true,
            DarkLight::Light => false,
            DarkLight::Default => match dark_light::detect() {
                dark_light::Mode::Dark | dark_light::Mode::Default => true,
                dark_light::Mode::Light => false,
            },
        }
    }
}

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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct RootConfig {
    pub osc: OscConfig,
    pub dark_light: DarkLight,
    pub enabled_plugins: HashSet<String>,
}
