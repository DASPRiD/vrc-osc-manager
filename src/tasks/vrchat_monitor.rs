use crate::tasks::orchestrate::AppEvent;
use async_trait::async_trait;
use log::debug;
use std::time::Duration;
use sysinfo::{ProcessRefreshKind, RefreshKind, System};
use tokio::sync::mpsc;
use tokio::time::sleep;
use tokio_graceful_shutdown::errors::CancelledByShutdown;
use tokio_graceful_shutdown::{FutureExt, IntoSubsystem, SubsystemHandle};

pub struct VrchatMonitorTask {
    app_event_tx: mpsc::Sender<AppEvent>,
}

impl VrchatMonitorTask {
    pub fn new(app_event_tx: mpsc::Sender<AppEvent>) -> Self {
        Self { app_event_tx }
    }

    async fn main_loop(&self) -> anyhow::Result<()> {
        let mut is_running = false;
        let mut sys = System::new();
        let refresh_kind = RefreshKind::new().with_processes(ProcessRefreshKind::new());

        loop {
            debug!("Checking if VRChat is running");
            sys.refresh_specifics(refresh_kind);

            let process_running = sys.processes_by_name("VRChat").next().is_some();

            if process_running != is_running {
                is_running = process_running;

                match is_running {
                    true => {
                        self.app_event_tx.send(AppEvent::VrchatStarted).await?;
                        debug!("VRChat process started");
                    }
                    false => {
                        self.app_event_tx.send(AppEvent::VrchatStopped).await?;
                        debug!("VRChat process stopped");
                    }
                }
            }

            sleep(Duration::from_secs(20)).await;
        }
    }
}

#[async_trait]
impl IntoSubsystem<anyhow::Error> for VrchatMonitorTask {
    async fn run(self, subsys: SubsystemHandle) -> anyhow::Result<()> {
        match self.main_loop().cancel_on_shutdown(&subsys).await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => return Err(error),
            Err(CancelledByShutdown) => {}
        }

        Ok(())
    }
}
