use async_osc::{OscMessage, OscSocket};
use async_trait::async_trait;
use log::debug;
use tokio::sync::mpsc;
use tokio_graceful_shutdown::errors::CancelledByShutdown;
use tokio_graceful_shutdown::{FutureExt, IntoSubsystem, SubsystemHandle};

pub struct OscSenderTask {
    port: u16,
    rx: mpsc::Receiver<OscMessage>,
}

impl OscSenderTask {
    pub fn new(port: u16, rx: mpsc::Receiver<OscMessage>) -> Self {
        Self { port, rx }
    }

    async fn main_loop(&mut self) -> anyhow::Result<()> {
        let socket = OscSocket::bind("127.0.0.1:0").await?;
        socket.connect(("127.0.0.1", self.port)).await?;

        while let Some(message) = self.rx.recv().await {
            if let Err(error) = socket.send(message).await {
                debug!("Failed to send OSC message: {}", error);
            }
        }

        Ok(())
    }
}

#[async_trait]
impl IntoSubsystem<anyhow::Error> for OscSenderTask {
    async fn run(mut self, subsys: SubsystemHandle) -> anyhow::Result<()> {
        match self.main_loop().cancel_on_shutdown(&subsys).await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => return Err(error),
            Err(CancelledByShutdown) => {}
        }

        Ok(())
    }
}
