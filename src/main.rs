#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

use crate::background::BackgroundTasks;
use crate::config::RootConfig;
use crate::plugins::get_plugins;
use crate::ui::run_ui;
use crate::utils::config::ConfigManager;
use anyhow::Context;
use directories::BaseDirs;
use flexi_logger::{Cleanup, Criterion, Duplicate, FileSpec, Logger, Naming};
use log::info;
use tokio::sync::mpsc;

mod background;
mod config;
mod osc_query;
mod platform;
mod plugins;
mod tasks;
mod ui;
mod utils;

slint::include_modules!();

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() -> anyhow::Result<()> {
    let base_dirs = BaseDirs::new().context("Base directories not available")?;
    let config_dir = base_dirs.config_dir().join("vrc-osc-manager");
    let data_dir = base_dirs.data_dir().join("vrc-osc-manager");
    let logs_dir = data_dir.join("logs");

    Logger::try_with_env_or_str("error, vrc_osc_manager=info")?
        .log_to_file(FileSpec::default().directory(logs_dir.clone()))
        .duplicate_to_stdout(Duplicate::All)
        .set_palette("b1;3;2;4;6".into())
        .rotate(
            Criterion::Size(1024 * 1024),
            Naming::Timestamps,
            Cleanup::KeepLogFiles(5),
        )
        .start()?;

    info!("Starting VRC OSC Manager v{}", VERSION);

    let (config_writer_tx, config_writer_rx) = mpsc::channel(8);
    let config_manager = ConfigManager::new(config_dir, config_writer_tx);
    let root_config = config_manager.load_config::<RootConfig>(None, None);
    let plugins = get_plugins(config_manager);
    let enabled_plugins = root_config.blocking_read().enabled_plugins.clone();

    let app_window = AppWindow::new()?;
    let (ui_event_tx, ui_event_rx) = mpsc::channel(8);
    let background_tasks = BackgroundTasks::new(
        root_config.clone(),
        plugins.clone(),
        config_writer_rx,
        logs_dir,
        ui_event_rx,
        app_window.as_weak(),
    )?;

    for plugin in plugins.values() {
        plugin.clone().register_settings_callbacks(&app_window)?
    }

    run_ui(
        app_window,
        plugins,
        enabled_plugins,
        ui_event_tx,
        root_config,
    )?;
    background_tasks.shutdown();

    Ok(())
}
