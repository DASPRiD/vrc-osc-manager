use std::collections::HashSet;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use crate::osc_query::node::OscAccess;
use crate::osc_query::service::OscQueryServiceBuilder;
use crate::plugins::{ChannelManager, Plugin};
use crate::utils::config::{ConfigHandle, ConfigManager};
use crate::{AppWindow, PishockSettings, Router};
use anyhow::anyhow;
use async_osc::{prelude::OscMessageExt, OscMessage, OscType};
use async_trait::async_trait;
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use serde_repr::Serialize_repr;
use slint::{ComponentHandle, Weak};
use tokio::select;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::{mpsc, RwLock};
use tokio::task::JoinSet;
use tokio::time::sleep;
use tokio_graceful_shutdown::{
    errors::CancelledByShutdown, FutureExt, IntoSubsystem, SubsystemBuilder, SubsystemHandle,
};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Copy, Serialize_repr)]
#[repr(u8)]
#[allow(dead_code)]
enum Operation {
    Shock = 0,
    Vibrate = 1,
    Beep = 2,
}

#[derive(Serialize)]
struct OperateBody {
    #[serde(rename = "AgentName")]
    agent_name: String,

    #[serde(rename = "Operation")]
    operation: Operation,

    #[serde(rename = "Duration")]
    duration: u32,

    #[serde(rename = "Intensity")]
    intensity: u8,
}

#[derive(Deserialize)]
struct UserIdResponse {
    #[serde(rename = "UserId")]
    user_id: u64,
}

#[derive(Deserialize)]
struct DeviceResponse {
    shockers: Vec<ShockerInfo>,
}

#[derive(Deserialize)]
struct ShockerInfo {
    #[serde(rename = "shockerId")]
    shocker_id: u64,
}

struct ApiContext {
    client: reqwest::Client,
    api_key: String,
    shocker_ids: Arc<Vec<u64>>,
    duration: u8,
}

async fn fetch_user_id(
    client: &reqwest::Client,
    api_key: &str,
    username: &str,
) -> anyhow::Result<u64> {
    let response = client
        .get("https://auth.pishock.com/Auth/GetUserIfAPIKeyValid")
        .query(&[("apikey", api_key), ("username", username)])
        .send()
        .await?
        .error_for_status()?
        .json::<UserIdResponse>()
        .await?;

    Ok(response.user_id)
}

async fn fetch_shocker_ids(
    client: &reqwest::Client,
    api_key: &str,
    user_id: u64,
) -> anyhow::Result<Vec<u64>> {
    let user_id_str = user_id.to_string();
    let devices = client
        .get("https://ps.pishock.com/PiShock/GetUserDevices")
        .query(&[
            ("UserId", user_id_str.as_str()),
            ("Token", api_key),
            ("api", "true"),
        ])
        .send()
        .await?
        .error_for_status()?
        .json::<Vec<DeviceResponse>>()
        .await?;

    let shocker_ids: Vec<u64> = devices
        .into_iter()
        .flat_map(|device| device.shockers.into_iter().map(|s| s.shocker_id))
        .collect();

    Ok(shocker_ids)
}

async fn send_shock(
    client: reqwest::Client,
    api_key: String,
    shocker_id: u64,
    intensity: f32,
    duration: u8,
) -> anyhow::Result<()> {
    let intensity = 1 + (99. * intensity) as u8;
    let duration_ms = (duration as u32) * 1000;

    info!(
        "Sending shock to shocker {} with intensity {} and duration {}s",
        shocker_id, intensity, duration
    );

    let body = OperateBody {
        agent_name: "VRC OSC Manager - PiShock Plugin".to_string(),
        operation: Operation::Shock,
        duration: duration_ms,
        intensity,
    };

    let response = client
        .post(format!("https://api.pishock.com/Shockers/{}", shocker_id))
        .header("X-PiShock-Api-Key", &api_key)
        .json(&body)
        .send()
        .await;

    match response {
        Ok(response) => {
            if response.status().is_success() {
                Ok(())
            } else {
                Err(anyhow!(
                    "Shock request failed with status {}",
                    response.status()
                ))
            }
        }
        Err(_) => Err(anyhow!("Failed to contact PiShock API")),
    }
}

