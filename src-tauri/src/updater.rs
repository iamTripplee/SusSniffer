//! Update check against the latest GitHub release. Downloads are verified
//! against the public key in tauri.conf.json before anything is installed.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::menu::MenuItem;
use tauri::{AppHandle, Manager, Wry};
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_updater::UpdaterExt;

use crate::win;

const FIRST_CHECK: Duration = Duration::from_secs(90);
const INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Idle,
    Checking,
    UpToDate,
    Available,
    Downloading,
    Failed,
}

#[derive(Clone, Serialize)]
pub struct Status {
    pub current: String,
    pub available: Option<String>,
    pub phase: Phase,
    pub error: Option<String>,
}

pub struct Updates {
    item: MenuItem<Wry>,
    status: Mutex<Status>,
    /// Present while an update started from the window is installing, so the
    /// relaunched core reopens the window.
    reopen_flag: PathBuf,
}

impl Updates {
    pub fn new(item: MenuItem<Wry>, current: String, data_dir: &std::path::Path) -> Self {
        let status = Status { current, available: None, phase: Phase::Idle, error: None };
        Self { item, status: Mutex::new(status), reopen_flag: data_dir.join("reopen-after-update") }
    }

    pub fn status(&self) -> Status {
        self.status.lock().unwrap().clone()
    }

    fn set(&self, phase: Phase, available: Option<String>, error: Option<String>) {
        let mut s = self.status.lock().unwrap();
        s.phase = phase;
        if available.is_some() {
            s.available = available;
        }
        s.error = error;
        let (text, enabled) = match (s.phase, &s.available) {
            (Phase::Downloading, _) => ("Downloading update…".to_string(), false),
            (Phase::Checking, _) => ("Checking for updates…".to_string(), false),
            (_, Some(v)) => (format!("Update to {v}"), true),
            _ => ("Check for updates".to_string(), true),
        };
        let _ = self.item.set_text(text);
        let _ = self.item.set_enabled(enabled);
    }

    /// True once per relaunch after an update started from the window.
    pub fn take_reopen(&self) -> bool {
        std::fs::remove_file(&self.reopen_flag).is_ok()
    }
}

pub fn spawn(app: AppHandle) {
    std::thread::Builder::new()
        .name("updater".into())
        .spawn(move || {
            std::thread::sleep(FIRST_CHECK);
            loop {
                if !win::user_is_busy() {
                    tauri::async_runtime::block_on(check(&app, Source::Background));
                }
                std::thread::sleep(INTERVAL);
            }
        })
        .expect("updater thread");
}

#[derive(Clone, Copy, PartialEq)]
pub enum Source {
    Background,
    Tray,
    Window,
}

/// Install a known update, otherwise check right now.
pub fn act(app: &AppHandle, source: Source) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let updates = app.state::<Updates>();
        let status = updates.status();
        if matches!(status.phase, Phase::Checking | Phase::Downloading) {
            return;
        }
        if status.available.is_some() {
            install(&app, source).await;
        } else {
            check(&app, source).await;
        }
    });
}

async fn check(app: &AppHandle, source: Source) {
    let updates = app.state::<Updates>();
    updates.set(Phase::Checking, None, None);
    let result = match app.updater() {
        Ok(u) => u.check().await,
        Err(e) => Err(e),
    };
    match result {
        Ok(Some(update)) => {
            let first_time = updates.status().available.is_none();
            updates.set(Phase::Available, Some(update.version.clone()), None);
            if first_time && source != Source::Window {
                notify(app, "Update available", &format!("WinGlance {} is out. Open WinGlance to install it.", update.version));
            }
        }
        Ok(None) => {
            updates.set(Phase::UpToDate, None, None);
            if source == Source::Tray {
                notify(app, "You're up to date", &format!("WinGlance {} is the latest version.", app.package_info().version));
            }
        }
        Err(e) => {
            updates.set(Phase::Failed, None, Some(e.to_string()));
            if source == Source::Tray {
                notify(app, "Couldn't check for updates", &e.to_string());
            }
        }
    }
}

async fn install(app: &AppHandle, source: Source) {
    let updates = app.state::<Updates>();
    updates.set(Phase::Downloading, None, None);
    let result = async {
        let update = app.updater()?.check().await?.ok_or(tauri_plugin_updater::Error::ReleaseNotFound)?;
        let bytes = update.download(|_, _| {}, || {}).await?;
        if source == Source::Window {
            let _ = std::fs::write(&updates.reopen_flag, b"");
        }
        // The window process holds the same exe open; close it before the installer runs.
        app.state::<std::sync::Arc<crate::UiLauncher>>().close();
        update.install(bytes)
    }
    .await;
    if let Err(e) = result {
        let _ = std::fs::remove_file(&updates.reopen_flag);
        updates.set(Phase::Failed, None, Some(e.to_string()));
        notify(app, "Update failed", &e.to_string());
    }
}

fn notify(app: &AppHandle, title: &str, body: &str) {
    let _ = app.notification().builder().title(title).body(body).show();
}
