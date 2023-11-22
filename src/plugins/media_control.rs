use anyhow::{bail, Result};
use async_osc::{prelude::OscMessageExt, OscMessage, OscType};
use enigo::Direction::Click;
use enigo::{Enigo, Key, Keyboard, Settings};
use log::{debug, warn};
use tokio::sync::broadcast;
use tokio::sync::broadcast::error::RecvError;
use tokio_graceful_shutdown::{errors::CancelledByShutdown, FutureExt, SubsystemHandle};

use crate::osc_query::{OscAccess, OscQueryService};

pub struct MediaControl {
    rx: broadcast::Receiver<OscMessage>,
}

impl MediaControl {
    pub fn new(rx: broadcast::Receiver<OscMessage>) -> Self {
        Self { rx }
    }

    async fn handle_buttons(&mut self) -> Result<()> {
        let mut enigo = Enigo::new(&Settings::default())?;

        loop {
            match self.rx.recv().await {
                Ok(message) => match message.as_tuple() {
                    ("/avatar/parameters/MC_PrevTrack", &[OscType::Bool(value)]) => {
                        if value {
                            enigo.key(Key::MediaPrevTrack, Click)?;
                        }
                    }
                    ("/avatar/parameters/MC_NextTrack", &[OscType::Bool(value)]) => {
                        if value {
                            enigo.key(Key::MediaNextTrack, Click)?;
                        }
                    }
                    ("/avatar/parameters/MC_PlayPause", &[OscType::Bool(value)]) => {
                        if value {
                            enigo.key(Key::MediaPlayPause, Click)?;
                        }
                    }
                    ("/avatar/parameters/MC_Stop", &[OscType::Bool(value)]) => {
                        if value {
                            enigo.key(Key::MediaStop, Click)?;
                        }
                    }
                    _ => {}
                },
                Err(error) => match error {
                    RecvError::Closed => {
                        debug!("Channel closed");
                        break;
                    }
                    RecvError::Lagged(skipped) => {
                        warn!(
                            "MediaControl lagging behind, {} messages have been dropped",
                            skipped
                        );
                    }
                },
            }
        }

        bail!("Message receiver died unexpectedly");
    }

    pub async fn run(mut self, subsys: SubsystemHandle) -> Result<()> {
        match (self.handle_buttons().cancel_on_shutdown(&subsys)).await {
            Ok(Ok(())) => subsys.request_shutdown(),
            Ok(Err(error)) => return Err(error),
            Err(CancelledByShutdown) => {}
        }

        Ok(())
    }
}

pub fn register_osc_query_parameters(service: &mut OscQueryService) {
    service.add_endpoint(
        "/avatar/parameters/MC_PrevTrack".to_string(),
        "b".to_string(),
        OscAccess::Read,
        "Media Control: Previous Track".to_string(),
    );
    service.add_endpoint(
        "/avatar/parameters/MC_NextTrack".to_string(),
        "b".to_string(),
        OscAccess::Read,
        "Media Control: Next Track".to_string(),
    );
    service.add_endpoint(
        "/avatar/parameters/MC_PlayPause".to_string(),
        "b".to_string(),
        OscAccess::Read,
        "Media Control: Play/Pause".to_string(),
    );
    service.add_endpoint(
        "/avatar/parameters/MC_Stop".to_string(),
        "b".to_string(),
        OscAccess::Read,
        "Media Control: Stop".to_string(),
    );
}
