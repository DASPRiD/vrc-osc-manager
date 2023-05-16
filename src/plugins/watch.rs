use anyhow::Result;
use async_osc::{OscMessage, OscType};
use chrono::{Local, Timelike};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::sleep;
use tokio_graceful_shutdown::{errors::CancelledByShutdown, FutureExt, SubsystemHandle};

pub struct Watch {
    tx: mpsc::Sender<OscMessage>,
}

impl Watch {
    pub fn new(tx: mpsc::Sender<OscMessage>) -> Self {
        Self { tx }
    }

    async fn send_time(&mut self) {
        loop {
            let now = Local::now();
            let hour = ((now.hour() % 12) as f32 + now.minute() as f32 / 60.) / 6. - 1.;
            let minute = (now.minute() as f32 + now.second() as f32 / 60.) / 30. - 1.;

            let _ = self
                .tx
                .send(OscMessage {
                    addr: "/avatar/parameters/RMBA_WatchHours".to_string(),
                    args: vec![OscType::Float(hour)],
                })
                .await;
            let _ = self
                .tx
                .send(OscMessage {
                    addr: "/avatar/parameters/RMBA_WatchMinutes".to_string(),
                    args: vec![OscType::Float(minute)],
                })
                .await;

            sleep(Duration::from_secs(10)).await;
        }
    }

    pub async fn run(mut self, subsys: SubsystemHandle) -> Result<()> {
        match (self.send_time().cancel_on_shutdown(&subsys)).await {
            Ok(()) => subsys.request_shutdown(),
            Err(CancelledByShutdown) => {}
        }

        Ok(())
    }
}
