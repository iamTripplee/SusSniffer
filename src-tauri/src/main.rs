#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod hw;
mod ipc;
mod rules;
mod startup;
mod ui;
mod updater;
mod watcher;
mod win;

use std::io::Read;
use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, RunEvent};
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_opener::OpenerExt;

use watcher::{Proc, Shared};

pub const TOKEN_ENV: &str = "WINGLANCE_TOKEN";
/// Value name under HKCU\...\Run; the uninstaller removes it too.
const AUTOSTART_NAME: &str = "WinGlance";

fn main() {
    let ctx = tauri::generate_context!();
    let args: Vec<String> = std::env::args().collect();
    match args.iter().position(|a| a == "--ui") {
        Some(i) => {
            let port = args.get(i + 1).and_then(|p| p.parse().ok()).unwrap_or(0);
            let token = std::env::var(TOKEN_ENV).unwrap_or_default();
            ui::run(ctx, port, token);
        }
        None => run_core(ctx, !args.iter().any(|a| a == "--minimized")),
    }
}

pub(crate) struct UiLauncher {
    port: u16,
    token: String,
    child: Mutex<Option<Child>>,
    /// Asks an already open window to come to the front on its next poll.
    focus: Arc<AtomicBool>,
}

impl UiLauncher {
    fn open(&self) {
        let mut child = self.child.lock().unwrap();
        if child.as_mut().is_some_and(|c| matches!(c.try_wait(), Ok(None))) {
            self.focus.store(true, Ordering::Relaxed);
            return;
        }
        let Ok(exe) = std::env::current_exe() else { return };
        *child = Command::new(exe)
            .arg("--ui")
            .arg(self.port.to_string())
            .env(TOKEN_ENV, &self.token)
            .spawn()
            .ok();
    }

    pub(crate) fn close(&self) {
        if let Some(mut c) = self.child.lock().unwrap().take() {
            let _ = c.kill();
        }
    }
}

