//! Startup entries from Run keys and Startup folders. Enabled state lives in
//! Explorer's StartupApproved keys, the same place Task Manager uses.

use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_SET_VALUE, KEY_WOW64_64KEY};
use winreg::{RegKey, RegValue, HKEY};

use crate::rules::{self, ProcFacts, Reason};
use crate::win;

const APPROVED: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved";

enum Kind {
    Run { hive: HKEY, key: &'static str },
    Folder { env: &'static str, sub: &'static str },
}

struct Source {
    kind: Kind,
    label: &'static str,
    all_users: bool,
    approved_hive: HKEY,
    approved_key: &'static str,
}

fn sources() -> [Source; 5] {
    [
        Source { kind: Kind::Run { hive: HKEY_CURRENT_USER, key: r"Software\Microsoft\Windows\CurrentVersion\Run" }, label: "Run key (user)", all_users: false, approved_hive: HKEY_CURRENT_USER, approved_key: "Run" },
        Source { kind: Kind::Run { hive: HKEY_LOCAL_MACHINE, key: r"Software\Microsoft\Windows\CurrentVersion\Run" }, label: "Run key (all users)", all_users: true, approved_hive: HKEY_LOCAL_MACHINE, approved_key: "Run" },
        Source { kind: Kind::Run { hive: HKEY_LOCAL_MACHINE, key: r"Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Run" }, label: "Run key (all users)", all_users: true, approved_hive: HKEY_LOCAL_MACHINE, approved_key: "Run32" },
        Source { kind: Kind::Folder { env: "APPDATA", sub: r"Microsoft\Windows\Start Menu\Programs\Startup" }, label: "Startup folder", all_users: false, approved_hive: HKEY_CURRENT_USER, approved_key: "StartupFolder" },
        Source { kind: Kind::Folder { env: "ProgramData", sub: r"Microsoft\Windows\Start Menu\Programs\StartUp" }, label: "Startup folder (all users)", all_users: true, approved_hive: HKEY_LOCAL_MACHINE, approved_key: "StartupFolder" },
    ]
}

#[derive(Serialize, Clone)]
pub struct Item {
    pub id: String,
    pub name: String,
    pub command: String,
    pub path: Option<String>,
    pub publisher: Option<String>,
    pub signed: Option<bool>,
    pub location: &'static str,
    pub all_users: bool,
    pub enabled: bool,
    /// Unix seconds when the target file was created.
    pub installed: Option<u64>,
    pub score: u32,
    pub level: &'static str,
    pub reasons: Vec<Reason>,
}

pub fn list() -> Vec<Item> {
    let mut out = Vec::new();
    for (i, src) in sources().iter().enumerate() {
        let approved = RegKey::predef(src.approved_hive)
            .open_subkey_with_flags(format!(r"{APPROVED}\{}", src.approved_key), KEY_READ)
            .ok();
        let is_enabled = |value: &str| {
            approved
                .as_ref()
                .and_then(|k| k.get_raw_value(value).ok())
                .is_none_or(|v| v.bytes.first().is_none_or(|b| b & 1 == 0))
        };
        match &src.kind {
            Kind::Run { hive, key } => {
                let Ok(k) = RegKey::predef(*hive).open_subkey_with_flags(key, KEY_READ | KEY_WOW64_64KEY) else { continue };
                for (name, value) in k.enum_values().flatten() {
                    let command = value.to_string();
                    let path = exe_from_command(&command);
                    out.push(item(i, src, name.clone(), command, path, is_enabled(&name)));
                }
            }
            Kind::Folder { env, sub } => {
                let Ok(base) = std::env::var(env) else { continue };
                let Ok(dir) = std::fs::read_dir(format!(r"{base}\{sub}")) else { continue };
                for entry in dir.flatten() {
                    let file = entry.file_name().to_string_lossy().to_string();
                    if file.eq_ignore_ascii_case("desktop.ini") {
                        continue;
                    }
                    let full = entry.path().to_string_lossy().to_string();
                    let path = if file.to_lowercase().ends_with(".lnk") { win::shortcut_target(&full) } else { Some(full.clone()) };
                    let mut it = item(i, src, file.trim_end_matches(".lnk").to_string(), full, path, is_enabled(&file));
                    it.id = format!("{i}|{file}");
                    out.push(it);
                }
            }
        }
    }
    out.sort_by(|a, b| b.score.cmp(&a.score).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    out
}

fn item(i: usize, src: &Source, name: String, command: String, path: Option<String>, enabled: bool) -> Item {
    let exists = path.as_deref().is_some_and(|p| std::path::Path::new(p).exists());
    let path = path.filter(|_| exists);
    let signed = path.as_deref().map(win::is_signed);
    let file_name = path.as_deref().and_then(|p| p.rsplit('\\').next()).unwrap_or(&name).to_string();
    let v = rules::analyze(&ProcFacts { name: &file_name, path: path.as_deref(), parent_name: None, signed });
    let installed = path
        .as_deref()
        .and_then(|p| std::fs::metadata(p).ok())
        .and_then(|m| m.created().ok())
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs());
    Item {
        id: format!("{i}|{name}"),
        publisher: path.as_deref().and_then(win::file_company),
        name,
        command,
        path,
        signed,
        location: src.label,
        all_users: src.all_users,
        enabled,
        installed,
        score: v.score,
        level: rules::level(v.score),
        reasons: v.reasons,
    }
}

/// Pulls the executable out of a Run command line.
fn exe_from_command(cmd: &str) -> Option<String> {
    let cmd = win::expand_env(cmd.trim());
    let exe = if let Some(rest) = cmd.strip_prefix('"') {
        rest.split('"').next()?.to_string()
    } else {
        let lower = cmd.to_lowercase();
        match lower.find(".exe") {
            Some(end) => cmd[..end + 4].to_string(),
            None => cmd.split(' ').next()?.to_string(),
        }
    };
    (!exe.is_empty()).then_some(exe)
}

/// Same format as Task Manager: first byte 2 = on, 3 = off, then a FILETIME.
pub fn set_enabled(id: &str, enabled: bool) -> Result<(), String> {
    let (idx, value) = id.split_once('|').ok_or("bad id")?;
    let idx: usize = idx.parse().map_err(|_| "bad id")?;
    let srcs = sources();
    let src = srcs.get(idx).ok_or("bad id")?;
    let key = RegKey::predef(src.approved_hive)
        .create_subkey_with_flags(format!(r"{APPROVED}\{}", src.approved_key), KEY_SET_VALUE)
        .map_err(|_| if src.all_users { "Needs admin rights".to_string() } else { "Can't write to the registry".to_string() })?
        .0;
    let mut bytes = vec![if enabled { 2u8 } else { 3u8 }, 0, 0, 0];
    let ft = if enabled { 0 } else { filetime_now() };
    bytes.extend_from_slice(&ft.to_le_bytes());
    key.set_raw_value(value, &RegValue { bytes: bytes.into(), vtype: winreg::enums::REG_BINARY })
        .map_err(|_| if src.all_users { "Needs admin rights".to_string() } else { "Can't write to the registry".to_string() })
}

fn filetime_now() -> u64 {
    // FILETIME counts 100 ns ticks since 1601.
    let unix = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    unix.as_nanos() as u64 / 100 + 116_444_736_000_000_000
}

#[cfg(test)]
mod tests {
    use super::exe_from_command;

    #[test]
    fn parses_run_commands() {
        assert_eq!(exe_from_command(r#""C:\Program Files\App\app.exe" --min"#).as_deref(), Some(r"C:\Program Files\App\app.exe"));
        assert_eq!(exe_from_command(r"C:\Tools\tool.exe /silent").as_deref(), Some(r"C:\Tools\tool.exe"));
        assert_eq!(exe_from_command(r"C:\Program Files\App\app.exe -x").as_deref(), Some(r"C:\Program Files\App\app.exe"));
    }
}
