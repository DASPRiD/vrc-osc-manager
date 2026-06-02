use std::collections::HashMap;
use std::sync::Arc;

use async_osc::prelude::OscMessageExt;
use async_osc::{OscMessage, OscType};
use log::warn;
use serde::{Deserialize, Serialize};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel, Weak};
use tokio::select;
use tokio::sync::broadcast::error::RecvError;
use tokio_graceful_shutdown::SubsystemHandle;
use tokio_stream::StreamExt;
use zbus::fdo::DBusProxy;
use zbus::message::Type as MessageType;
use zbus::names::BusName;
use zbus::proxy::CacheProperties;
use zbus::zvariant::{OwnedValue, Value};
use zbus::{proxy, Connection, MatchRule, MessageStream};

use crate::plugins::ChannelManager;
use crate::utils::config::ConfigHandle;
use crate::{AppWindow, MediaControlSettings, Router};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct MediaControlConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    mpris_target: Option<String>,
}

impl MediaControlConfig {
    pub(super) fn target(&self) -> Option<String> {
        self.mpris_target.clone()
    }

    pub(super) fn set_target(&mut self, target: Option<String>) {
        self.mpris_target = target;
    }
}

#[proxy(
    interface = "org.mpris.MediaPlayer2.Player",
    default_path = "/org/mpris/MediaPlayer2"
)]
trait Player {
    fn play_pause(&self) -> zbus::Result<()>;
    fn next(&self) -> zbus::Result<()>;
    fn previous(&self) -> zbus::Result<()>;
    fn stop(&self) -> zbus::Result<()>;

    #[zbus(property)]
    fn playback_status(&self) -> zbus::Result<String>;
}

#[proxy(
    interface = "org.mpris.MediaPlayer2",
    default_path = "/org/mpris/MediaPlayer2"
)]
trait MediaPlayer2 {
    #[zbus(property)]
    fn identity(&self) -> zbus::Result<String>;
}

#[derive(Clone, Copy)]
enum MediaCommand {
    PlayPause,
    Next,
    Previous,
    Stop,
}

pub(super) async fn run(
    subsys: &SubsystemHandle,
    config: &ConfigHandle<MediaControlConfig>,
    channels: Arc<ChannelManager>,
) -> anyhow::Result<()> {
    let target = config.read().await.mpris_target.clone();
    let connection = Connection::session().await?;
    let dbus = DBusProxy::new(&connection).await?;

    let mut name_changes = dbus.receive_name_owner_changed().await?;

    let property_rule = MatchRule::builder()
        .msg_type(MessageType::Signal)
        .interface("org.freedesktop.DBus.Properties")?
        .member("PropertiesChanged")?
        .path("/org/mpris/MediaPlayer2")?
        .build();
    let mut property_changes =
        MessageStream::for_match_rule(property_rule, &connection, None).await?;

    let mut state = MprisState::default();
    let mut unique_to_well: HashMap<String, String> = HashMap::new();

    for owned in dbus.list_names().await? {
        let name = owned.to_string();

        if !is_mpris_player(&name) {
            continue;
        }

        if let Ok(bus_name) = BusName::try_from(name.as_str()) {
            if let Ok(unique) = dbus.get_name_owner(bus_name).await {
                unique_to_well.insert(unique.to_string(), name.clone());
            }
        }

        let playing = query_playing(&connection, &name).await;
        state.on_appear(name, playing);
    }

    let mut osc_rx = channels.subscribe_to_osc();

    loop {
        select! {
            _ = subsys.on_shutdown_requested() => break,
            osc = osc_rx.recv() => {
                match osc {
                    Ok(message) => {
                        if let Some(command) = media_command(&message) {
                            let player = match &target {
                                Some(target) => resolve_pinned(state.players(), target),
                                None => state.active.clone(),
                            };

                            if let Some(player) = player {
                                if let Err(error) =
                                    dispatch_command(&connection, &player, command).await
                                {
                                    warn!("Failed to send media command to {}: {}", player, error);
                                }
                            }
                        }
                    }
                    Err(RecvError::Closed) => break,
                    Err(RecvError::Lagged(skipped)) => {
                        warn!(
                            "MediaControl lagging behind, {} messages have been dropped",
                            skipped
                        );
                    }
                }
            }
            Some(signal) = name_changes.next() => {
                let Ok(args) = signal.args() else {
                    continue;
                };

                let name = args.name().to_string();

                if !is_mpris_player(&name) {
                    continue;
                }

                unique_to_well.retain(|_, well| well != &name);
                state.on_vanish(&name);

                if let Some(unique) = args.new_owner().as_ref() {
                    unique_to_well.insert(unique.to_string(), name.clone());
                    let playing = query_playing(&connection, &name).await;
                    state.on_appear(name, playing);
                }
            }
            Some(message) = property_changes.next() => {
                let Ok(message) = message else {
                    continue;
                };

                let Some(sender) = message.header().sender().map(ToString::to_string) else {
                    continue;
                };

                let Some(name) = unique_to_well.get(&sender).cloned() else {
                    continue;
                };

                let body = message.body();
                let Ok((_, changed, invalidated)) =
                    body.deserialize::<(String, HashMap<String, OwnedValue>, Vec<String>)>()
                else {
                    continue;
                };

                let playing = if let Some(value) = changed.get("PlaybackStatus") {
                    Some(is_playing_value(value))
                } else if invalidated.iter().any(|property| property == "PlaybackStatus") {
                    Some(query_playing(&connection, &name).await)
                } else {
                    None
                };

                if let Some(playing) = playing {
                    state.on_status(&name, playing);
                }
            }
        }
    }

    Ok(())
}

