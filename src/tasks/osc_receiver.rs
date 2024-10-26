use async_osc::{OscMessage, OscPacket, OscSocket};
use async_trait::async_trait;
use tokio::sync::broadcast;
use tokio_graceful_shutdown::errors::CancelledByShutdown;
use tokio_graceful_shutdown::{FutureExt, IntoSubsystem, SubsystemHandle};
use tokio_stream::StreamExt;

pub struct OscReceiverTask {
    port: u16,
    tx: broadcast::Sender<OscMessage>,
}

impl OscReceiverTask {
    pub fn new(port: u16, tx: broadcast::Sender<OscMessage>) -> Self {
        Self { port, tx }
    }

    async fn main_loop(&mut self) -> anyhow::Result<()> {
        let mut socket = OscSocket::bind(("127.0.0.1", self.port)).await?;

        while let Some(packet) = socket.next().await {
            let (packet, _) = packet?;

            match packet {
                OscPacket::Bundle(_) => {}
                OscPacket::Message(message) => {
                    let _ = self.tx.send(message);
                }
            }
        }

        Ok(())
    }
}

#[async_trait]
impl IntoSubsystem<anyhow::Error> for OscReceiverTask {
    async fn run(mut self, subsys: SubsystemHandle) -> anyhow::Result<()> {
        match self.main_loop().cancel_on_shutdown(&subsys).await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => return Err(error),
            Err(CancelledByShutdown) => {}
        }

        Ok(())
    }
}
