use std::collections::HashMap;
use std::error::Error;
use std::sync::Arc;

use async_trait::async_trait;
use log::{error, warn};
use tokio::sync::mpsc;
use tokio_graceful_shutdown::errors::CancelledByShutdown;
use tokio_graceful_shutdown::{
    FutureExt, IntoSubsystem, NestedSubsystem, SubsystemBuilder, SubsystemHandle,
};

use crate::config::RootConfig;
use crate::plugins::{ChannelManager, Plugin};
use crate::utils::config::ConfigHandle;

pub enum Command {
    StartPlugins,
    StopPlugins,
    EnablePlugin(String),
    DisablePlugin(String),
}

struct PluginHandle {
    plugin: Arc<dyn Plugin>,
    subsys: Option<NestedSubsystem<Box<dyn Error + Send + Sync>>>,
}

pub struct PluginManagerTask {
    rx: mpsc::Receiver<Command>,
    config: ConfigHandle<RootConfig>,
    plugins: HashMap<&'static str, PluginHandle>,
    channel_manager: Arc<ChannelManager>,
}

impl PluginManagerTask {
    pub fn new(
        rx: mpsc::Receiver<Command>,
        config: ConfigHandle<RootConfig>,
        plugins: HashMap<&'static str, Arc<dyn Plugin>>,
        channel_manager: ChannelManager,
    ) -> Self {
        let plugins = plugins
            .into_iter()
            .map(|(id, plugin)| {
                (
                    id,
                    PluginHandle {
                        plugin,
                        subsys: None,
                    },
                )
            })
            .collect();

        Self {
            rx,
            config,
            plugins,
            channel_manager: Arc::new(channel_manager),
        }
    }

    fn start_plugin(
        plugin_id: String,
        plugin: Arc<dyn Plugin>,
        channel_manager: Arc<ChannelManager>,
        subsys: &SubsystemHandle,
    ) -> Option<NestedSubsystem<Box<dyn Error + Send + Sync>>> {
        Some(
            subsys.start(SubsystemBuilder::new(plugin_id, move |subsys| async move {
                match plugin
                    .run(&subsys, channel_manager)
                    .cancel_on_shutdown(&subsys)
                    .await
                {
                    Ok(Ok(())) | Err(CancelledByShutdown) => Ok(()),
                    Ok(err) => err,
                }
            })),
        )
    }

    async fn main_loop(&mut self, subsys: &SubsystemHandle) -> anyhow::Result<()> {
        let mut plugins_started = false;

        while let Some(command) = self.rx.recv().await {
            match command {
                Command::StartPlugins => {
                    let config = self.config.read().await;

                    for plugin_id in config.enabled_plugins.iter() {
                        let container = match self.plugins.get_mut(plugin_id.as_str()) {
                            Some(container) => container,
                            None => {
                                warn!("Unknown plugin found in enabled_plugins: {}", plugin_id);
                                continue;
                            }
                        };

                        container.subsys = Self::start_plugin(
                            plugin_id.clone(),
                            container.plugin.clone(),
                            self.channel_manager.clone(),
                            subsys,
                        );
                    }

                    plugins_started = true;
                }
                Command::StopPlugins => {
                    for container in self.plugins.values_mut() {
                        let subsys = match container.subsys.take() {
                            Some(subsys) => subsys,
                            None => continue,
                        };

                        subsys.initiate_shutdown();
                    }

                    plugins_started = false;
                }
                Command::EnablePlugin(plugin_id) => {
                    let container = match self.plugins.get_mut(plugin_id.as_str()) {
                        Some(plugin) => plugin,
                        None => {
                            error!("Plugin with ID {} not found", plugin_id);
                            continue;
                        }
                    };

                    self.config
                        .update({
                            let plugin_id = plugin_id.clone();

                            |config| {
                                config.enabled_plugins.insert(plugin_id);
                            }
                        })
                        .await?;

                    if !plugins_started || container.subsys.is_some() {
                        continue;
                    }

                    container.subsys = Self::start_plugin(
                        plugin_id.clone(),
                        container.plugin.clone(),
                        self.channel_manager.clone(),
                        subsys,
                    );
                }
                Command::DisablePlugin(plugin_id) => {
                    let container = match self.plugins.get_mut(plugin_id.as_str()) {
                        Some(plugin) => plugin,
                        None => {
                            error!("Plugin with ID {} not found", plugin_id);
                            continue;
                        }
                    };

                    self.config
                        .update(|config| {
                            config.enabled_plugins.remove(&plugin_id);
                        })
                        .await?;

                    if !plugins_started {
                        continue;
                    }

                    let subsys = match container.subsys.take() {
                        Some(subsys) => subsys,
                        None => continue,
                    };

                    subsys.initiate_shutdown();
                }
            }
        }

        Ok(())
    }
}

#[async_trait]
impl IntoSubsystem<anyhow::Error> for PluginManagerTask {
    async fn run(mut self, subsys: SubsystemHandle) -> anyhow::Result<()> {
        match self.main_loop(&subsys).cancel_on_shutdown(&subsys).await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => return Err(error),
            Err(CancelledByShutdown) => {}
        }

        Ok(())
    }
}
