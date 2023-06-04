use crate::config::Config;
use anyhow::{bail, Context, Result};
use async_osc::{prelude::OscMessageExt, OscMessage, OscType};
use debounced::debounced;
use log::{debug, info, warn};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::fs::{metadata, File};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio::time::sleep;
use tokio::{select, spawn};
use tokio_graceful_shutdown::{errors::CancelledByShutdown, FutureExt, SubsystemHandle};
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct Settings {
    intensity: f32,
    intensity_cap: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            intensity: 0.,
            intensity_cap: 1.,
        }
    }
}

#[derive(Debug)]
enum SettingsAction {
    GetSettings {
        responder: oneshot::Sender<Settings>,
    },
    SetIntensity {
        intensity: f32,
        responder: oneshot::Sender<Option<f32>>,
    },
    SetIntensityCap {
        cap: f32,
        responder: oneshot::Sender<Option<f32>>,
    },
}

struct SettingsStorage {
    path: PathBuf,
    settings: Settings,
}

impl SettingsStorage {
    async fn new(data_dir: PathBuf) -> Result<Self> {
        let path = data_dir.join("pishock.toml");

        if metadata(&path).await.is_err() {
            let settings: Settings = Default::default();
            return Ok(Self { path, settings });
        }

        let mut file = File::open(&path)
            .await
            .with_context(|| format!("Failed to open {}", path.display()))?;
        let mut toml_settings = String::new();
        file.read_to_string(&mut toml_settings).await?;
        let settings: Settings = toml::from_str(&toml_settings)?;

        Ok(Self { path, settings })
    }

    async fn run(&mut self, mut rx: mpsc::Receiver<SettingsAction>) -> Result<()> {
        let (store_tx, store_rx) = mpsc::channel(8);
        let mut debounced_store = debounced(ReceiverStream::new(store_rx), Duration::from_secs(10));

        loop {
            select! {
                action = rx.recv() => {
                    match action {
                        Some(action) => {
                            use SettingsAction::*;

                            match action {
                                GetSettings { responder } => {
                                    responder.send(self.settings.clone()).unwrap();
                                }
                                SetIntensity {
                                    intensity,
                                    responder,
                                } => {
                                    self.settings.intensity = intensity.clamp(0., 1.);
                                    let mut new_cap = None;

                                    if self.settings.intensity_cap < self.settings.intensity {
                                        self.settings.intensity_cap = self.settings.intensity;
                                        new_cap = Some(self.settings.intensity_cap);
                                    }

                                    store_tx.send(()).await?;
                                    responder.send(new_cap).unwrap();
                                }
                                SetIntensityCap { cap, responder } => {
                                    self.settings.intensity_cap = cap.clamp(0., 1.);
                                    let mut new_intensity = None;

                                    if self.settings.intensity > self.settings.intensity_cap {
                                        self.settings.intensity = self.settings.intensity_cap;
                                        new_intensity = Some(self.settings.intensity);
                                    }

                                    store_tx.send(()).await?;
                                    responder.send(new_intensity).unwrap();
                                }
                            }
                        }
                        None => {
                            return Ok(());
                        }
                    }
                }
                _ = debounced_store.next() => {
                    let mut file = File::create(&self.path)
                        .await
                        .with_context(|| format!("Failed to open {}", self.path.display()))?;
                    file.write_all(toml::to_string(&self.settings)?.as_bytes())
                        .await?;
                }
            }
        }
    }
}

async fn get_settings(settings_tx: &mpsc::Sender<SettingsAction>) -> Result<Settings> {
    let (responder_tx, responder_rx) = oneshot::channel();
    settings_tx
        .send(SettingsAction::GetSettings {
            responder: responder_tx,
        })
        .await
        .unwrap();
    Ok(responder_rx.await?)
}

async fn set_intensity(
    settings_tx: &mpsc::Sender<SettingsAction>,
    intensity: f32,
) -> Result<Option<f32>> {
    let (responder_tx, responder_rx) = oneshot::channel();
    settings_tx
        .send(SettingsAction::SetIntensity {
            intensity,
            responder: responder_tx,
        })
        .await
        .unwrap();
    Ok(responder_rx.await?)
}

