use crate::config::Config;
use anyhow::Result;
use async_osc::{prelude::OscMessageExt, OscMessage, OscType};
use log::info;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, mpsc, Mutex};
use tokio::time::sleep;
use tokio::{select, spawn};
use tokio_graceful_shutdown::{errors::CancelledByShutdown, FutureExt, SubsystemHandle};
use tokio_util::sync::CancellationToken;

#[derive(Debug)]
enum ModifierButton {
    Minus,
    Plus,
}

fn start_delta_sending(tx: mpsc::Sender<f32>, base: f32) -> CancellationToken {
    let token = CancellationToken::new();
    let delta_cancel = token.clone();

    spawn(async move {
        let delta = 0.01 * base;
        let _ = tx.send(delta).await;

        sleep(Duration::from_millis(900)).await;

        loop {
            select! {
                _ = token.cancelled() => return,
                _ = sleep(Duration::from_millis(100)) => {
                    let _ = tx.send(delta).await;
                },
            }
        }
    });

    delta_cancel
}

async fn handle_modifier(
    mut modifier_rx: mpsc::Receiver<(ModifierButton, bool)>,
    delta_tx: mpsc::Sender<f32>,
) -> Result<()> {
    let mut minus_pressed = false;
    let mut plus_pressed = false;
    let mut delta_cancel: Option<CancellationToken> = None;

    while let Some((button, pressed)) = modifier_rx.recv().await {
        match button {
            ModifierButton::Minus => minus_pressed = pressed,
            ModifierButton::Plus => plus_pressed = pressed,
        }

        if minus_pressed && !plus_pressed {
            if delta_cancel.is_none() {
                delta_cancel = Some(start_delta_sending(delta_tx.clone(), -1.));
            }
        } else if plus_pressed && !minus_pressed {
            if delta_cancel.is_none() {
                delta_cancel = Some(start_delta_sending(delta_tx.clone(), 1.));
            }
        } else if let Some(token) = delta_cancel {
            token.cancel();
            delta_cancel = None;
        }
    }

    Ok(())
}

async fn handle_delta(
    mut delta_rx: mpsc::Receiver<f32>,
    strength: Arc<Mutex<f32>>,
    osc_tx: mpsc::Sender<OscMessage>,
) -> Result<()> {
    while let Some(delta) = delta_rx.recv().await {
        let mut strength = strength.lock().await;
        *strength = (*strength + delta).clamp(0., 1.);

        let _ = osc_tx
            .send(OscMessage {
                addr: "/avatar/parameters/PS_Strength".to_string(),
                args: vec![OscType::Float(*strength)],
            })
            .await;
    }

    Ok(())
}

#[derive(Debug)]
enum ShockButton {
    Left,
    Right,
}

#[derive(Serialize, Deserialize, Debug)]
struct ShockBody {
    #[serde(rename = "Username")]
    username: String,

    #[serde(rename = "ApiKey")]
    api_key: String,

    #[serde(rename = "Code")]
    code: String,

    #[serde(rename = "Name")]
    name: String,

    #[serde(rename = "Op")]
    op: u8,

    #[serde(rename = "Duration")]
    duration: u8,

    #[serde(rename = "Intensity")]
    intensity: u8,
}

async fn handle_shock(
    mut shock_rx: mpsc::Receiver<(ShockButton, bool)>,
    strength: Arc<Mutex<f32>>,
    config: Arc<Config>,
) -> Result<()> {
    let mut left_pressed = false;
    let mut right_pressed = false;
    let mut shock_cancel: Option<CancellationToken> = None;

    while let Some((button, pressed)) = shock_rx.recv().await {
        match button {
            ShockButton::Left => left_pressed = pressed,
            ShockButton::Right => right_pressed = pressed,
        }

        if left_pressed && right_pressed {
            if shock_cancel.is_none() {
                let token = CancellationToken::new();
                shock_cancel = Some(token.clone());
                let strength = strength.clone();
                let config = config.clone();

                spawn(async move {
                    loop {
                        let strength = *strength.lock().await;
                        info!("Sending shock with strength {}", strength);

                        let body = ShockBody {
                            username: config.pishock.username.clone(),
                            api_key: config.pishock.api_key.clone(),
                            code: config.pishock.code.clone(),
                            name: "OSC Manager - PiShock Plugin".to_string(),
                            op: 0,
                            duration: config.pishock.duration,
                            intensity: 1 + (99. * strength) as u8,
                        };

                        let client = reqwest::Client::new();
                        let _ = client
                            .post("https://do.pishock.com/api/apioperate")
                            .json(&body)
                            .send()
                            .await;

                        select! {
                            _ = token.cancelled() => return,
                            _ = sleep(Duration::from_secs(3)) => continue,
                        }
                    }
                });
            }
        } else if let Some(token) = shock_cancel {
            token.cancel();
            shock_cancel = None;
        }
    }

    Ok(())
}

pub struct PiShock {
    tx: mpsc::Sender<OscMessage>,
    rx: broadcast::Receiver<OscMessage>,
    config: Arc<Config>,
}

impl PiShock {
    pub fn new(
        tx: mpsc::Sender<OscMessage>,
        rx: broadcast::Receiver<OscMessage>,
        config: Arc<Config>,
    ) -> Self {
        Self { tx, rx, config }
    }

    async fn handle_buttons(&mut self) -> Result<()> {
        let (shock_tx, shock_rx) = mpsc::channel(8);
        let (modifier_tx, modifier_rx) = mpsc::channel(8);
        let (delta_tx, delta_rx) = mpsc::channel(8);
        let strength = Arc::new(Mutex::new(0_f32));
        let delta_strength = strength.clone();
        let shock_strength = strength.clone();
        let osc_tx = self.tx.clone();
        let config = self.config.clone();

        spawn(async move {
            let _ = handle_modifier(modifier_rx, delta_tx).await;
        });

        spawn(async move {
            let _ = handle_delta(delta_rx, delta_strength, osc_tx).await;
        });

        spawn(async move {
            let _ = handle_shock(shock_rx, shock_strength, config).await;
        });

        while let Ok(message) = self.rx.recv().await {
            match message.as_tuple() {
                ("/avatar/parameters/PS_Minus_Pressed", &[OscType::Bool(value)]) => {
                    modifier_tx.send((ModifierButton::Minus, value)).await?;
                }
                ("/avatar/parameters/PS_Plus_Pressed", &[OscType::Bool(value)]) => {
                    modifier_tx.send((ModifierButton::Plus, value)).await?;
                }
                ("/avatar/parameters/PS_ShockLeft_Pressed", &[OscType::Bool(value)]) => {
                    shock_tx.send((ShockButton::Left, value)).await?;
                }
                ("/avatar/parameters/PS_ShockRight_Pressed", &[OscType::Bool(value)]) => {
                    shock_tx.send((ShockButton::Right, value)).await?;
                }
                ("/avatar/parameters/PS_Strength", &[OscType::Float(value)]) => {
                    *strength.lock().await = value;
                }
                _ => {}
            }
        }

        Ok(())
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