async fn send_shocks(
    client: &reqwest::Client,
    api_key: &str,
    shocker_ids: &[u64],
    intensity: f32,
    duration: u8,
    activity_tx: &mpsc::Sender<u8>,
) {
    if shocker_ids.is_empty() {
        warn!("No shockers available");
        return;
    }

    let mut set = JoinSet::new();

    for &shocker_id in shocker_ids {
        set.spawn(send_shock(
            client.clone(),
            api_key.to_string(),
            shocker_id,
            intensity,
            duration,
        ));
    }

    let mut succeeded = false;

    while let Some(res) = set.join_next().await {
        match res {
            Ok(Ok(())) => {
                debug!("Shock succeeded");
                succeeded = true;
            }
            Ok(Err(error)) => {
                warn!("{}", error);
            }
            Err(error) => {
                error!("{}", error);
            }
        }
    }

    if succeeded {
        let _ = activity_tx.send(duration).await;
    }
}

async fn run_connection_test(username: String, api_key: String) -> anyhow::Result<()> {
    if username.is_empty() || api_key.is_empty() {
        return Err(anyhow!("Username and API key are required"));
    }

    let client = reqwest::Client::new();
    let user_id = fetch_user_id(&client, &api_key, &username).await?;
    let shocker_ids = fetch_shocker_ids(&client, &api_key, user_id).await?;

    if shocker_ids.is_empty() {
        return Err(anyhow!("No shockers found"));
    }

    let body = OperateBody {
        agent_name: "VRC OSC Manager - PiShock Plugin".to_string(),
        operation: Operation::Beep,
        duration: 1000,
        intensity: 20,
    };

    let mut any_succeeded = false;
    let mut last_error = None;

    let mut set = JoinSet::new();
    for &shocker_id in &shocker_ids {
        let client = client.clone();
        let api_key = api_key.clone();
        let body_json = serde_json::to_value(&body).unwrap();
        set.spawn(async move {
            let response = client
                .post(format!("https://api.pishock.com/Shockers/{}", shocker_id))
                .header("X-PiShock-Api-Key", &api_key)
                .json(&body_json)
                .send()
                .await
                .map_err(|_| anyhow!("Failed to contact PiShock API"))?;

            if response.status().is_success() {
                Ok(())
            } else {
                Err(anyhow!("Status {}", response.status()))
            }
        });
    }

    while let Some(res) = set.join_next().await {
        match res {
            Ok(Ok(())) => any_succeeded = true,
            Ok(Err(e)) => last_error = Some(e),
            Err(e) => last_error = Some(anyhow!("{}", e)),
        }
    }

    if any_succeeded {
        Ok(())
    } else {
        Err(last_error.unwrap_or_else(|| anyhow!("Unknown error")))
    }
}

struct ContinuousShockSender {
    client: reqwest::Client,
    api_key: String,
    shocker_ids: Arc<Vec<u64>>,
    duration: u8,
    session_config: Arc<ConfigHandle<SessionConfig>>,
    cancellation_token: CancellationToken,
    activity_tx: mpsc::Sender<u8>,
}

impl ContinuousShockSender {
    pub fn new(
        client: reqwest::Client,
        api_key: String,
        shocker_ids: Arc<Vec<u64>>,
        duration: u8,
        session_config: Arc<ConfigHandle<SessionConfig>>,
        cancellation_token: CancellationToken,
        activity_tx: mpsc::Sender<u8>,
    ) -> Self {
        Self {
            client,
            api_key,
            shocker_ids,
            duration,
            session_config,
            cancellation_token,
            activity_tx,
        }
    }