#[derive(Default)]
struct MprisState {
    playing: HashMap<String, bool>,
    active: Option<String>,
    parked: Vec<String>,
}

impl MprisState {
    fn active_playing(&self) -> bool {
        self.active
            .as_ref()
            .is_some_and(|active| self.playing.get(active).copied().unwrap_or(false))
    }

    fn on_appear(&mut self, name: String, playing: bool) {
        self.playing.insert(name.clone(), playing);

        if self.active.is_some() && self.active_playing() {
            self.parked.push(name);
        } else {
            if let Some(previous) = self.active.take() {
                self.parked.push(previous);
            }

            self.active = Some(name);
        }
    }

    fn on_status(&mut self, name: &str, playing: bool) {
        self.playing.insert(name.to_string(), playing);

        if self.active.as_deref() == Some(name) || self.active_playing() {
            return;
        }

        if playing {
            self.parked.retain(|parked| parked != name);

            if let Some(previous) = self.active.take() {
                self.parked.push(previous);
            }

            self.active = Some(name.to_string());
        }
    }

    fn on_vanish(&mut self, name: &str) {
        self.playing.remove(name);

        if self.active.as_deref() == Some(name) {
            self.active = self.parked.pop();
        } else {
            self.parked.retain(|parked| parked != name);
        }
    }

    fn players(&self) -> impl Iterator<Item = &String> {
        self.playing.keys()
    }
}

fn is_mpris_player(name: &str) -> bool {
    name.starts_with("org.mpris.MediaPlayer2.") && player_segment(name) != "playerctld"
}

fn is_playing_value(value: &OwnedValue) -> bool {
    matches!(&**value, Value::Str(status) if status == "Playing")
}

fn media_command(message: &OscMessage) -> Option<MediaCommand> {
    match message.as_tuple() {
        ("/avatar/parameters/MC_PrevTrack", &[OscType::Bool(value)]) if value => {
            Some(MediaCommand::Previous)
        }
        ("/avatar/parameters/MC_NextTrack", &[OscType::Bool(value)]) if value => {
            Some(MediaCommand::Next)
        }
        ("/avatar/parameters/MC_PlayPause", &[OscType::Bool(value)]) if value => {
            Some(MediaCommand::PlayPause)
        }
        ("/avatar/parameters/MC_Stop", &[OscType::Bool(value)]) if value => {
            Some(MediaCommand::Stop)
        }
        _ => None,
    }
}

