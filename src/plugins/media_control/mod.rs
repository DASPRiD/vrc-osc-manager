use std::sync::Arc;

use async_trait::async_trait;
#[cfg(target_os = "linux")]
use slint::Weak;
use tokio_graceful_shutdown::SubsystemHandle;

use crate::osc_query::node::OscAccess;
use crate::osc_query::service::OscQueryServiceBuilder;
use crate::plugins::{ChannelManager, Plugin};
#[cfg(target_os = "linux")]
use crate::utils::config::ConfigHandle;
use crate::utils::config::ConfigManager;
#[cfg(target_os = "linux")]
use crate::AppWindow;

#[cfg(not(target_os = "linux"))]
mod enigo;
#[cfg(target_os = "linux")]
mod mpris;

#[cfg(target_os = "linux")]
use mpris::MediaControlConfig;

pub struct MediaControl {
    #[cfg(target_os = "linux")]
    config: Arc<ConfigHandle<MediaControlConfig>>,
}

#[async_trait]
impl Plugin for MediaControl {
    fn new(config_manager: ConfigManager) -> Self {
        #[cfg(target_os = "linux")]
        {
            Self {
                config: Arc::new(config_manager.load_config(None, None)),
            }
        }

        #[cfg(not(target_os = "linux"))]
        {
            let _ = config_manager;
            Self {}
        }
    }

    fn title(&self) -> &'static str {
        "Media Control"
    }

    fn description(&self) -> &'static str {
        "Control your media player through VRChat."
    }

    async fn run(
        &self,
        subsys: &SubsystemHandle,
        channels: Arc<ChannelManager>,
    ) -> anyhow::Result<()> {
        #[cfg(target_os = "linux")]
        {
            mpris::run(subsys, &self.config, channels).await
        }

        #[cfg(not(target_os = "linux"))]
        {
            let _ = subsys;
            enigo::run(channels).await
        }
    }

    fn register_osc_parameters(&self, service: &mut OscQueryServiceBuilder) {
        service.add_endpoint(
            "/avatar/parameters/MC_PrevTrack".to_string(),
            "b".to_string(),
            OscAccess::Read,
            "Media Control: Previous Track".to_string(),
        );
        service.add_endpoint(
            "/avatar/parameters/MC_NextTrack".to_string(),
            "b".to_string(),
            OscAccess::Read,
            "Media Control: Next Track".to_string(),
        );
        service.add_endpoint(
            "/avatar/parameters/MC_PlayPause".to_string(),
            "b".to_string(),
            OscAccess::Read,
            "Media Control: Play/Pause".to_string(),
        );
        service.add_endpoint(
            "/avatar/parameters/MC_Stop".to_string(),
            "b".to_string(),
            OscAccess::Read,
            "Media Control: Stop".to_string(),
        );
    }

    #[cfg(target_os = "linux")]
    fn has_settings(&self) -> bool {
        true
    }

    #[cfg(target_os = "linux")]
    fn register_settings_callbacks(self: Arc<Self>, app_window: &AppWindow) -> anyhow::Result<()> {
        mpris::register_settings_callbacks(self.config.clone(), app_window)
    }

    #[cfg(target_os = "linux")]
    fn open_settings(self: Arc<Self>, app_window: Weak<AppWindow>) -> anyhow::Result<()> {
        mpris::open_settings(self.config.clone(), app_window)
    }
}
