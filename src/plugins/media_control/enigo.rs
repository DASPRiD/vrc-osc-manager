use std::sync::Arc;

use async_osc::prelude::OscMessageExt;
use async_osc::OscType;
use enigo::Direction::Click;
use enigo::{Enigo, Key, Keyboard, Settings};
use log::warn;
use tokio::sync::broadcast::error::RecvError;

use crate::plugins::ChannelManager;

pub(super) async fn run(channels: Arc<ChannelManager>) -> anyhow::Result<()> {
    let mut enigo = Enigo::new(&Settings::default())?;
    let mut rx = channels.subscribe_to_osc();

    loop {
        match rx.recv().await {
            Ok(message) => match message.as_tuple() {
                ("/avatar/parameters/MC_PrevTrack", &[OscType::Bool(value)]) if value => {
                    enigo.key(Key::MediaPrevTrack, Click)?;
                }
                ("/avatar/parameters/MC_NextTrack", &[OscType::Bool(value)]) if value => {
                    enigo.key(Key::MediaNextTrack, Click)?;
                }
                ("/avatar/parameters/MC_PlayPause", &[OscType::Bool(value)]) if value => {
                    enigo.key(Key::MediaPlayPause, Click)?;
                }
                ("/avatar/parameters/MC_Stop", &[OscType::Bool(value)]) if value => {
                    enigo.key(Key::MediaStop, Click)?;
                }
                _ => {}
            },
            Err(RecvError::Closed) => break,
            Err(RecvError::Lagged(skipped)) => {
                warn!(
                    "MediaControl lagging behind, {} messages have been dropped",
                    skipped
                );
            }
        }
    }

    Ok(())
}
