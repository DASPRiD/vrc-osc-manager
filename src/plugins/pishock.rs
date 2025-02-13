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

async fn send_command(
    config: Arc<ConfigHandle<CoreConfig>>,
    code: String,
    intensity: f32,
    duration: u8,
    op: Operation,
) -> anyhow::Result<()> {
    let intensity = 1 + (99. * intensity) as u8;
    let duration = duration.clamp(1, 15);

    info!(
        "Sending {:?} to {} with intensity {} and duration {}",
        op, code, intensity, duration
    );

    let (username, api_key) = {
        let config = config.read().await;
        (config.username.clone(), config.api_key.clone())
    };

    let body = ShockBody {
        username,
        api_key,
        code,
        name: "VRC OSC Manager - PiShock Plugin".to_string(),
        op: op.into_byte(),
        duration,
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
                    "Not Authorized." => Err(anyhow!("Invalid credentials")),
                    "Operation Succeeded." => Ok(()),
                    "Operation Attempted." => Ok(()),
                    _ => Err(anyhow!("Unknown response: {}", status)),
                },
                Err(_) => Err(anyhow!("Failed to parse response")),
            }
        }
        Err(_) => Err(anyhow!("Failed to contact pishock API")),
    }
}

async fn send_commands(
    config: &Arc<ConfigHandle<CoreConfig>>,
    intensity: f32,
    duration: u8,
    activity_tx: &mpsc::Sender<u8>,
    op: Operation,
) {
    let codes = config.read().await.codes.clone();

    if codes.is_empty() {
        warn!("No codes configured");
        return;
    }

    let mut set = JoinSet::new();

    for code in codes {
        set.spawn(send_command
        (
            config.clone(),
            code.clone(),
            intensity,
            duration,
            op,
        ));
    }

    let mut succeeded = false;

    while let Some(res) = set.join_next().await {
        match res {
            Ok(Ok(())) => {
                debug!("Command succeeded");
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

    if succeeded && op.is_shock() {
        let _ = activity_tx.send(duration).await;
    }
}

struct ContinuousShockSender {
    core_config: Arc<ConfigHandle<CoreConfig>>,
    session_config: Arc<ConfigHandle<SessionConfig>>,
    cancellation_token: CancellationToken,
    activity_tx: mpsc::Sender<u8>,
}

impl ContinuousShockSender {
    pub fn new(
        core_config: Arc<ConfigHandle<CoreConfig>>,
        session_config: Arc<ConfigHandle<SessionConfig>>,
        cancellation_token: CancellationToken,
        activity_tx: mpsc::Sender<u8>,
    ) -> Self {
        Self {
            core_config,
            session_config,
            cancellation_token,
            activity_tx,
        }
    }

    async fn main_loop(&self) {
        let duration = self.core_config.read().await.duration;

        loop {
            let intensity = self.session_config.read().await.intensity;

            send_commands(&self.core_config, intensity, duration, &self.activity_tx, Operation::Shock).await;

            select! {
                _ = self.cancellation_token.cancelled() => break,
                _ = sleep(Duration::from_secs(duration as u64)) => continue,
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
    pub codes: Vec<String>,
    pub duration: u8,
}

impl Default for CoreConfig {
    fn default() -> Self {
        Self {
            username: "".to_string(),
            api_key: "".to_string(),
            codes: vec!["".to_string()],
            duration: 4,
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

#[derive(Debug, Clone, Copy)]
enum Operation {
    Shock,
    Vibrate,
}

impl Operation {
    fn into_byte(self) -> u8 {
        match self {
            Operation::Shock => 0,
            Operation::Vibrate => 1,
        }
    }

    fn is_shock(self) -> bool {
        match self {
            Operation::Shock => true,
            _ => false,
        }
    }
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
        let osc_tx = channels.create_osc_sender();
        let (activity_tx, activity_rx) = mpsc::channel(8);
        let mut osc_rx = channels.subscribe_to_osc();

        subsys.start(SubsystemBuilder::new("ActivityMonitor", {
            let osc_tx = osc_tx.clone();
            move |s| ActivityMonitor::new(activity_rx, osc_tx).run(s)
        }));

        self.send_state(&osc_tx).await;

        loop {
            match osc_rx.recv().await {
                Ok(message) => {
                    self.handle_osc_messages(message, &osc_tx, subsys, &activity_tx)
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

    async fn handle_osc_messages(
        &self,
        message: OscMessage,
        osc_tx: &mpsc::Sender<OscMessage>,
        subsys: &SubsystemHandle,
        activity_tx: &mpsc::Sender<u8>,
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
                self.check_shock_state(subsys, activity_tx).await;
            }
            ("/avatar/parameters/PS_ShockRight_Pressed", &[OscType::Bool(value)]) => {
                self.toggle_button(Button::ShockRight, value).await;
                self.check_shock_state(subsys, activity_tx).await;
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
            ("/avatar/parameters/PS_QuickVibrate", &[OscType::Float(value)]) => {
                if value >= 0. {
                    let state = self.session_config.read().await;

                    send_commands(
                        &self.core_config,
                        value.clamp(0., state.intensity_cap),
                        1,
                        activity_tx,
                        Operation::Vibrate,
                    )
                    .await;
                }
            }
            ("/avatar/parameters/PS_QuickShock", &[OscType::Float(value)]) => {
                if value >= 0. {
                    let state = self.session_config.read().await;

                    send_commands(
                        &self.core_config,
                        value.clamp(0., state.intensity_cap),
                        1,
                        activity_tx,
                        Operation::Shock,
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

    async fn check_shock_state(&self, subsys: &SubsystemHandle, activity_tx: &mpsc::Sender<u8>) {
        let mut state = self.state.write().await;

        match (
            state.cancel_shock.clone(),
            state.pressed_buttons.contains(&Button::ShockLeft)
                && state.pressed_buttons.contains(&Button::ShockRight),
        ) {
            (None, true) => {
                let cancellation_token = CancellationToken::new();

                subsys.start(SubsystemBuilder::new("ContinuousShockSender", {
                    let core_config = self.core_config.clone();
                    let session_config = self.session_config.clone();
                    let cancellation_token = cancellation_token.clone();
                    let activity_tx = activity_tx.clone();

                    move |s| {
                        ContinuousShockSender::new(
                            core_config,
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
        let codes = Self::parse_share_codes(settings.get_share_codes().to_string());

        self.core_config.blocking_update(|config| {
            config.username = username.clone();
            config.api_key = api_key.clone();
            config.duration = duration;
            config.codes = codes.clone();
        })?;

        settings.set_username(username.into());
        settings.set_api_key(api_key.into());
        settings.set_duration(duration as i32);
        settings.set_share_codes(codes.join("\n").into());
        settings.set_is_dirty(false);

        Ok(())
    }

    fn parse_share_codes(input: String) -> Vec<String> {
        input
            .lines()
            .map(str::trim)
            .map(|line| {
                if let Some(index) = line.find("?sharecode=") {
                    line[index + 11..].to_string()
                } else {
                    line.to_string()
                }
            })
            .filter(|line| !line.is_empty())
            .collect()
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
            "/avatar/parameters/PS_QuickVibrate".to_string(),
            "d".to_string(),
            OscAccess::ReadWrite,
            "Quick shock".to_string(),
        );
        service.add_endpoint(
            "/avatar/parameters/PS_QuickShock".to_string(),
            "d".to_string(),
            OscAccess::ReadWrite,
            "Quick vibrate".to_string(),
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

        Ok(())
    }

    fn open_settings(self: Arc<Self>, app_window: Weak<AppWindow>) -> anyhow::Result<()> {
        let app_window = app_window.unwrap();
        let settings = app_window.global::<PishockSettings>();
        let config = self.core_config.blocking_read().clone();

        settings.set_username(config.username.into());
        settings.set_api_key(config.api_key.into());
        settings.set_duration(config.duration as i32);
        settings.set_share_codes(config.codes.join("\n").into());
        settings.set_is_dirty(false);

        app_window
            .global::<Router>()
            .set_settings_page("pishock".into());

        Ok(())
    }
}