fn player_segment(name: &str) -> &str {
    name.strip_prefix("org.mpris.MediaPlayer2.")
        .unwrap_or(name)
        .split(".instance")
        .next()
        .unwrap_or(name)
}

fn resolve_pinned<'a>(
    mut players: impl Iterator<Item = &'a String>,
    target: &str,
) -> Option<String> {
    players
        .find(|name| player_segment(name).eq_ignore_ascii_case(target))
        .cloned()
}

async fn query_playing(connection: &Connection, name: &str) -> bool {
    let status: zbus::Result<String> = async {
        let player = PlayerProxy::builder(connection)
            .destination(name.to_string())?
            .cache_properties(CacheProperties::No)
            .build()
            .await?;
        player.playback_status().await
    }
    .await;

    matches!(status.as_deref(), Ok("Playing"))
}

async fn dispatch_command(
    connection: &Connection,
    name: &str,
    command: MediaCommand,
) -> zbus::Result<()> {
    let player = PlayerProxy::builder(connection)
        .destination(name.to_string())?
        .cache_properties(CacheProperties::No)
        .build()
        .await?;

    match command {
        MediaCommand::PlayPause => player.play_pause().await,
        MediaCommand::Next => player.next().await,
        MediaCommand::Previous => player.previous().await,
        MediaCommand::Stop => player.stop().await,
    }
}

async fn test_play_pause(target: Option<String>) -> String {
    let outcome: zbus::Result<Option<String>> = async {
        let connection = Connection::session().await?;
        let dbus = DBusProxy::new(&connection).await?;

        let players: Vec<String> = dbus
            .list_names()
            .await?
            .into_iter()
            .map(|name| name.to_string())
            .filter(|name| is_mpris_player(name))
            .collect();

        let resolved = match &target {
            Some(target) => resolve_pinned(players.iter(), target),
            None => {
                let mut state = MprisState::default();

                for name in &players {
                    let playing = query_playing(&connection, name).await;
                    state.on_appear(name.clone(), playing);
                }

                state.active
            }
        };

        let Some(name) = resolved else {
            return Ok(None);
        };

        dispatch_command(&connection, &name, MediaCommand::PlayPause).await?;
        let label = identity(&connection, &name)
            .await
            .unwrap_or_else(|| player_segment(&name).to_string());
        Ok(Some(label))
    }
    .await;

    match outcome {
        Ok(Some(label)) => format!("Sent play/pause to {}", label),
        Ok(None) => "No matching player is running".to_string(),
        Err(error) => format!("Failed: {}", error),
    }
}

async fn identity(connection: &Connection, name: &str) -> Option<String> {
    let result: zbus::Result<String> = async {
        let player = MediaPlayer2Proxy::builder(connection)
            .destination(name.to_string())?
            .cache_properties(CacheProperties::No)
            .build()
            .await?;
        player.identity().await
    }
    .await;

    result.ok()
}

async fn enumerate_players(saved_target: Option<&str>) -> Vec<(String, String)> {
    let mut options: Vec<(String, String)> = Vec::new();

    let collected: zbus::Result<()> = async {
        let connection = Connection::session().await?;
        let dbus = DBusProxy::new(&connection).await?;

        for owned in dbus.list_names().await? {
            let name = owned.to_string();

            if !is_mpris_player(&name) {
                continue;
            }

            let value = player_segment(&name).to_string();

            if options
                .iter()
                .any(|(_, existing)| existing.eq_ignore_ascii_case(&value))
            {
                continue;
            }

            let label = identity(&connection, &name)
                .await
                .unwrap_or_else(|| value.clone());
            options.push((label, value));
        }

        Ok(())
    }
    .await;

    if let Err(error) = collected {
        warn!("Failed to enumerate MPRIS players: {}", error);
    }

    if let Some(target) = saved_target {
        if !options
            .iter()
            .any(|(_, value)| value.eq_ignore_ascii_case(target))
        {
            options.push((format!("{} (not running)", target), target.to_string()));
        }
    }

    options
}

