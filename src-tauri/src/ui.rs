//! The window process. It exits when the window closes, which is the only way
//! to get WebView2 fully out of memory. Hardware counters and startup scans
//! also live here so the core never loads PDH, DXGI or COM.

use std::collections::HashMap;
use std::mem::size_of;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use serde_json::{json, Value};
use tauri::window::{Effect, EffectsBuilder};
use tauri::{State, WebviewUrl, WebviewWindowBuilder};
use windows::core::BOOL;
use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetShellWindow, GetWindow, GetWindowLongW, GetWindowThreadProcessId, IsHungAppWindow,
    IsWindowVisible, GWL_EXSTYLE, GW_OWNER, WS_EX_APPWINDOW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
};

use crate::hw::{self, Hardware};
use crate::{ipc, startup, win};

struct Conn {
    port: u16,
    token: String,
}

type Hw = Arc<Mutex<Hardware>>;

static HW_STATIC: OnceLock<hw::Static> = OnceLock::new();
static APP_NAMES: OnceLock<Mutex<HashMap<String, Option<String>>>> = OnceLock::new();

fn windowed_apps() -> HashMap<u32, bool> {
    unsafe extern "system" fn collect(hwnd: HWND, arg: LPARAM) -> BOOL {
        unsafe {
            if !IsWindowVisible(hwnd).as_bool() || hwnd == GetShellWindow() {
                return true.into();
            }
            let style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
            let app_window = style & WS_EX_APPWINDOW.0 != 0;
            if !app_window && (style & (WS_EX_TOOLWINDOW.0 | WS_EX_NOACTIVATE.0) != 0
                || GetWindow(hwnd, GW_OWNER).is_ok())
            {
                return true.into();
            }
            let mut cloaked = 0u32;
            let _ = DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, (&mut cloaked as *mut u32).cast(), size_of::<u32>() as u32);
            if cloaked == 0 {
                let mut pid = 0;
                GetWindowThreadProcessId(hwnd, Some(&mut pid));
                if pid != 0 {
                    let apps = &mut *(arg.0 as *mut HashMap<u32, bool>);
                    let unresponsive = apps.entry(pid).or_default();
                    *unresponsive = *unresponsive || IsHungAppWindow(hwnd).as_bool();
                }
            }
        }
        true.into()
    }

    let mut apps = HashMap::new();
    unsafe {
        let _ = EnumWindows(Some(collect), LPARAM(&mut apps as *mut HashMap<u32, bool> as isize));
    }
    apps
}

fn add_display_name(row: &mut Value, names: &mut HashMap<String, Option<String>>) {
    let Some(path) = row["path"].as_str().filter(|path| !path.starts_with(r"\")) else {
        return;
    };
    if !names.contains_key(path) && names.len() >= 512 {
        names.clear();
    }
    let name = names.entry(path.to_owned()).or_insert_with(|| {
        win::file_description(path).map(|name| name.chars().take(120).collect())
    });
    if let Some(name) = name {
        row["display_name"] = json!(name);
    }
}

#[tauri::command]
async fn core(conn: State<'_, Conn>, cmd: String, pid: Option<u32>, arg: Option<String>) -> Result<Value, String> {
    let (port, token) = (conn.port, conn.token.clone());
    tauri::async_runtime::spawn_blocking(move || {
        let show_apps = cmd == "snapshot" && arg.as_deref() == Some("apps");
        let mut reply = ipc::call(port, &token, &cmd, pid, arg)?;
        if show_apps {
            let apps = windowed_apps();
            let mut names = APP_NAMES.get_or_init(Mutex::default).lock().unwrap();
            if let Some(rows) = reply["procs"].as_array_mut() {
                for row in rows {
                    if let Some(unresponsive) = row["pid"].as_u64().and_then(|pid| apps.get(&(pid as u32))) {
                        row["is_app"] = json!(true);
                        row["not_responding"] = json!(unresponsive);
                        add_display_name(row, &mut names);
                    }
                }
            }
        } else if cmd == "recent" && let Some(rows) = reply.as_array_mut() {
            let mut names = APP_NAMES.get_or_init(Mutex::default).lock().unwrap();
            for row in rows {
                add_display_name(row, &mut names);
            }
        }
        Ok(reply)
    })
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn hardware(hw: State<'_, Hw>) -> Result<Value, String> {
    let hw = hw.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let info = HW_STATIC.get_or_init(hw::static_info);
        let live = hw.lock().unwrap().live.clone();
        let (mem_total, mem_avail) = win::memory_status();
        let disks: Vec<Value> = win::fixed_disks()
            .into_iter()
            .map(|d| json!({ "root": d.root, "label": d.label, "total": d.total, "free": d.free }))
            .collect();
        json!({ "info": info, "live": live, "mem_total": mem_total, "mem_used": mem_total.saturating_sub(mem_avail), "disks": disks })
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn startup_list() -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(|| json!(startup::list())).await.map_err(|e| e.to_string())
}

#[tauri::command]
fn startup_set(id: String, enabled: bool) -> Result<(), String> {
    startup::set_enabled(&id, enabled)
}

#[tauri::command]
async fn reveal_path(path: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || win::reveal_path(&path)).await.map_err(|e| e.to_string())?
}

#[tauri::command]
async fn app_icon(path: String) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || crate::app_icon::from_exe(&path)).await.map_err(|e| e.to_string())
}

pub fn run(ctx: tauri::Context, port: u16, token: String) {
    let hw: Hw = Arc::default();
    {
        let hw = hw.clone();
        std::thread::spawn(move || loop {
            hw.lock().unwrap().sample();
            std::thread::sleep(Duration::from_secs(2));
        });
    }
    tauri::Builder::default()
        .manage(Conn { port, token })
        .manage(hw)
        .invoke_handler(tauri::generate_handler![core, hardware, startup_list, startup_set, reveal_path, app_icon])
        .setup(|app| {
            WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title("WinGlance")
                .inner_size(880.0, 640.0)
                .min_inner_size(640.0, 460.0)
                .decorations(false)
                .transparent(true)
                .effects(EffectsBuilder::new().effect(Effect::Mica).build())
                .build()?;
            Ok(())
        })
        .run(ctx)
        .expect("failed to run window");
}
