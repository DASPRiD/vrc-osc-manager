use anyhow::Result;
use async_osc::{OscMessage, OscPacket, OscSocket};
use tokio::sync::{broadcast, mpsc};
use tokio_graceful_shutdown::{errors::CancelledByShutdown, FutureExt, SubsystemHandle};
use tokio_stream::StreamExt;

pub struct Sender {
    rx: mpsc::Receiver<OscMessage>,
    port: u16,
}

impl Sender {
    pub fn new(rx: mpsc::Receiver<OscMessage>, port: u16) -> Self {
        Self { rx, port }
    }

    async fn send(&mut self) -> Result<()> {
        let socket = OscSocket::bind("127.0.0.1:0").await?;
        socket.connect(("127.0.0.1", self.port)).await?;

        while let Some(message) = self.rx.recv().await {
            // We ignore failure of sending, as a proper shutdown will be handled by the launcher.
            let _ = socket.send(message).await;
        }

        Ok(())
    }

    pub async fn run(mut self, subsys: SubsystemHandle) -> Result<()> {
        match (self.send().cancel_on_shutdown(&subsys)).await {
            Ok(Ok(())) => subsys.request_shutdown(),
            Ok(Err(error)) => return Err(error),
            Err(CancelledByShutdown) => {}
        }

        Ok(())
    }
}

pub struct Receiver {
    tx: broadcast::Sender<OscMessage>,
    port: u16,
}

impl Receiver {
    pub fn new(tx: broadcast::Sender<OscMessage>, port: u16) -> Self {
        Self { tx, port }
    }

    async fn receive(&mut self) -> Result<()> {
        let mut socket = OscSocket::bind(("127.0.0.1", self.port)).await?;

        while let Some(packet) = socket.next().await {
            let (packet, _) = packet?;

            match packet {
                OscPacket::Bundle(_) => {}
                OscPacket::Message(message) => {
                    self.tx.send(message)?;
                }
            }
        }

        Ok(())
    }

    pub async fn run(mut self, subsys: SubsystemHandle) -> Result<()> {
        match (self.receive().cancel_on_shutdown(&subsys)).await {
            Ok(Ok(())) => subsys.request_shutdown(),
            Ok(Err(error)) => return Err(error),
            Err(CancelledByShutdown) => {}
        }

        Ok(())
    }
}