fn run_core(ctx: tauri::Context, show_window: bool) {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| open_ui(app)))
        .plugin(tauri_plugin_autostart::Builder::new().app_name(AUTOSTART_NAME).args(["--minimized"]).build())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(move |app| {
            let data_dir = app.path().app_data_dir()?;
            let autostart_on = setup_autostart(app.handle(), &data_dir);
            let shared = Shared::new(data_dir.clone());

            let handle = app.handle().clone();
            watcher::spawn(shared.clone(), move |found| notify(&handle, &found));

            let token = ipc::random_token();
            let focus = Arc::new(AtomicBool::new(false));
            let handler = {
                let (shared, handle, focus) = (shared.clone(), app.handle().clone(), focus.clone());
                Arc::new(move |cmd: &str, pid: Option<u32>, arg: Option<&str>| handle_cmd(&shared, &handle, &focus, cmd, pid, arg))
            };
            let port = ipc::serve(token.clone(), handler)?;
            app.manage(Arc::new(UiLauncher { port, token, child: Mutex::new(None), focus }));

            let open = MenuItem::with_id(app, "open", "Open WinGlance", true, None::<&str>)?;
            let autostart =
                CheckMenuItem::with_id(app, "autostart", "Start with Windows", true, autostart_on, None::<&str>)?;
            let update = MenuItem::with_id(app, "update", "Check for updates", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let sep = PredefinedMenuItem::separator(app)?;
            let menu = Menu::with_items(app, &[&open, &autostart, &update, &sep, &quit])?;
            app.manage(updater::Updates::new(update, app.package_info().version.to_string(), &data_dir));
            TrayIconBuilder::with_id("tray")
                .icon(tauri::image::Image::from_bytes(include_bytes!("../icons/tray.png"))?)
                .tooltip(format!("WinGlance {}", app.package_info().version))
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(move |app, e| match e.id.as_ref() {
                    "open" => open_ui(app),
                    "update" => updater::act(app, updater::Source::Tray),
                    "autostart" => {
                        let on = autostart.is_checked().unwrap_or(false);
                        let res = if on { app.autolaunch().enable() } else { app.autolaunch().disable() };
                        let actual = app.autolaunch().is_enabled().unwrap_or(false);
                        if res.is_err() || actual != on {
                            let _ = autostart.set_checked(actual);
                        }
                    }
                    "quit" => {
                        app.state::<Arc<UiLauncher>>().close();
                        app.exit(0);
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, e| {
                    if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
                        open_ui(tray.app_handle());
                    }
                })
                .build(app)?;

            updater::spawn(app.handle().clone());
            if show_window || app.state::<updater::Updates>().take_reopen() {
                open_ui(app.handle());
            }
            Ok(())
        })
        .build(ctx)
        .expect("failed to build app");

    app.run(|_, event| {
        // The core has no windows; it only quits from the tray menu.
        if let RunEvent::ExitRequested { api, code: None, .. } = event {
            api.prevent_exit();
        }
    });
}

fn open_ui(app: &AppHandle) {
    app.state::<Arc<UiLauncher>>().open();
}

/// On by default on first run. Later runs keep the user's choice but re-register
/// so the entry points at the current exe after a reinstall.
fn setup_autostart(app: &AppHandle, data_dir: &std::path::Path) -> bool {
    let marker = data_dir.join("first-run-done");
    let launcher = app.autolaunch();
    let want = if marker.exists() { launcher.is_enabled().unwrap_or(false) } else { true };
    if want {
        let _ = launcher.enable();
    }
    let _ = std::fs::create_dir_all(data_dir);
    let _ = std::fs::write(&marker, b"");
    launcher.is_enabled().unwrap_or(false)
}

fn handle_cmd(shared: &Shared, app: &AppHandle, focus: &AtomicBool, cmd: &str, pid: Option<u32>, arg: Option<&str>) -> Value {
    let path = || pid.and_then(|p| shared.procs.lock().unwrap().get(&p).and_then(|x| x.path.clone()));
    match cmd {
        "snapshot" => {
            let show_recent = arg == Some("recent");
            *shared.ui_seen.lock().unwrap() = (!show_recent).then(Instant::now);
            let mut procs: Vec<Proc> = if show_recent {
                Vec::new()
            } else {
                shared.procs.lock().unwrap().values().cloned().collect()
            };
            procs.sort_by(|a, b| b.score.cmp(&a.score).then(b.cpu.total_cmp(&a.cpu)));
            let update = app.try_state::<updater::Updates>().map(|u| u.status());
            json!({ "procs": procs, "totals": watcher::totals(shared), "focus": focus.swap(false, Ordering::Relaxed), "update": update })
        }
        "recent" => json!(shared.recent(arg.and_then(|value| value.parse().ok()))),
        "kill" => json!(pid.is_some_and(win::kill)),
        "priority" => {
            let Some(pid) = pid.filter(|pid| shared.procs.lock().unwrap().contains_key(pid)) else {
                return json!({ "error": "The process is no longer listed." });
            };
            match arg {
                Some(name) => match win::set_priority(pid, name) {
                    Ok(()) => json!(true),
                    Err(error) => json!({ "error": error }),
                },
                None => {
                    let (current, can_set, can_end) = win::process_controls(pid);
                    json!({ "current": current, "can_set": can_set, "can_end": can_end })
                }
            }
        }
        "trust" => match path() {
            Some(p) => {
                shared.trust(&p);
                json!(true)
            }
            None => json!(false),
        },
        "reveal" => match path().ok_or_else(|| "The file path is unavailable.".to_string()).and_then(|p| win::reveal_path(&p)) {
            Ok(()) => json!(true),
            Err(error) => json!({ "error": error }),
        },
        "virustotal" => match path().ok_or_else(|| "Can't resolve the file path".to_string()).and_then(|p| sha256_file(&p)) {
            Ok(hex) => match app.opener().open_url(format!("https://www.virustotal.com/gui/file/{hex}"), None::<&str>) {
                Ok(()) => json!(true),
                Err(e) => json!({ "error": e.to_string() }),
            },
            Err(e) => json!({ "error": e }),
        },
        "update" => {
            updater::act(app, updater::Source::Window);
            json!(true)
        }
        _ => json!({ "error": "unknown command" }),
    }
}

fn sha256_file(path: &str) -> Result<String, String> {
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

fn notify(app: &AppHandle, found: &[Proc]) {
    let (title, body) = match found {
        [p] => (
            "Suspicious process".to_string(),
            format!(
                "{} — risk {}/100. {}",
                p.name,
                p.score,
                p.reasons.first().map(|r| r.title.as_str()).unwrap_or("")
            ),
        ),
        many => (
            format!("{} suspicious processes", many.len()),
            many.iter().map(|p| p.name.as_str()).collect::<Vec<_>>().join(", "),
        ),
    };
    let _ = app.notification().builder().title(title).body(body).show();
}

