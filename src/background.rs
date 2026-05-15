use crate::config::RootConfig;
use crate::osc_query::node::OscAccess;
use crate::osc_query::service::{OscHostInfo, OscQueryServiceBuilder};
use crate::plugins::{ChannelManager, Plugin};
use crate::tasks::broadcaster::BroadcasterTask;
use crate::tasks::config_writer::{ConfigWriterTask, WriteConfigRequest};
use crate::tasks::orchestrate::{AppEvent, OrchestrateTask, UiEvent};
use crate::tasks::osc_query::OscQueryTask;
use crate::tasks::osc_receiver::OscReceiverTask;
use crate::tasks::osc_sender::OscSenderTask;
use crate::tasks::plugin_manager::PluginManagerTask;
use crate::tasks::tray::TrayTask;
use crate::tasks::update_checker::UpdateCheckerTask;
use crate::tasks::vrchat_monitor::VrchatMonitorTask;
use crate::utils::config::ConfigHandle;
use crate::AppWindow;
use log::error;
use slint::Weak;
use std::collections::HashMap;
use std::net::{TcpListener, UdpSocket};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::runtime::Runtime;
use tokio::sync::{broadcast, mpsc};
use tokio::task::JoinHandle;
use tokio_graceful_shutdown::{IntoSubsystem, SubsystemBuilder, SubsystemHandle, Toplevel};

fn get_available_tcp_port() -> anyhow::Result<u16> {
    let socket = TcpListener::bind("127.0.0.1:0")?;
    Ok(socket.local_addr()?.port())
}

fn get_available_udp_port() -> anyhow::Result<u16> {
    let socket = UdpSocket::bind("127.0.0.1:0")?;
    Ok(socket.local_addr()?.port())
}

pub struct RuntimeParams {
    osc_listener_port: u16,
    osc_query_port: u16,
    config: ConfigHandle<RootConfig>,
    logs_dir: PathBuf,
    plugins: HashMap<&'static str, Arc<dyn Plugin>>,
    config_writer_rx: mpsc::Receiver<WriteConfigRequest>,
    app_window: Weak<AppWindow>,
    ui_event_rx: mpsc::Receiver<UiEvent>,
    app_event_tx: mpsc::Sender<AppEvent>,
    app_event_rx: mpsc::Receiver<AppEvent>,
}