pub(super) fn open_settings(
    config: Arc<ConfigHandle<MediaControlConfig>>,
    app_window: Weak<AppWindow>,
) -> anyhow::Result<()> {
    let handle = app_window.unwrap();
    let settings = handle.global::<MediaControlSettings>();

    settings.set_loading(true);
    settings.set_is_dirty(false);
    settings.set_player_labels(ModelRc::new(VecModel::default()));
    settings.set_player_values(ModelRc::new(VecModel::default()));
    settings.set_current_index(0);

    let saved = config.blocking_read().target();
    settings.set_selected_value(saved.clone().unwrap_or_default().into());

    handle
        .global::<Router>()
        .set_settings_page("media_control".into());

    std::thread::spawn(move || {
        let runtime = match tokio::runtime::Runtime::new() {
            Ok(runtime) => runtime,
            Err(error) => {
                warn!("Failed to start runtime for player enumeration: {}", error);
                return;
            }
        };

        let options = runtime.block_on(enumerate_players(saved.as_deref()));

        let _ = app_window.upgrade_in_event_loop(move |handle| {
            let settings = handle.global::<MediaControlSettings>();

            let mut labels: Vec<SharedString> = vec!["Auto (active player)".into()];
            let mut values: Vec<SharedString> = vec![SharedString::new()];

            for (label, value) in options {
                labels.push(label.into());
                values.push(value.into());
            }

            let current_index = match &saved {
                Some(target) => values
                    .iter()
                    .position(|value| value.eq_ignore_ascii_case(target))
                    .unwrap_or(0),
                None => 0,
            } as i32;

            settings.set_player_labels(ModelRc::new(VecModel::from(labels)));
            settings.set_player_values(ModelRc::new(VecModel::from(values)));
            settings.set_current_index(current_index);
            settings.set_loading(false);
        });
    });

    Ok(())
}

pub(super) fn register_settings_callbacks(
    config: Arc<ConfigHandle<MediaControlConfig>>,
    app_window: &AppWindow,
) -> anyhow::Result<()> {
    let settings = app_window.global::<MediaControlSettings>();

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
        let config = config.clone();

        move || {
            let settings = app_window.unwrap();
            store_settings(&config, &settings.global::<MediaControlSettings>());
        }
    });

    settings.on_okay({
        let app_window = app_window.as_weak();

        move || {
            let app_window = app_window.unwrap();
            let settings = app_window.global::<MediaControlSettings>();

            if settings.get_is_dirty() {
                store_settings(&config, &settings);
            }

            app_window.global::<Router>().set_settings_page("".into());
        }
    });

    settings.on_test({
        let app_window = app_window.as_weak();

        move || {
            let handle = app_window.unwrap();
            let settings = handle.global::<MediaControlSettings>();
            let value = settings.get_selected_value().to_string();
            let target = if value.is_empty() { None } else { Some(value) };

            settings.set_test_running(true);
            settings.set_test_status("".into());

            let app_window = app_window.clone();

            std::thread::spawn(move || {
                let message = match tokio::runtime::Runtime::new() {
                    Ok(runtime) => runtime.block_on(test_play_pause(target)),
                    Err(error) => format!("Failed: {}", error),
                };

                let _ = app_window.upgrade_in_event_loop(move |handle| {
                    let settings = handle.global::<MediaControlSettings>();
                    settings.set_test_running(false);
                    settings.set_test_status(message.into());
                });
            });
        }
    });

    Ok(())
}

fn store_settings(config: &ConfigHandle<MediaControlConfig>, settings: &MediaControlSettings) {
    let value = settings.get_selected_value().to_string();
    let target = if value.is_empty() { None } else { Some(value) };
    let _ = config.blocking_update(|config| config.set_target(target));
    settings.set_is_dirty(false);
}
