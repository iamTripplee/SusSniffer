//! Window <-> core link. One JSON line per request and per reply over
//! 127.0.0.1; requests without the session token are dropped.

use std::hash::{BuildHasher, Hasher};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Serialize, Deserialize)]
pub struct Request {
    pub token: String,
    pub cmd: String,
    pub pid: Option<u32>,
}

pub type Handler = dyn Fn(&str, Option<u32>) -> Value + Send + Sync;

pub fn random_token() -> String {
    let mut out = String::new();
    for _ in 0..2 {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos());
        h.write_u32(std::process::id());
        out.push_str(&format!("{:016x}", h.finish()));
    }
    out
}

/// Starts the server and returns its port.
pub fn serve(token: String, handler: Arc<Handler>) -> std::io::Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    std::thread::Builder::new().name("ipc".into()).spawn(move || {
        for stream in listener.incoming().flatten() {
            let _ = handle(stream, &token, &*handler);
        }
    })?;
    Ok(port)
}

fn handle(stream: TcpStream, token: &str, handler: &Handler) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    let mut line = String::new();
    BufReader::new(&stream).take(64 * 1024).read_line(&mut line)?;
    let reply = match serde_json::from_str::<Request>(&line) {
        Ok(req) if req.token == token => handler(&req.cmd, req.pid),
        _ => json!({ "error": "unauthorized" }),
    };
    let mut w = &stream;
    w.write_all(reply.to_string().as_bytes())?;
    w.write_all(b"\n")
}

pub fn call(port: u16, token: &str, cmd: &str, pid: Option<u32>) -> Result<Value, String> {
    let stream = TcpStream::connect(("127.0.0.1", port)).map_err(|e| format!("Core is not running: {e}"))?;
    stream.set_read_timeout(Some(Duration::from_secs(10))).ok();
    let req = Request { token: token.into(), cmd: cmd.into(), pid };
    let mut w = &stream;
    w.write_all(serde_json::to_string(&req).map_err(|e| e.to_string())?.as_bytes()).map_err(|e| e.to_string())?;
    w.write_all(b"\n").map_err(|e| e.to_string())?;
    let mut line = String::new();
    BufReader::new(&stream).read_line(&mut line).map_err(|e| e.to_string())?;
    serde_json::from_str(&line).map_err(|e| e.to_string())
}
