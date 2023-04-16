use anyhow::Result;
use async_osc::OscSocket;
use chrono::{Local, Timelike};
use image::Rgba;
use ksni::{Icon, MenuItem};
use log::{debug, info};
use std::time::Duration;
use sysinfo::{ProcessRefreshKind, RefreshKind, System, SystemExt};
use tokio::sync::broadcast;
use tokio::sync::broadcast::{Receiver, Sender};
use tokio::time::sleep;
use tokio::{select, spawn};

fn convert(img: &[u8]) -> Result<Vec<u8>> {
    let img = image::load_from_memory(img)?;
    let mut img = img.to_rgba8();

    for Rgba(pixel) in img.pixels_mut() {
        *pixel = u32::from_be_bytes(*pixel).rotate_right(8).to_be_bytes();
    }

    Ok(img.into_raw())
}

const STANDARD_ICON: &[u8] = include_bytes!("../assets/icon.png");
const ACTIVE_ICON: &[u8] = include_bytes!("../assets/icon-active.png");

struct ApplicationTray {
    running: bool,
}

impl ksni::Tray for ApplicationTray {
    fn id(&self) -> String {
        "osx-watch".to_string()
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        vec![Icon {
            width: 64,
            height: 64,
            data: convert(if self.running {
                ACTIVE_ICON
            } else {
                STANDARD_ICON
            })
            .unwrap(),
        }]
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        use ksni::menu::*;

        vec![StandardItem {
            label: "Exit".into(),
            activate: Box::new(|_| std::process::exit(0)),
            ..Default::default()
        }
        .into()]
    }
}

async fn vrchat_check_loop(tx: Sender<bool>) -> Result<()> {
    let mut vrchat_running = false;
    let mut sys = System::new();
    let refresh_kind = RefreshKind::new().with_processes(ProcessRefreshKind::new());

    loop {
        debug!("Checking if VRChat is running");
        sys.refresh_specifics(refresh_kind);

        {
            let mut processes = sys.processes_by_name("VRChat");
            let running = processes.next().is_some();

            if running != vrchat_running {
                vrchat_running = running;
                tx.send(vrchat_running)?;

                info!(
                    "VRChat {}",
                    if vrchat_running {
                        "is running"
                    } else {
                        "has stopped"
                    }
                );
            }
        }

        sleep(Duration::from_secs(20)).await;
    }
}

async fn time_sender_loop(mut rx: Receiver<bool>) -> Result<()> {
    let socket = OscSocket::bind("127.0.0.1:0").await?;
    socket.connect("127.0.0.1:9000").await?;

    loop {
        let now = Local::now();
        let hour = ((now.hour() % 12) as f32 + now.minute() as f32 / 60.) / 6. - 1.;
        let minute = (now.minute() as f32 + now.second() as f32 / 60.) / 30. - 1.;

        let _ = socket
            .send(("/avatar/parameters/RMBA_WatchHours", (hour,)))
            .await;
        let _ = socket
            .send(("/avatar/parameters/RMBA_WatchMinutes", (minute,)))
            .await;
        debug!("Sent new time");

        select! {
            vrchat_running = rx.recv() => {
                if !vrchat_running? {
                    debug!("Stopping time sender");
                    return Ok(());
                }
            },
            _ = sleep(Duration::from_secs(10)) => {
                continue;
            },
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    let service = ksni::TrayService::new(ApplicationTray { running: false });
    let handle = service.handle();
    service.spawn();

    let (tx, mut rx) = broadcast::channel(2);
    let check_tx = tx.clone();

    spawn(async move {
        vrchat_check_loop(check_tx).await.unwrap();
    });

    loop {
        let vrchat_running = rx.recv().await?;

        if vrchat_running {
            debug!("Starting time sender");
            let rx = tx.subscribe();

            handle.update(|tray: &mut ApplicationTray| {
                tray.running = true;
            });

            spawn(async move {
                time_sender_loop(rx).await.unwrap();
            });
        } else {
            handle.update(|tray: &mut ApplicationTray| {
                tray.running = false;
            });
        }
    }
}
