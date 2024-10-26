use std::collections::HashMap;
use std::sync::Arc;

use crate::osc_query::service::OscQueryServiceBuilder;
use crate::utils::config::ConfigManager;
use crate::AppWindow;
use async_osc::OscMessage;
use async_trait::async_trait;
use slint::Weak;
use tokio::sync::{broadcast, mpsc};
use tokio_graceful_shutdown::SubsystemHandle;

pub mod media_control;
pub mod pishock;
pub mod watch;

#[async_trait]
pub trait Plugin: Send + Sync {
    fn new(config_manager: ConfigManager) -> Self
    where
        Self: Sized;

    fn title(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn info_url(&self) -> Option<&'static str> {
        None
    }

    async fn run(
        &self,
        subsys: &SubsystemHandle,
        channels: Arc<ChannelManager>,
    ) -> anyhow::Result<()>;

    fn register_osc_parameters(&self, service: &mut OscQueryServiceBuilder);

    fn has_settings(&self) -> bool {
        false
    }

    fn register_settings_callbacks(self: Arc<Self>, _app_window: &AppWindow) -> anyhow::Result<()> {
        Ok(())
    }

    fn open_settings(self: Arc<Self>, _app_window: Weak<AppWindow>) -> anyhow::Result<()> {
        Ok(())
    }
}

pub struct ChannelManager {
    osc_broadcast: broadcast::Sender<OscMessage>,
    osc_sender: mpsc::Sender<OscMessage>,
}

impl ChannelManager {
    pub fn new(
        osc_broadcast: broadcast::Sender<OscMessage>,
        osc_sender: mpsc::Sender<OscMessage>,
    ) -> Self {
        Self {
            osc_broadcast,
            osc_sender,
        }
    }

    pub fn subscribe_to_osc(&self) -> broadcast::Receiver<OscMessage> {
        self.osc_broadcast.subscribe()
    }

    pub fn create_osc_sender(&self) -> mpsc::Sender<OscMessage> {
        self.osc_sender.clone()
    }
}

macro_rules! define_plugins {
    (
        {
            $( $plugin_id:ident : $plugin:ident ),* $(,)?
        }
    ) => {
        pub fn get_plugins(
            mut config_manager: ConfigManager
        ) -> HashMap<&'static str, Arc<dyn Plugin>> {
            let mut map = HashMap::new();
            $(
                let plugin = Arc::new($plugin_id::$plugin::new(
                    config_manager.with_plugin_id(stringify!($plugin_id))
                ));
                map.insert(stringify!($plugin_id), plugin as Arc<dyn Plugin>);
            )*
            map
        }
    };
}

define_plugins!({
    media_control: MediaControl,
    watch: Watch,
    pishock: PiShock,
});