async fn set_intensity_cap(
    settings_tx: &mpsc::Sender<SettingsAction>,
    cap: f32,
) -> Result<Option<f32>> {
    let (responder_tx, responder_rx) = oneshot::channel();
    settings_tx
        .send(SettingsAction::SetIntensityCap {
            cap,
            responder: responder_tx,
        })
        .await
        .unwrap();
    Ok(responder_rx.await?)
}

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
    settings_tx: mpsc::Sender<SettingsAction>,
    osc_tx: mpsc::Sender<OscMessage>,
) -> Result<()> {
    while let Some(delta) = delta_rx.recv().await {
        let settings = get_settings(&settings_tx).await?;
        let intensity = (settings.intensity + delta).clamp(0., settings.intensity_cap);
        set_intensity(&settings_tx, intensity).await?;

        let _ = osc_tx
            .send(OscMessage {
                addr: "/avatar/parameters/PS_Intensity".to_string(),
                args: vec![OscType::Float(intensity)],
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

async fn send_shock(
    config: &Arc<Config>,
    intensity: f32,
    duration: u8,
    activity_tx: &mpsc::Sender<u8>,
) {
    let intensity = 1 + (99. * intensity) as u8;
    let duration = duration.clamp(1, 15);

    info!(
        "Sending shock with intensity {} and duration {}",
        intensity, duration
    );

    let body = ShockBody {
        username: config.pishock.username.clone(),
        api_key: config.pishock.api_key.clone(),
        code: config.pishock.code.clone(),
        name: "OSC Manager - PiShock Plugin".to_string(),
        op: 0,
        duration: config.pishock.duration,
        intensity,
    };

    let client = reqwest::Client::new();
    let response = client
        .post("https://do.pishock.com/api/apioperate")
        .json(&body)
        .send()
        .await;

    match response {
        Ok(response) => {
            let status = response.text().await;

            match status {
                Ok(status) => match status.as_str() {
                    "Not Authorized." => warn!("Invalid credentials"),
                    "Operation Succeeded." => {
                        debug!("Shock succeeded");
                        let _ = activity_tx.send(duration).await;
                    }
                    _ => warn!("Unknown response: {}", status),
                },
                Err(_) => {
                    warn!("Failed to parse response");
                }
            }
        }
        Err(_) => {
            warn!("Failed to contact pishock API");
        }
    }
}

async fn handle_shock(
    mut shock_rx: mpsc::Receiver<(ShockButton, bool)>,
    settings_tx: mpsc::Sender<SettingsAction>,
    config: Arc<Config>,
    activity_tx: mpsc::Sender<u8>,
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
                let config = config.clone();
                let activity_tx = activity_tx.clone();
                let settings_tx = settings_tx.clone();

                spawn(async move {
                    loop {
                        let settings = get_settings(&settings_tx).await.unwrap();

                        send_shock(
                            &config,
                            settings.intensity,
                            config.pishock.duration,
                            &activity_tx,
                        )
                        .await;

                        select! {
                            _ = token.cancelled() => return,
                            _ = sleep(Duration::from_secs(config.pishock.duration as u64)) => continue,
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

async fn handle_activity(mut activity_rx: mpsc::Receiver<u8>, osc_tx: mpsc::Sender<OscMessage>) {
    while let Some(duration) = activity_rx.recv().await {
        let _ = osc_tx
            .send(OscMessage {
                addr: "/avatar/parameters/PS_ShockActive".to_string(),
                args: vec![OscType::Bool(true)],
            })
            .await;

        let mut next_disabler = Some(sleep(Duration::from_secs(duration as u64)));

        while let Some(disabler) = next_disabler {
            next_disabler = None;
            let deadline = disabler.deadline();

            select! {
                Some(duration) = activity_rx.recv() => {
                    let new_disabler = sleep(Duration::from_secs(duration as u64));

                    if new_disabler.deadline() > deadline {
                        next_disabler = Some(new_disabler);
                    }
                }
                _ = disabler => {
                    let _ = osc_tx
                        .send(OscMessage {
                            addr: "/avatar/parameters/PS_ShockActive".to_string(),
                            args: vec![OscType::Bool(false)],
                        })
                        .await;
                }
            }
        }
    }
}

pub struct PiShock {
    tx: mpsc::Sender<OscMessage>,
    rx: broadcast::Receiver<OscMessage>,
    config: Arc<Config>,
    data_dir: PathBuf,
}

impl PiShock {
    pub fn new(
        tx: mpsc::Sender<OscMessage>,
        rx: broadcast::Receiver<OscMessage>,
        config: Arc<Config>,
        data_dir: PathBuf,
    ) -> Self {
        Self {
            tx,
            rx,
            config,
            data_dir,
        }
    }

    async fn handle_buttons(&mut self) -> Result<()> {
        let (activity_tx, activity_rx) = mpsc::channel(8);
        let (shock_tx, shock_rx) = mpsc::channel(8);
        let (modifier_tx, modifier_rx) = mpsc::channel(8);
        let (delta_tx, delta_rx) = mpsc::channel(8);
        let (settings_tx, settings_rx) = mpsc::channel(8);

        let mut settings_storage = SettingsStorage::new(self.data_dir.clone()).await?;

        spawn(async move {
            let _ = settings_storage.run(settings_rx).await;
        });

        spawn(async move {
            let _ = handle_modifier(modifier_rx, delta_tx).await;
        });

        let delta_settings_tx = settings_tx.clone();
        let delta_osc_tx = self.tx.clone();

        spawn(async move {
            let _ = handle_delta(delta_rx, delta_settings_tx, delta_osc_tx).await;
        });

        let shock_settings_tx = settings_tx.clone();
        let shock_config = self.config.clone();
        let shock_activity_tx = activity_tx.clone();

        spawn(async move {
            let _ =
                handle_shock(shock_rx, shock_settings_tx, shock_config, shock_activity_tx).await;
        });

        let activity_osc_tx = self.tx.clone();

        spawn(async move {
            handle_activity(activity_rx, activity_osc_tx).await;
        });

        loop {
            match self.rx.recv().await {
                Ok(message) => match message.as_tuple() {
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
                    ("/avatar/parameters/PS_Intensity", &[OscType::Float(value)]) => {
                        if let Some(new_cap) = set_intensity(&settings_tx, value).await? {
                            let _ = self
                                .tx
                                .send(OscMessage {
                                    addr: "/avatar/parameters/PS_IntensityCap".to_string(),
                                    args: vec![OscType::Float(new_cap)],
                                })
                                .await;
                        }
                    }
                    ("/avatar/parameters/PS_IntensityCap", &[OscType::Float(value)]) => {
                        if let Some(new_intensity) = set_intensity_cap(&settings_tx, value).await? {
                            let _ = self
                                .tx
                                .send(OscMessage {
                                    addr: "/avatar/parameters/PS_Intensity".to_string(),
                                    args: vec![OscType::Float(new_intensity)],
                                })
                                .await;
                        }
                    }
                    ("/avatar/parameters/PS_QuickShock", &[OscType::Float(value)]) => {
                        if value >= 0. {
                            send_shock(&self.config, value, 1, &activity_tx).await;
                        }
                    }
                    ("/avatar/change", &[OscType::String(_)]) => {
                        let settings = get_settings(&settings_tx).await?;

                        let _ = self
                            .tx
                            .send(OscMessage {
                                addr: "/avatar/parameters/PS_Intensity".to_string(),
                                args: vec![OscType::Float(settings.intensity)],
                            })
                            .await;
                        let _ = self
                            .tx
                            .send(OscMessage {
                                addr: "/avatar/parameters/PS_IntensityCap".to_string(),
                                args: vec![OscType::Float(settings.intensity_cap)],
                            })
                            .await;
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
                            "PiShock lagging behind, {} messages have been dropped",
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
