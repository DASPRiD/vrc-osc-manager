use async_trait::async_trait;
use log::error;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio::fs::{create_dir_all, write};
use tokio::select;
use tokio::sync::mpsc;
use tokio::time::interval;
use tokio_graceful_shutdown::errors::CancelledByShutdown;
use tokio_graceful_shutdown::{FutureExt, IntoSubsystem, SubsystemHandle};

pub struct WriteConfigRequest {
    pub path: PathBuf,
    pub config: String,
    pub debounce: Option<Duration>,
}

pub struct ConfigWriterTask {
    rx: mpsc::Receiver<WriteConfigRequest>,
    debounced: HashMap<PathBuf, (Instant, WriteConfigRequest)>,
}

impl ConfigWriterTask {
    pub fn new(rx: mpsc::Receiver<WriteConfigRequest>) -> Self {
        Self {
            rx,
            debounced: HashMap::new(),
        }
    }

    async fn main_loop(&mut self) -> anyhow::Result<()> {
        let mut interval = interval(Duration::from_secs(5));

        loop {
            select! {
                request = self.rx.recv() => match request {
                    Some(request) => {
                        match request.debounce {
                            Some(debounce) => {
                                self.debounced.insert(
                                    request.path.clone(),
                                    (Instant::now() + debounce, request)
                                );
                            }
                            None => {
                                self.write_config(&request).await;
                                continue;
                            }
                        }
                    }
                    None => break,
                },
                _ = interval.tick() => {
                    let now = Instant::now();
                    let mut expired_paths = vec![];

                    for (path, (expire_time, request)) in &self.debounced {
                        if now >= *expire_time {
                            self.write_config(request).await;
                            expired_paths.push(path.clone());
                        }
                    }

                    for path in expired_paths {
                        self.debounced.remove(path.as_path());
                    }
                }
            }
        }

        self.flush_pending_requests().await;
        Ok(())
    }

    async fn flush_pending_requests(&mut self) {
        let requests: Vec<_> = self
            .debounced
            .drain()
            .map(|(_, (_, request))| request)
            .collect();

        for request in requests {
            self.write_config(&request).await;
        }
    }

    async fn write_config(&self, request: &WriteConfigRequest) {
        if let Some(parent_dir) = request.path.parent() {
            if let Err(error) = create_dir_all(parent_dir).await {
                error!("Failed to create config directory: {}", error);
            }
        }

        if let Err(error) = write(&request.path, &request.config).await {
            error!("Failed to write config: {}", error);
        }
    }
}

#[async_trait]
impl IntoSubsystem<anyhow::Error> for ConfigWriterTask {
    async fn run(mut self, subsys: SubsystemHandle) -> anyhow::Result<()> {
        match self.main_loop().cancel_on_shutdown(&subsys).await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => return Err(error),
            Err(CancelledByShutdown) => {
                self.flush_pending_requests().await;
            }
        }

        Ok(())
    }
}
