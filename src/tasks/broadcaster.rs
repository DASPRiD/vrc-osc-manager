use log::{info, warn};
use searchlight::broadcast::{Broadcaster, BroadcasterBuilder, BroadcasterHandle, ServiceBuilder};
use searchlight::net::IpVersion;
use std::net::{IpAddr, Ipv4Addr};
use std::str::FromStr;
use std::time::{Duration, Instant};
use tokio::time::sleep;
use tokio_graceful_shutdown::errors::CancelledByShutdown;
use tokio_graceful_shutdown::{FutureExt, IntoSubsystem, SubsystemHandle};

const INITIAL_RETRY_DELAY: Duration = Duration::from_secs(1);
const MAX_RETRY_DELAY: Duration = Duration::from_secs(30);
const NOTIFY_AFTER: Duration = Duration::from_secs(60);

pub struct BroadcasterTask {
    osc_listener_port: u16,
    osc_query_port: u16,
}

impl BroadcasterTask {
    pub fn new(osc_listener_port: u16, osc_query_port: u16) -> Self {
        Self {
            osc_listener_port,
            osc_query_port,
        }
    }

    fn build(&self) -> anyhow::Result<Broadcaster> {
        let ip_addr = IpAddr::V4(Ipv4Addr::from_str("127.0.0.1")?);

        Ok(BroadcasterBuilder::new()
            .loopback()
            .add_service(
                ServiceBuilder::new(
                    "_oscjson._tcp.local.",
                    "VRC-OSC-Manager",
                    self.osc_query_port,
                )?
                .add_ip_address(ip_addr)
                .build()?,
            )
            .add_service(
                ServiceBuilder::new(
                    "_osc._udp.local.",
                    "VRC-OSC-Manager",
                    self.osc_listener_port,
                )?
                .add_ip_address(ip_addr)
                .build()?,
            )
            .build(IpVersion::V4)?)
    }

    async fn start_with_retry(&self) -> BroadcasterHandle {
        let started_at = Instant::now();
        let mut delay = INITIAL_RETRY_DELAY;
        let mut attempt: u32 = 0;
        let mut notified = false;

        loop {
            attempt += 1;

            match self.build() {
                Ok(broadcaster) => {
                    if attempt > 1 {
                        info!("mDNS broadcaster started after {attempt} attempts");
                    }

                    return broadcaster.run_in_background();
                }
                Err(error) => {
                    if attempt == 1 {
                        warn!(
                            "mDNS broadcaster startup failed (network may not be ready yet), retrying with backoff: {error}"
                        );
                    }

                    if !notified && started_at.elapsed() >= NOTIFY_AFTER {
                        notify_failure(&error.to_string());
                        notified = true;
                    }

                    sleep(delay).await;
                    delay = (delay * 2).min(MAX_RETRY_DELAY);
                }
            }
        }
    }
}

fn notify_failure(detail: &str) {
    let result = notify_rust::Notification::new()
        .appname("VRC OSC Manager")
        .summary("VRC OSC Manager: service discovery unavailable")
        .body(&format!(
            "Could not start the mDNS broadcaster. VRChat will not auto-discover the manager until the network comes up.\n\n{detail}"
        ))
        .show();

    if let Err(error) = result {
        warn!("Failed to show broadcaster failure notification: {error}");
    }
}

impl IntoSubsystem<anyhow::Error> for BroadcasterTask {
    async fn run(self, subsys: &mut SubsystemHandle) -> anyhow::Result<()> {
        let handle = match self.start_with_retry().cancel_on_shutdown(subsys).await {
            Ok(handle) => handle,
            Err(CancelledByShutdown) => return Ok(()),
        };

        subsys.on_shutdown_requested().await;
        let _ = handle.shutdown();

        Ok(())
    }
}
