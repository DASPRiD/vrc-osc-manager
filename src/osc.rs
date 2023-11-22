use crate::osc_query::{MakeOscQueryStatic, OscAccess, OscHostInfo, OscQueryService};
use crate::plugins;
use anyhow::{bail, Result};
use async_osc::{OscMessage, OscPacket, OscSocket};
use hyper::Server;
use log::info;
use searchlight::broadcast::{BroadcasterBuilder, ServiceBuilder};
use searchlight::net::IpVersion;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::str::FromStr;
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

        bail!("Sender stream closed unexpectedly");
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
        info!("OSC running on UDP port {}", self.port);

        while let Some(packet) = socket.next().await {
            let (packet, _) = packet?;

            match packet {
                OscPacket::Bundle(_) => {}
                OscPacket::Message(message) => {
                    let _ = self.tx.send(message);
                }
            }
        }

        bail!("Receiver stream closed unexpectedly");
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

pub struct Query {
    tcp_port: u16,
    udp_port: u16,
}

impl Query {
    pub fn new(tcp_port: u16, udp_port: u16) -> Self {
        Self { tcp_port, udp_port }
    }

    async fn listen(self) -> Result<()> {
        let _broadcaster = BroadcasterBuilder::new()
            .loopback()
            .add_service(
                ServiceBuilder::new("_oscjson._tcp.local.", "VRC-OSC-Manager", self.tcp_port)?
                    .add_ip_address(IpAddr::V4(Ipv4Addr::from_str("127.0.0.1")?))
                    .build()?,
            )
            .add_service(
                ServiceBuilder::new("_osc._udp.local.", "VRC-OSC-Manager", self.udp_port)?
                    .add_ip_address(IpAddr::V4(Ipv4Addr::from_str("127.0.0.1")?))
                    .build()?,
            )
            .build(IpVersion::V4)?
            .run_in_background();

        let mut osc_query_service = OscQueryService::new(OscHostInfo::new(
            "VRC OSC Manager".to_string(),
            "127.0.0.1".to_string(),
            self.udp_port,
        ));
        osc_query_service.add_endpoint(
            "/avatar/change".to_string(),
            "s".to_string(),
            OscAccess::Read,
            "".to_string(),
        );

        #[cfg(feature = "media-control")]
        plugins::media_control::register_osc_query_parameters(&mut osc_query_service);

        #[cfg(feature = "pishock")]
        plugins::pishock::register_osc_query_parameters(&mut osc_query_service);

        info!("OSCQuery running on TCP port {}", self.tcp_port);

        let addr = SocketAddr::from(([127, 0, 0, 1], self.tcp_port));
        let server = Server::bind(&addr).serve(MakeOscQueryStatic::new(osc_query_service));
        server.await?;

        Ok(())
    }

    pub async fn run(self, subsys: SubsystemHandle) -> Result<()> {
        match (self.listen().cancel_on_shutdown(&subsys)).await {
            Ok(Ok(())) => subsys.request_shutdown(),
            Ok(Err(error)) => return Err(error),
            Err(CancelledByShutdown) => {}
        }

        Ok(())
    }
}
