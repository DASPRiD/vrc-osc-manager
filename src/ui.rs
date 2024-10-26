use crate::config::{DarkLight, RootConfig};
use crate::platform::{get_platform, Platform};
use crate::plugins::Plugin;
use crate::tasks::orchestrate::UiEvent;
use crate::utils::config::ConfigHandle;
use crate::{AppWindow, PluginItem, PluginItems, Settings};
use log::error;
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;

pub fn run_ui(
    app_window: AppWindow,
    plugins: HashMap<&'static str, Arc<dyn Plugin>>,
    enabled_plugins: HashSet<String>,
    ui_event_tx: mpsc::Sender<UiEvent>,
    config: ConfigHandle<RootConfig>,
) -> anyhow::Result<()> {
    let plugin_items = create_plugin_items(&plugins, &enabled_plugins);
    let model = ModelRc::new(VecModel::from(plugin_items));
    app_window.global::<PluginItems>().set_items(model.clone());

    app_window.global::<PluginItems>().on_toggle_enabled({
        let model = model.clone();
        let ui_event_tx = ui_event_tx.clone();

        move |plugin_id, enabled| {
            if let Some(index) = model.iter().position(|item| item.id == plugin_id) {
                let mut row_data = model.row_data(index).unwrap();
                row_data.enabled = enabled;
                model.set_row_data(index, row_data);
            }

            ui_event_tx
                .blocking_send(UiEvent::PluginToggle(plugin_id.into(), enabled))
                .unwrap();
        }
    });

    app_window.global::<PluginItems>().on_open_url(|url| {
        let _ = open::that(<&str as Into<PathBuf>>::into(url.as_str()));
    });

    app_window.global::<PluginItems>().on_open_settings({
        let app_window = app_window.as_weak();

        move |plugin_id| {
            let plugin = match plugins.get(plugin_id.as_str()) {
                Some(plugin) => plugin,
                None => {
                    error!("Unknown plugin requested: {}", plugin_id);
                    return;
                }
            };

            plugin.clone().open_settings(app_window.clone()).unwrap();
        }
    });

    let settings = app_window.global::<Settings>();

    settings.set_tray_icons(match config.blocking_read().dark_light {
        DarkLight::Default => "Auto detect".into(),
        DarkLight::Dark => "Dark".into(),
        DarkLight::Light => "Light".into(),
    });

    settings.set_auto_start(get_platform().has_auto_start());

    settings.on_toggle_tray_icons({
        let ui_event_tx = ui_event_tx.clone();

        move |mode| {
            let mode = match mode.as_str() {
                "Auto detect" => DarkLight::Default,
                "Dark" => DarkLight::Dark,
                "Light" => DarkLight::Light,
                _ => return,
            };

            ui_event_tx
                .blocking_send(UiEvent::TrayIconsToggle(mode))
                .unwrap();
        }
    });

    settings.on_toggle_auto_start({
        let ui_event_tx = ui_event_tx.clone();

        move |enabled| {
            ui_event_tx
                .blocking_send(UiEvent::AutoStartToggle(enabled))
                .unwrap();
        }
    });

    settings.on_open_logs_folder({
        let ui_event_tx = ui_event_tx.clone();

        move || {
            ui_event_tx.blocking_send(UiEvent::OpenLogsFolder).unwrap();
        }
    });

    settings.on_start_plugins({
        let ui_event_tx = ui_event_tx.clone();

        move || {
            ui_event_tx.blocking_send(UiEvent::StartPlugins).unwrap();
        }
    });

    slint::run_event_loop_until_quit()?;
    Ok(())
}

fn create_plugin_items(
    plugins: &HashMap<&'static str, Arc<dyn Plugin>>,
    enabled_plugins: &HashSet<String>,
) -> Vec<PluginItem> {
    let mut items = plugins
        .iter()
        .map(|(id, plugin)| PluginItem {
            id: id.to_string().into(),
            title: plugin.title().into(),
            description: plugin.description().into(),
            enabled: enabled_plugins.contains(&id.to_string()),
            has_settings: plugin.has_settings(),
            info_url: plugin.info_url().unwrap_or("").into(),
        })
        .collect::<Vec<_>>();

    items.sort_by(|a, b| a.title.cmp(&b.title));
    items
}