    async fn main_loop(&self) {
        loop {
            let intensity = self.session_config.read().await.intensity;

            send_shocks(
                &self.client,
                &self.api_key,
                &self.shocker_ids,
                intensity,
                self.duration,
                &self.activity_tx,
            )
            .await;

            select! {
                _ = self.cancellation_token.cancelled() => break,
                _ = sleep(Duration::from_secs(self.duration as u64)) => continue,
            }
        }
    }
}

#[async_trait]
impl IntoSubsystem<Infallible> for ContinuousShockSender {
    async fn run(mut self, subsys: SubsystemHandle) -> Result<(), Infallible> {
        let _ = self.main_loop().cancel_on_shutdown(&subsys).await;
        Ok(())
    }
}

struct IntensityModifier {
    base: f32,
    osc_tx: mpsc::Sender<OscMessage>,
    session_config: Arc<ConfigHandle<SessionConfig>>,
    cancellation_token: CancellationToken,
}

impl IntensityModifier {
    pub fn new(
        base: f32,
        osc_tx: mpsc::Sender<OscMessage>,
        session_config: Arc<ConfigHandle<SessionConfig>>,
        cancellation_token: CancellationToken,
    ) -> Self {
        Self {
            base,
            osc_tx,
            session_config,
            cancellation_token,
        }
    }

    async fn main_loop(&self) {
        let delta = 0.01 * self.base;
        self.modify_intensity(delta).await;

        sleep(Duration::from_millis(900)).await;

        loop {
            select! {
                _ = self.cancellation_token.cancelled() => break,
                _ = sleep(Duration::from_millis(100)) => self.modify_intensity(delta).await,
            }
        }
    }

    pub async fn modify_intensity(&self, delta: f32) {
        let result = self
            .session_config
            .update(|config| {
                let intensity = (config.intensity + delta).clamp(0., config.intensity_cap);
                config.intensity = intensity;
                intensity
            })
            .await;

        let intensity = match result {
            Ok(intensity) => intensity,
            Err(error) => {
                warn!("Failed to store PiShock session config: {}", error);
                return;
            }
        };

        let _ = self
            .osc_tx
            .send(OscMessage {
                addr: "/avatar/parameters/PS_Intensity".to_string(),
                args: vec![OscType::Float(intensity)],
            })
            .await;
    }
}

#[async_trait]
impl IntoSubsystem<Infallible> for IntensityModifier {
    async fn run(mut self, subsys: SubsystemHandle) -> Result<(), Infallible> {
        let _ = self.main_loop().cancel_on_shutdown(&subsys).await;
        Ok(())
    }
}

struct ActivityMonitor {
    activity_rx: mpsc::Receiver<u8>,
    osc_tx: mpsc::Sender<OscMessage>,
}

impl ActivityMonitor {
    fn new(activity_rx: mpsc::Receiver<u8>, osc_tx: mpsc::Sender<OscMessage>) -> Self {
        Self {
            activity_rx,
            osc_tx,
        }
    }

