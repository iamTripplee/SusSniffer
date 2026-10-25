//! The window process. It exits when the window closes, which is the only
//! way to get WebView2 fully out of memory.

use serde_json::Value;
use tauri::window::{Effect, EffectsBuilder};
use tauri::{State, WebviewUrl, WebviewWindowBuilder};

use crate::ipc;

struct Conn {
    port: u16,
    token: String,
}

#[tauri::command]
async fn core(conn: State<'_, Conn>, cmd: String, pid: Option<u32>) -> Result<Value, String> {
    let (port, token) = (conn.port, conn.token.clone());
    tauri::async_runtime::spawn_blocking(move || ipc::call(port, &token, &cmd, pid))
        .await
        .map_err(|e| e.to_string())?
}

pub fn run(ctx: tauri::Context, port: u16, token: String) {
    tauri::Builder::default()
        .manage(Conn { port, token })
        .invoke_handler(tauri::generate_handler![core])
        .setup(|app| {
            WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title("SusSniffer")
                .inner_size(760.0, 540.0)
                .min_inner_size(560.0, 380.0)
                .decorations(false)
                .transparent(true)
                .effects(EffectsBuilder::new().effect(Effect::Mica).build())
                .build()?;
            Ok(())
        })
        .run(ctx)
        .expect("failed to run window");
}
