//! The window process. It exits when the window closes, which is the only way
//! to get WebView2 fully out of memory. Hardware counters and startup scans
//! also live here so the core never loads PDH, DXGI or COM.

use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use serde_json::{json, Value};
use tauri::window::{Effect, EffectsBuilder};
use tauri::{State, WebviewUrl, WebviewWindowBuilder};

use crate::hw::{self, Hardware};
use crate::{ipc, startup, win};

struct Conn {
    port: u16,
    token: String,
}

type Hw = Arc<Mutex<Hardware>>;

static HW_STATIC: OnceLock<hw::Static> = OnceLock::new();

#[tauri::command]
async fn core(conn: State<'_, Conn>, cmd: String, pid: Option<u32>, arg: Option<String>) -> Result<Value, String> {
    let (port, token) = (conn.port, conn.token.clone());
    tauri::async_runtime::spawn_blocking(move || ipc::call(port, &token, &cmd, pid, arg))
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
        .invoke_handler(tauri::generate_handler![core, hardware, startup_list, startup_set])
        .setup(|app| {
            WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title("SusSniffer")
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