fn start_runtime(params: RuntimeParams) -> anyhow::Result<(Runtime, JoinHandle<()>)> {
    let mut osc_query_service_builder = OscQueryServiceBuilder::new(OscHostInfo::new(
        "VRC OSC Manager".to_string(),
        "127.0.0.1".to_string(),
        params.osc_listener_port,
    ));
    osc_query_service_builder.add_endpoint(
        "/avatar/change".to_string(),
        "s".to_string(),
        OscAccess::Read,
        "".to_string(),
    );

    for plugin in params.plugins.values() {
        plugin.register_osc_parameters(&mut osc_query_service_builder);
    }

    let osc_query_service = osc_query_service_builder.build();

    let runtime = Runtime::new()?;
    let _guard = runtime.enter();

    let join_handle = runtime.spawn(async move {
        let osc_target_port = params.config.read().await.osc.send_port;

        let (plugin_manager_tx, plugin_manager_rx) = mpsc::channel(1);
        let (osc_receiver_tx, _) = broadcast::channel(64);
        let (osc_sender_tx, osc_sender_rx) = mpsc::channel(16);
        let (tray_property_tx, tray_property_rx) = mpsc::channel(1);

        let dark_mode = match dark_light::detect() {
            Ok(dark_light::Mode::Dark | dark_light::Mode::Unspecified) | Err(_) => true,
            Ok(dark_light::Mode::Light) => false,
        };

        let channel_manager = ChannelManager::new(osc_receiver_tx.clone(), osc_sender_tx);

        let orchestrate_task = OrchestrateTask::new(
            params.app_event_rx,
            params.ui_event_rx,
            plugin_manager_tx,
            tray_property_tx,
            params.app_window,
            params.config.clone(),
            params.logs_dir,
        );
        let broadcaster_task =
            BroadcasterTask::new(params.osc_listener_port, params.osc_query_port);
        let config_writer_task = ConfigWriterTask::new(params.config_writer_rx);
        let vrchat_monitor_task = VrchatMonitorTask::new(params.app_event_tx.clone());
        let tray_task = TrayTask::new(tray_property_rx, params.app_event_tx.clone(), dark_mode);
        let osc_query_task = OscQueryTask::new(params.osc_query_port, osc_query_service);
        let osc_receiver_task = OscReceiverTask::new(params.osc_listener_port, osc_receiver_tx);
        let osc_sender_task = OscSenderTask::new(osc_target_port, osc_sender_rx);
        let plugin_manager_task = PluginManagerTask::new(
            plugin_manager_rx,
            params.config.clone(),
            params.plugins,
            channel_manager,
        );
        let update_checker_task =
            match UpdateCheckerTask::new(params.app_event_tx.clone(), params.config) {
                Ok(task) => Some(task),
                Err(error) => {
                    error!("Failed to initialize update checker: {}", error);
                    None
                }
            };

        let result = Toplevel::new(async |s: &mut SubsystemHandle| {
            s.start(SubsystemBuilder::new(
                "Orchestrate",
                orchestrate_task.into_subsystem(),
            ));
            s.start(SubsystemBuilder::new(
                "Broadcaster",
                broadcaster_task.into_subsystem(),
            ));
            s.start(SubsystemBuilder::new("Tray", tray_task.into_subsystem()));
            s.start(SubsystemBuilder::new(
                "ConfigWriter",
                config_writer_task.into_subsystem(),
            ));
            s.start(SubsystemBuilder::new(
                "VrchatMonitor",
                vrchat_monitor_task.into_subsystem(),
            ));
            s.start(SubsystemBuilder::new(
                "OscQuery",
                osc_query_task.into_subsystem(),
            ));
            s.start(SubsystemBuilder::new(
                "OscReceiver",
                osc_receiver_task.into_subsystem(),
            ));
            s.start(SubsystemBuilder::new(
                "OscSender",
                osc_sender_task.into_subsystem(),
            ));
            s.start(SubsystemBuilder::new(
                "PluginManager",
                plugin_manager_task.into_subsystem(),
            ));

            if let Some(task) = update_checker_task {
                s.start(SubsystemBuilder::new(
                    "UpdateChecker",
                    task.into_subsystem(),
                ));
            }
        })
        .handle_shutdown_requests(Duration::from_millis(1000))
        .await;

        if let Err(error) = result {
            slint::quit_event_loop().unwrap();
            error!("Background process crashed: {}", error);
        }
    });

    Ok((runtime, join_handle))
}

pub struct BackgroundTasks {
    runtime: Runtime,
    join_handle: JoinHandle<()>,
    app_event_tx: mpsc::Sender<AppEvent>,
}

impl BackgroundTasks {
    pub fn new(
        config: ConfigHandle<RootConfig>,
        plugins: HashMap<&'static str, Arc<dyn Plugin>>,
        config_writer_rx: mpsc::Receiver<WriteConfigRequest>,
        logs_dir: PathBuf,
        ui_event_rx: mpsc::Receiver<UiEvent>,
        app_window: Weak<AppWindow>,
    ) -> anyhow::Result<Self> {
        let osc_listener_port = get_available_udp_port()?;
        let osc_query_port = get_available_tcp_port()?;
        let (app_event_tx, app_event_rx) = mpsc::channel(8);

        let (runtime, join_handle) = start_runtime(RuntimeParams {
            osc_listener_port,
            osc_query_port,
            config,
            logs_dir,
            plugins,
            config_writer_rx,
            app_window,
            ui_event_rx,
            app_event_tx: app_event_tx.clone(),
            app_event_rx,
        })?;

        Ok(Self {
            runtime,
            join_handle,
            app_event_tx,
        })
    }

    pub fn shutdown(self) {
        let _ = self.app_event_tx.blocking_send(AppEvent::ShutdownRequested);
        self.runtime.block_on(self.join_handle).unwrap();
    }
}
