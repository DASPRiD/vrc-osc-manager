use std::fs::File;
use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use log::{debug, error};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::error::SendError;
use tokio::sync::{mpsc, RwLock, RwLockReadGuard, RwLockWriteGuard};

use crate::tasks::config_writer::WriteConfigRequest;

#[derive(Clone)]
pub struct ConfigHandle<T>
where
    T: Serialize,
{
    config: Arc<RwLock<T>>,
    file_path: PathBuf,
    debounce: Option<Duration>,
    sender: mpsc::Sender<WriteConfigRequest>,
}

impl<T> ConfigHandle<T>
where
    T: Serialize,
{
    pub fn blocking_update<F, R>(&self, modify_fn: F) -> Result<R, SendError<WriteConfigRequest>>
    where
        F: FnOnce(&mut T) -> R,
    {
        let (request, result) = {
            let mut config = self.config.blocking_write();
            let result = modify_fn(&mut config);
            (self.write_config_request(&config), result)
        };

        self.sender.blocking_send(request)?;
        Ok(result)
    }

    pub fn blocking_read(&self) -> RwLockReadGuard<T> {
        self.config.blocking_read()
    }

    pub async fn update<F, R>(&self, modify_fn: F) -> Result<R, SendError<WriteConfigRequest>>
    where
        F: FnOnce(&mut RwLockWriteGuard<T>) -> R,
    {
        let (request, result) = {
            let mut config = self.config.write().await;
            let result = modify_fn(&mut config);
            (self.write_config_request(&config), result)
        };

        self.sender.send(request).await?;
        Ok(result)
    }

    pub async fn read(&self) -> RwLockReadGuard<T> {
        self.config.read().await
    }

    fn write_config_request(&self, config: &T) -> WriteConfigRequest {
        WriteConfigRequest {
            config: toml::to_string_pretty(&config).expect("Serialization of config failed"),
            path: self.file_path.clone(),
            debounce: self.debounce,
        }
    }
}

pub struct ConfigManager {
    config_dir: PathBuf,
    plugin_id: Option<&'static str>,
    sender: mpsc::Sender<WriteConfigRequest>,
}

impl ConfigManager {
    pub fn new<P: Into<PathBuf>>(config_dir: P, sender: mpsc::Sender<WriteConfigRequest>) -> Self {
        Self {
            config_dir: config_dir.into(),
            plugin_id: None,
            sender,
        }
    }

    pub fn with_plugin_id(&mut self, id: &'static str) -> Self {
        Self {
            config_dir: self.config_dir.clone(),
            plugin_id: Some(id),
            sender: self.sender.clone(),
        }
    }

    pub fn load_config<T>(
        &self,
        name: Option<&str>,
        debounce_write: Option<Duration>,
    ) -> ConfigHandle<T>
    where
        T: Serialize + for<'de> Deserialize<'de> + Default + Clone,
    {
        let path = match self.plugin_id {
            Some(id) => {
                let path = self.config_dir.join("plugins").join(id);

                match name {
                    Some(name) => path.join(format!("{}.toml", name)),
                    None => path.join("config.toml"),
                }
            }
            None => self.config_dir.join("config.toml"),
        };

        let config = self.load_config_from_file(&path);

        ConfigHandle {
            config: Arc::new(RwLock::new(config)),
            file_path: path,
            debounce: debounce_write,
            sender: self.sender.clone(),
        }
    }

    fn load_config_from_file<T>(&self, path: &Path) -> T
    where
        T: Serialize + for<'de> Deserialize<'de> + Default + Clone,
    {
        let mut file = match File::open(path) {
            Ok(file) => file,
            Err(ref error) if error.kind() == ErrorKind::NotFound => {
                debug!(
                    "Config file {} does not exist, falling back to default",
                    path.to_string_lossy()
                );
                return T::default();
            }
            Err(error) => {
                error!(
                    "Failed to open config file {}, falling back to default: {:?}",
                    path.to_string_lossy(),
                    error
                );
                return T::default();
            }
        };

        let mut toml_config = String::new();

        if let Err(error) = file.read_to_string(&mut toml_config) {
            error!(
                "Failed to read config file {}, falling back to default: {:?}",
                path.to_string_lossy(),
                error
            );
            return T::default();
        }

        match toml::from_str(&toml_config) {
            Ok(config) => config,
            Err(error) => {
                error!(
                    "Failed to parse config file {}, falling back to default: {:?}",
                    path.to_string_lossy(),
                    error
                );
                T::default()
            }
        }
    }
}
