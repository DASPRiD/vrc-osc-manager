use std::future::IntoFuture;
use std::net::SocketAddr;

use axum::serve;
use tokio::net::TcpListener;
use tokio_graceful_shutdown::errors::CancelledByShutdown;
use tokio_graceful_shutdown::{FutureExt, IntoSubsystem, SubsystemHandle};
use tower::make::Shared;

use crate::osc_query::service::OscQueryService;

pub struct OscQueryTask {
    port: u16,
    service: OscQueryService,
}

impl OscQueryTask {
    pub fn new(port: u16, service: OscQueryService) -> Self {
        Self { port, service }
    }
}

impl IntoSubsystem<anyhow::Error> for OscQueryTask {
    async fn run(self, subsys: &mut SubsystemHandle) -> anyhow::Result<()> {
        let addr = SocketAddr::from(([127, 0, 0, 1], self.port));
        let listener = TcpListener::bind(addr).await?;
        let service = Shared::new(self.service);

        match serve(listener, service)
            .into_future()
            .cancel_on_shutdown(subsys)
            .await
        {
            Ok(Ok(())) => {}
            Ok(Err(error)) => return Err(error.into()),
            Err(CancelledByShutdown) => {}
        }

        Ok(())
    }
}
