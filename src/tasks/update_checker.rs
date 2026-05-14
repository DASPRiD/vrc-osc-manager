use crate::config::RootConfig;
use crate::tasks::orchestrate::AppEvent;
use crate::utils::config::ConfigHandle;
use log::{debug, warn};
use semver::Version;
use serde::Deserialize;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::sleep;
use tokio_graceful_shutdown::errors::CancelledByShutdown;
use tokio_graceful_shutdown::{FutureExt, IntoSubsystem, SubsystemHandle};

const LATEST_RELEASE_URL: &str =
    "https://api.github.com/repos/DASPRiD/vrc-osc-manager/releases/latest";
const USER_AGENT: &str = concat!("vrc-osc-manager/", env!("CARGO_PKG_VERSION"));
const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
    html_url: String,
}

pub struct UpdateCheckerTask {
    app_event_tx: mpsc::Sender<AppEvent>,
    config: ConfigHandle<RootConfig>,
    current_version: Version,
}

impl UpdateCheckerTask {
    pub fn new(
        app_event_tx: mpsc::Sender<AppEvent>,
        config: ConfigHandle<RootConfig>,
    ) -> anyhow::Result<Self> {
        let current_version = Version::parse(env!("CARGO_PKG_VERSION"))?;

        Ok(Self {
            app_event_tx,
            config,
            current_version,
        })
    }

    async fn check_once(
        &self,
        client: &reqwest::Client,
        last_notified: &mut Option<Version>,
    ) -> anyhow::Result<()> {
        if !self.config.read().await.check_for_updates {
            debug!("Update check disabled by user, skipping");
            return Ok(());
        }

        let release = client
            .get(LATEST_RELEASE_URL)
            .header("Accept", "application/vnd.github+json")
            .send()
            .await?
            .error_for_status()?
            .json::<GithubRelease>()
            .await?;

        let tag = release.tag_name.trim_start_matches('v');
        let latest = Version::parse(tag)?;

        if latest <= self.current_version {
            debug!("Already on latest version: {}", self.current_version);
            return Ok(());
        }

        debug!("New version available: {}", latest);

        self.app_event_tx
            .send(AppEvent::UpdateAvailable {
                version: latest.to_string(),
                url: release.html_url.clone(),
            })
            .await?;

        let should_notify = last_notified.as_ref().is_none_or(|seen| *seen < latest);

        if should_notify {
            notify_update(&latest, &release.html_url);
            *last_notified = Some(latest);
        }

        Ok(())
    }

    async fn main_loop(&self) -> anyhow::Result<()> {
        let client = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(REQUEST_TIMEOUT)
            .build()?;
        let mut last_notified: Option<Version> = None;

        loop {
            if let Err(error) = self.check_once(&client, &mut last_notified).await {
                warn!("Update check failed: {}", error);
            }

            sleep(CHECK_INTERVAL).await;
        }
    }
}

fn notify_update(version: &Version, url: &str) {
    let result = notify_rust::Notification::new()
        .appname("VRC OSC Manager")
        .summary("VRC OSC Manager update available")
        .body(&format!("Version {version} is available.\n{url}"))
        .show();

    if let Err(error) = result {
        warn!("Failed to show update notification: {}", error);
    }
}

impl IntoSubsystem<anyhow::Error> for UpdateCheckerTask {
    async fn run(self, subsys: &mut SubsystemHandle) -> anyhow::Result<()> {
        match self.main_loop().cancel_on_shutdown(subsys).await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => return Err(error),
            Err(CancelledByShutdown) => {}
        }

        Ok(())
    }
}