    async fn main_loop(&mut self) {
        while let Some(duration) = self.activity_rx.recv().await {
            let _ = self
                .osc_tx
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
                    Some(duration) = self.activity_rx.recv() => {
                        let new_disabler = sleep(Duration::from_secs(duration as u64));

                        if new_disabler.deadline() > deadline {
                            next_disabler = Some(new_disabler);
                        }
                    }
                    _ = disabler => {
                        let _ = self.osc_tx
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
}

#[async_trait]
impl IntoSubsystem<Infallible> for ActivityMonitor {
    async fn run(mut self, subsys: SubsystemHandle) -> Result<(), Infallible> {
        let _ = self.main_loop().cancel_on_shutdown(&subsys).await;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CoreConfig {
    pub username: String,
    pub api_key: String,
    pub duration: u8,
    pub user_id: Option<u64>,
}

impl Default for CoreConfig {
    fn default() -> Self {
        Self {
            username: "".to_string(),
            api_key: "".to_string(),
            duration: 4,
            user_id: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct SessionConfig {
    intensity: f32,
    intensity_cap: f32,
}

impl SessionConfig {
    fn set_intensity(&mut self, intensity: f32) -> Option<f32> {
        self.intensity = intensity.clamp(0., 1.);

        if self.intensity_cap < self.intensity {
            self.intensity_cap = self.intensity;
            return Some(self.intensity_cap);
        }

        None
    }

    fn set_intensity_cap(&mut self, cap: f32) -> Option<f32> {
        self.intensity_cap = cap.clamp(0., 1.);

        if self.intensity > self.intensity_cap {
            self.intensity = self.intensity_cap;
            return Some(self.intensity);
        }

        None
    }
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            intensity: 0.,
            intensity_cap: 1.,
        }
    }
}

#[derive(PartialEq, Eq, Hash)]
enum Button {
    Minus,
    Plus,
    ShockLeft,
    ShockRight,
}

#[derive(Default)]
struct State {
    pressed_buttons: HashSet<Button>,
    cancel_shock: Option<CancellationToken>,
    cancel_modification: Option<CancellationToken>,
}

impl State {
    fn reset(&mut self) {
        self.pressed_buttons.clear();
        self.cancel_shock = None;
        self.cancel_modification = None;
    }
}

pub struct PiShock {
    core_config: Arc<ConfigHandle<CoreConfig>>,
    session_config: Arc<ConfigHandle<SessionConfig>>,
    state: Arc<RwLock<State>>,
}

impl PiShock {
    async fn main_loop(
        &self,
        subsys: &SubsystemHandle,
        channels: Arc<ChannelManager>,
    ) -> anyhow::Result<()> {
        let client = reqwest::Client::new();
        let osc_tx = channels.create_osc_sender();
        let (activity_tx, activity_rx) = mpsc::channel(8);
        let mut osc_rx = channels.subscribe_to_osc();

        subsys.start(SubsystemBuilder::new("ActivityMonitor", {
            let osc_tx = osc_tx.clone();
            move |s| ActivityMonitor::new(activity_rx, osc_tx).run(s)
        }));

        let config = self.core_config.read().await.clone();
        let shocker_ids = if config.username.is_empty() || config.api_key.is_empty() {
            warn!("PiShock credentials not configured");
            vec![]
        } else {
            match self.resolve_shocker_ids(&client, &config).await {
                Ok(ids) => {
                    info!("Found {} shocker(s)", ids.len());
                    ids
                }
                Err(error) => {
                    warn!("Failed to initialize PiShock API: {}", error);
                    vec![]
                }
            }
        };

        let api = ApiContext {
            client,
            api_key: config.api_key.clone(),
            shocker_ids: Arc::new(shocker_ids),
            duration: config.duration,
        };

        self.send_state(&osc_tx).await;

        loop {
            match osc_rx.recv().await {
                Ok(message) => {
                    self.handle_osc_messages(message, &osc_tx, subsys, &activity_tx, &api)
                        .await?;
                }
                Err(RecvError::Closed) => break,
                Err(RecvError::Lagged(skipped)) => {
                    warn!(
                        "PiShock lagging behind, {} messages have been dropped",
                        skipped
                    );
                }
            }
        }

        Ok(())
    }

    async fn resolve_shocker_ids(
        &self,
        client: &reqwest::Client,
        config: &CoreConfig,
    ) -> anyhow::Result<Vec<u64>> {
        let user_id = match config.user_id {
            Some(id) => id,
            None => {
                let id = fetch_user_id(client, &config.api_key, &config.username).await?;
                let _ = self
                    .core_config
                    .update(|c| {
                        c.user_id = Some(id);
                    })
                    .await;
                id
            }
        };

        fetch_shocker_ids(client, &config.api_key, user_id).await
    }

    async fn handle_osc_messages(
        &self,
        message: OscMessage,
        osc_tx: &mpsc::Sender<OscMessage>,
        subsys: &SubsystemHandle,
        activity_tx: &mpsc::Sender<u8>,
        api: &ApiContext,
    ) -> anyhow::Result<()> {
        match message.as_tuple() {
            ("/avatar/parameters/PS_Minus_Pressed", &[OscType::Bool(value)]) => {
                self.toggle_button(Button::Minus, value).await;
                self.check_modifier_state(subsys, osc_tx).await;
            }
            ("/avatar/parameters/PS_Plus_Pressed", &[OscType::Bool(value)]) => {
                self.toggle_button(Button::Plus, value).await;
                self.check_modifier_state(subsys, osc_tx).await;
            }
            ("/avatar/parameters/PS_ShockLeft_Pressed", &[OscType::Bool(value)]) => {
                self.toggle_button(Button::ShockLeft, value).await;
                self.check_shock_state(subsys, activity_tx, api).await;
            }
            ("/avatar/parameters/PS_ShockRight_Pressed", &[OscType::Bool(value)]) => {
                self.toggle_button(Button::ShockRight, value).await;
                self.check_shock_state(subsys, activity_tx, api).await;
            }
            ("/avatar/parameters/PS_Intensity", &[OscType::Float(value)]) => {
                let new_cap = self
                    .session_config
                    .update(|config| config.set_intensity(value))
                    .await?;

                if let Some(new_cap) = new_cap {
                    let _ = osc_tx
                        .send(OscMessage {
                            addr: "/avatar/parameters/PS_IntensityCap".to_string(),
                            args: vec![OscType::Float(new_cap)],
                        })
                        .await;
                }
            }
            ("/avatar/parameters/PS_IntensityCap", &[OscType::Float(value)]) => {
                let new_intensity = self
                    .session_config
                    .update(|config| config.set_intensity_cap(value))
                    .await?;

                if let Some(new_intensity) = new_intensity {
                    let _ = osc_tx
                        .send(OscMessage {
                            addr: "/avatar/parameters/PS_Intensity".to_string(),
                            args: vec![OscType::Float(new_intensity)],
                        })
                        .await;
                }
            }
            ("/avatar/parameters/PS_QuickShock", &[OscType::Float(value)]) => {
                if value >= 0. {
                    let state = self.session_config.read().await;

                    send_shocks(
                        &api.client,
                        &api.api_key,
                        &api.shocker_ids,
                        value.clamp(0., state.intensity_cap),
                        1,
                        activity_tx,
                    )
                    .await;
                }
            }
            ("/avatar/change", &[OscType::String(_)]) => {
                self.send_state(osc_tx).await;
            }
            _ => {}
        }

        Ok(())
    }

    async fn toggle_button(&self, button: Button, pressed: bool) {
        let mut state = self.state.write().await;

        if pressed {
            state.pressed_buttons.insert(button);
        } else {
            state.pressed_buttons.remove(&button);
        }
    }

    async fn check_modifier_state(
        &self,
        subsys: &SubsystemHandle,
        osc_tx: &mpsc::Sender<OscMessage>,
    ) {
        let mut state = self.state.write().await;

        match (
            state.cancel_modification.clone(),
            state.pressed_buttons.contains(&Button::Minus),
            state.pressed_buttons.contains(&Button::Plus),
        ) {
            (None, true, false) => {
                state.cancel_modification =
                    Some(self.start_intensity_modifier(subsys, -1., osc_tx.clone()));
            }
            (None, false, true) => {
                state.cancel_modification =
                    Some(self.start_intensity_modifier(subsys, 1., osc_tx.clone()));
            }
            (Some(token), _, _) => {
                token.cancel();
                state.cancel_modification = None;
            }
            _ => {}
        }
    }

    fn start_intensity_modifier(
        &self,
        subsys: &SubsystemHandle,
        base: f32,
        osc_tx: mpsc::Sender<OscMessage>,
    ) -> CancellationToken {
        let cancellation_token = CancellationToken::new();

        subsys.start(SubsystemBuilder::new("IntensityModifier", {
            let cancellation_token = cancellation_token.clone();
            let session_config = self.session_config.clone();

            move |s| {
                IntensityModifier::new(base, osc_tx.clone(), session_config, cancellation_token)
                    .run(s)
            }
        }));

        cancellation_token
    }

    async fn check_shock_state(
        &self,
        subsys: &SubsystemHandle,
        activity_tx: &mpsc::Sender<u8>,
        api: &ApiContext,
    ) {
        let mut state = self.state.write().await;

        match (
            state.cancel_shock.clone(),
            state.pressed_buttons.contains(&Button::ShockLeft)
                && state.pressed_buttons.contains(&Button::ShockRight),
        ) {
            (None, true) => {
                let cancellation_token = CancellationToken::new();

                subsys.start(SubsystemBuilder::new("ContinuousShockSender", {
                    let client = api.client.clone();
                    let api_key = api.api_key.clone();
                    let shocker_ids = api.shocker_ids.clone();
                    let duration = api.duration;
                    let session_config = self.session_config.clone();
                    let cancellation_token = cancellation_token.clone();
                    let activity_tx = activity_tx.clone();

                    move |s| {
                        ContinuousShockSender::new(
                            client,
                            api_key,
                            shocker_ids,
                            duration,
                            session_config,
                            cancellation_token,
                            activity_tx,
                        )
                        .run(s)
                    }
                }));

                state.cancel_shock = Some(cancellation_token);
            }
            (Some(token), false) => {
                token.cancel();
                state.cancel_shock = None;
            }
            _ => {}
        }
    }

    async fn send_state(&self, osc_tx: &mpsc::Sender<OscMessage>) {
        let state = self.session_config.read().await;

        let _ = osc_tx
            .send(OscMessage {
                addr: "/avatar/parameters/PS_Intensity".to_string(),
                args: vec![OscType::Float(state.intensity)],
            })
            .await;
        let _ = osc_tx
            .send(OscMessage {
                addr: "/avatar/parameters/PS_IntensityCap".to_string(),
                args: vec![OscType::Float(state.intensity_cap)],
            })
            .await;
    }

    fn store_settings(&self, settings: &PishockSettings) -> anyhow::Result<()> {
        let username = settings.get_username().to_string().trim().to_string();
        let api_key = settings.get_api_key().to_string().trim().to_string();
        let duration = settings.get_duration().clamp(1, 15) as u8;

        self.core_config.blocking_update(|config| {
            if config.username != username || config.api_key != api_key {
                config.user_id = None;
            }
            config.username = username.clone();
            config.api_key = api_key.clone();
            config.duration = duration;
        })?;

        settings.set_username(username.into());
        settings.set_api_key(api_key.into());
        settings.set_duration(duration as i32);
        settings.set_is_dirty(false);

        Ok(())
    }
}

#[async_trait]
impl Plugin for PiShock {
    fn new(config_manager: ConfigManager) -> Self {
        Self {
            core_config: Arc::new(config_manager.load_config(None, None)),
            session_config: Arc::new(config_manager.load_config(Some("state"), None)),
            state: Arc::new(RwLock::new(State::default())),
        }
    }

    fn title(&self) -> &'static str {
        "PiShock Controller"
    }

    fn description(&self) -> &'static str {
        "Let other people control your PiShock shockers via an in-game interface."
    }

    fn info_url(&self) -> Option<&'static str> {
        Some("https://dasprid.gumroad.com/l/llfyq")
    }

    async fn run(
        &self,
        subsys: &SubsystemHandle,
        channels: Arc<ChannelManager>,
    ) -> anyhow::Result<()> {
        self.state.write().await.reset();

        match self
            .main_loop(subsys, channels)
            .cancel_on_shutdown(subsys)
            .await
        {
            Ok(Ok(())) => subsys.request_shutdown(),
            Ok(Err(error)) => return Err(error),
            Err(CancelledByShutdown) => {}
        }

        Ok(())
    }

    fn register_osc_parameters(&self, service: &mut OscQueryServiceBuilder) {
        service.add_endpoint(
            "/avatar/parameters/PS_Minus_Pressed".to_string(),
            "b".to_string(),
            OscAccess::Write,
            "Minus button pressed".to_string(),
        );
        service.add_endpoint(
            "/avatar/parameters/PS_Plus_Pressed".to_string(),
            "b".to_string(),
            OscAccess::Write,
            "Plus button pressed".to_string(),
        );
        service.add_endpoint(
            "/avatar/parameters/PS_ShockLeft_Pressed".to_string(),
            "b".to_string(),
            OscAccess::Write,
            "Left shock button pressed".to_string(),
        );
        service.add_endpoint(
            "/avatar/parameters/PS_ShockRight_Pressed".to_string(),
            "b".to_string(),
            OscAccess::Write,
            "Right shock button pressed".to_string(),
        );
        service.add_endpoint(
            "/avatar/parameters/PS_Intensity".to_string(),
            "d".to_string(),
            OscAccess::ReadWrite,
            "Shock intensity".to_string(),
        );
        service.add_endpoint(
            "/avatar/parameters/PS_IntensityCap".to_string(),
            "d".to_string(),
            OscAccess::ReadWrite,
            "Shock intensity cap".to_string(),
        );
        service.add_endpoint(
            "/avatar/parameters/PS_QuickShock".to_string(),
            "d".to_string(),
            OscAccess::ReadWrite,
            "Quick shock".to_string(),
        );
    }

    fn has_settings(&self) -> bool {
        true
    }

    fn register_settings_callbacks(self: Arc<Self>, app_window: &AppWindow) -> anyhow::Result<()> {
        let settings = app_window.global::<PishockSettings>();

        settings.on_cancel({
            let app_window = app_window.as_weak();

            move || {
                app_window
                    .unwrap()
                    .global::<Router>()
                    .set_settings_page("".into());
            }
        });

        settings.on_apply({
            let app_window = app_window.as_weak();
            let plugin = self.clone();

            move || {
                let app_window = app_window.unwrap();
                let settings = app_window.global::<PishockSettings>();

                let _ = plugin.store_settings(&settings);
            }
        });

        settings.on_okay({
            let app_window = app_window.as_weak();
            let plugin = self.clone();

            move || {
                let app_window = app_window.unwrap();
                let settings = app_window.global::<PishockSettings>();

                if settings.get_is_dirty() {
                    let _ = plugin.store_settings(&settings);
                }

                app_window.global::<Router>().set_settings_page("".into());
            }
        });

        settings.on_test({
            let app_window = app_window.as_weak();

            move || {
                let handle = app_window.unwrap();
                let settings = handle.global::<PishockSettings>();

                let username = settings.get_username().to_string().trim().to_string();
                let api_key = settings.get_api_key().to_string().trim().to_string();

                settings.set_test_running(true);
                settings.set_test_status("".into());

                let app_window = app_window.clone();
                std::thread::spawn(move || {
                    let rt = tokio::runtime::Runtime::new().unwrap();
                    let result = rt.block_on(run_connection_test(username, api_key));

                    let _ = app_window.upgrade_in_event_loop(move |handle| {
                        let settings = handle.global::<PishockSettings>();
                        settings.set_test_running(false);
                        match result {
                            Ok(()) => {
                                settings.set_test_success(true);
                                settings.set_test_status("Connection successful!".into());
                            }
                            Err(error) => {
                                settings.set_test_success(false);
                                settings.set_test_status(format!("{}", error).into());
                            }
                        }
                    });
                });
            }
        });

        Ok(())
    }

    fn open_settings(self: Arc<Self>, app_window: Weak<AppWindow>) -> anyhow::Result<()> {
        let app_window = app_window.unwrap();
        let settings = app_window.global::<PishockSettings>();
        let config = self.core_config.blocking_read().clone();

        settings.set_username(config.username.into());
        settings.set_api_key(config.api_key.into());
        settings.set_duration(config.duration as i32);
        settings.set_is_dirty(false);
        settings.set_test_status("".into());
        settings.set_test_running(false);

        app_window
            .global::<Router>()
            .set_settings_page("pishock".into());

        Ok(())
    }
}
