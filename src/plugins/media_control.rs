use async_osc::{prelude::OscMessageExt, OscType};
use async_trait::async_trait;
use enigo::Direction::Click;
use enigo::{Enigo, Key, Keyboard, Settings};
use log::warn;
use std::sync::Arc;
use tokio::sync::broadcast::error::RecvError;
use tokio_graceful_shutdown::SubsystemHandle;

use crate::osc_query::node::OscAccess;
use crate::osc_query::service::OscQueryServiceBuilder;
use crate::plugins::{ChannelManager, Plugin};
use crate::utils::config::ConfigManager;

pub struct MediaControl;

#[async_trait]
impl Plugin for MediaControl {
    fn new(_config_manager: ConfigManager) -> Self {
        Self
    }

    fn title(&self) -> &'static str {
        "Media Control"
    }

    fn description(&self) -> &'static str {
        "Control your media player through VRChat."
    }

    async fn run(
        &self,
        _subsys: &SubsystemHandle,
        channels: Arc<ChannelManager>,
    ) -> anyhow::Result<()> {
        let mut enigo = Enigo::new(&Settings::default())?;
        let mut rx = channels.subscribe_to_osc();

        loop {
            match rx.recv().await {
                Ok(message) => match message.as_tuple() {
                    ("/avatar/parameters/MC_PrevTrack", &[OscType::Bool(value)]) => {
                        if value {
                            enigo.key(Key::MediaPrevTrack, Click)?;
                        }
                    }
                    ("/avatar/parameters/MC_NextTrack", &[OscType::Bool(value)]) => {
                        if value {
                            enigo.key(Key::MediaNextTrack, Click)?;
                        }
                    }
                    ("/avatar/parameters/MC_PlayPause", &[OscType::Bool(value)]) => {
                        if value {
                            enigo.key(Key::MediaPlayPause, Click)?;
                        }
                    }
                    ("/avatar/parameters/MC_Stop", &[OscType::Bool(value)]) => {
                        if value {
                            enigo.key(Key::MediaStop, Click)?;
                        }
                    }
                    _ => {}
                },
                Err(RecvError::Closed) => break,
                Err(RecvError::Lagged(skipped)) => {
                    warn!(
                        "MediaControl lagging behind, {} messages have been dropped",
                        skipped
                    );
                }
            }
        }

        Ok(())
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
}
