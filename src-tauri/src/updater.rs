//! Update check against the latest GitHub release. Downloads are verified
//! against the public key in tauri.conf.json before anything is installed.

use std::sync::Mutex;
use std::time::Duration;

use tauri::menu::MenuItem;
use tauri::{AppHandle, Manager, Wry};
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_updater::UpdaterExt;

use crate::win;

const FIRST_CHECK: Duration = Duration::from_secs(90);
const INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

pub struct Updates {
    pub item: MenuItem<Wry>,
    found: Mutex<Option<String>>,
}

impl Updates {
    pub fn new(item: MenuItem<Wry>) -> Self {
        Self { item, found: Mutex::new(None) }
    }
}

pub fn spawn(app: AppHandle) {
    std::thread::Builder::new()
        .name("updater".into())
        .spawn(move || {
            std::thread::sleep(FIRST_CHECK);
            loop {
                if !win::user_is_busy() {
                    tauri::async_runtime::block_on(check(&app, false));
                }
                std::thread::sleep(INTERVAL);
            }
        })
        .expect("updater thread");
}

/// Tray click: install a known update, otherwise check right now.
pub fn on_click(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let known = app.state::<Updates>().found.lock().unwrap().clone();
        if known.is_some() {
            install(&app).await;
        } else {
            check(&app, true).await;
        }
    });
}

async fn check(app: &AppHandle, manual: bool) {
    let updates = app.state::<Updates>();
    let result = match app.updater() {
        Ok(u) => u.check().await,
        Err(e) => Err(e),
    };
    match result {
        Ok(Some(update)) => {
            let first_time = updates.found.lock().unwrap().replace(update.version.clone()).is_none();
            let _ = updates.item.set_text(format!("Update to {}", update.version));
            if first_time || manual {
                notify(app, "Update available", &format!("WinGlance {} is out. Right-click the tray icon to install it.", update.version));
            }
        }
        Ok(None) if manual => notify(app, "You're up to date", &format!("WinGlance {} is the latest version.", app.package_info().version)),
        Err(e) if manual => notify(app, "Couldn't check for updates", &e.to_string()),
        _ => {}
    }
}

async fn install(app: &AppHandle) {
    let updates = app.state::<Updates>();
    let _ = updates.item.set_text("Downloading update…");
    let _ = updates.item.set_enabled(false);
    let result = async {
        let update = app.updater()?.check().await?.ok_or(tauri_plugin_updater::Error::ReleaseNotFound)?;
        let bytes = update.download(|_, _| {}, || {}).await?;
        // The window process holds the same exe open; close it before the installer runs.
        app.state::<std::sync::Arc<crate::UiLauncher>>().close();
        update.install(bytes)
    }
    .await;
    if let Err(e) = result {
        let _ = updates.item.set_text("Retry update");
        let _ = updates.item.set_enabled(true);
        notify(app, "Update failed", &e.to_string());
    }
}

fn notify(app: &AppHandle, title: &str, body: &str) {
    let _ = app.notification().builder().title(title).body(body).show();
}
