use std::sync::Arc;
use std::time::Duration;

use async_osc::{OscMessage, OscType};
use async_trait::async_trait;
use chrono::{Local, Timelike};
use tokio::time::sleep;
use tokio_graceful_shutdown::SubsystemHandle;

use crate::osc_query::node::OscAccess;
use crate::osc_query::service::OscQueryServiceBuilder;
use crate::plugins::{ChannelManager, Plugin};
use crate::utils::config::ConfigManager;

pub struct Watch;

#[async_trait]
impl Plugin for Watch {
    fn new(_config_manager: ConfigManager) -> Self {
        Self
    }

    fn title(&self) -> &'static str {
        "OSC Watch"
    }

    fn description(&self) -> &'static str {
        "Drive OSC Watch by Reimajo without an additional program."
    }

    fn info_url(&self) -> Option<&'static str> {
        Some("https://booth.pm/en/items/3687002")
    }

    async fn run(
        &self,
        _subsys: &SubsystemHandle,
        channels: Arc<ChannelManager>,
    ) -> anyhow::Result<()> {
        let sender = channels.create_osc_sender();

        loop {
            let now = Local::now();
            let hour = ((now.hour() % 12) as f32 + now.minute() as f32 / 60.) / 6. - 1.;
            let minute = (now.minute() as f32 + now.second() as f32 / 60.) / 30. - 1.;

            let _ = sender
                .send(OscMessage {
                    addr: "/avatar/parameters/RMBA_WatchHours".to_string(),
                    args: vec![OscType::Float(hour)],
                })
                .await;
            let _ = sender
                .send(OscMessage {
                    addr: "/avatar/parameters/RMBA_WatchMinutes".to_string(),
                    args: vec![OscType::Float(minute)],
                })
                .await;

            sleep(Duration::from_secs(10)).await;
        }
    }

    fn register_osc_parameters(&self, service: &mut OscQueryServiceBuilder) {
        service.add_endpoint(
            "/avatar/parameters/RMBA_WatchHours".to_string(),
            "d".to_string(),
            OscAccess::Write,
            "RMBA encoded hours".to_string(),
        );
        service.add_endpoint(
            "/avatar/parameters/RMBA_WatchMinutes".to_string(),
            "d".to_string(),
            OscAccess::Write,
            "RMBA encoded minutes".to_string(),
        );
    }
}
