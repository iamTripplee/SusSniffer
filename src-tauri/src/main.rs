// Без консольного окна в релизной сборке.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod ipc;
mod rules;
mod ui;
mod watcher;
mod win;

use std::io::Read;
use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, RunEvent};
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_opener::OpenerExt;

use watcher::{Proc, Shared};

pub const TOKEN_ENV: &str = "SUSSNIFFER_TOKEN";

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

/// Процесс окна: запускается по клику, живёт, пока открыто окно.
struct UiLauncher {
    port: u16,
    token: String,
    child: Mutex<Option<Child>>,
    /// Просьба к уже открытому окну выйти на передний план; окно читает её
    /// вместе со следующим снимком.
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

    fn close(&self) {
        if let Some(mut c) = self.child.lock().unwrap().take() {
            let _ = c.kill();
        }
    }
}

fn run_core(ctx: tauri::Context, show_window: bool) {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .setup(move |app| {
            let shared = Shared::new(app.path().app_data_dir()?);

            let handle = app.handle().clone();
            watcher::spawn(shared.clone(), move |found| notify(&handle, &found));

            let token = ipc::random_token();
            let focus = Arc::new(AtomicBool::new(false));
            let handler = {
                let (shared, handle, focus) = (shared.clone(), app.handle().clone(), focus.clone());
                Arc::new(move |cmd: &str, pid: Option<u32>| handle_cmd(&shared, &handle, &focus, cmd, pid))
            };
            let port = ipc::serve(token.clone(), handler)?;
            app.manage(Arc::new(UiLauncher { port, token, child: Mutex::new(None), focus }));

            let open = MenuItem::with_id(app, "open", "Открыть SusSniffer", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Выход", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open, &quit])?;
            TrayIconBuilder::with_id("tray")
                .icon(app.default_window_icon().cloned().expect("app icon"))
                .tooltip("SusSniffer")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, e| match e.id.as_ref() {
                    "open" => open_ui(app),
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

            if show_window {
                open_ui(app.handle());
            }
            Ok(())
        })
        .build(ctx)
        .expect("failed to build app");

    app.run(|_, event| {
        // У ядра нет окон, и завершаться оно должно только из меню трея.
        if let RunEvent::ExitRequested { api, code: None, .. } = event {
            api.prevent_exit();
        }
    });
}

fn open_ui(app: &AppHandle) {
    app.state::<Arc<UiLauncher>>().open();
}

fn handle_cmd(shared: &Shared, app: &AppHandle, focus: &AtomicBool, cmd: &str, pid: Option<u32>) -> Value {
    let path = || pid.and_then(|p| shared.procs.lock().unwrap().get(&p).and_then(|x| x.path.clone()));
    match cmd {
        "snapshot" => {
            *shared.ui_seen.lock().unwrap() = Some(Instant::now());
            let mut procs: Vec<Proc> = shared.procs.lock().unwrap().values().cloned().collect();
            procs.sort_by(|a, b| b.score.cmp(&a.score).then(b.cpu.total_cmp(&a.cpu)));
            json!({ "procs": procs, "totals": watcher::totals(shared), "focus": focus.swap(false, Ordering::Relaxed) })
        }
        "kill" => json!(pid.is_some_and(win::kill)),
        "trust" => match path() {
            Some(p) => {
                shared.trust(&p);
                json!(true)
            }
            None => json!(false),
        },
        "reveal" => json!(path().is_some_and(|p| Command::new("explorer").arg(format!("/select,{p}")).spawn().is_ok())),
        "virustotal" => match path().ok_or_else(|| "Не удалось узнать путь к файлу".to_string()).and_then(|p| sha256_file(&p)) {
            Ok(hex) => match app.opener().open_url(format!("https://www.virustotal.com/gui/file/{hex}"), None::<&str>) {
                Ok(()) => json!(true),
                Err(e) => json!({ "error": e.to_string() }),
            },
            Err(e) => json!({ "error": e }),
        },
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
            "Подозрительный процесс".to_string(),
            format!(
                "{} — риск {} из 100. {}",
                p.name,
                p.score,
                p.reasons.first().map(|r| r.title.as_str()).unwrap_or("")
            ),
        ),
        many => (
            format!("Подозрительных процессов: {}", many.len()),
            many.iter().map(|p| p.name.as_str()).collect::<Vec<_>>().join(", "),
        ),
    };
    let _ = app.notification().builder().title(title).body(body).show();
}
