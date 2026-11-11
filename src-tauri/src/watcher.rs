//! Background process watcher. Every tick it takes one process snapshot and
//! only analyses processes it hasn't seen yet. Per-process CPU and memory are
//! sampled only while the window is open.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::rules::{self, ProcFacts, Verdict};
use crate::win;

const TICK: Duration = Duration::from_secs(2);
const RECENT_LIMIT: usize = 200;
pub const ALERT_SCORE: u32 = 60;

#[derive(Clone, Serialize)]
pub struct Proc {
    pub pid: u32,
    pub ppid: u32,
    pub name: String,
    pub path: Option<String>,
    pub parent_name: Option<String>,
    pub signed: Option<bool>,
    pub trusted: bool,
    pub score: u32,
    pub level: &'static str,
    pub reasons: Vec<rules::Reason>,
    pub cpu: f32,
    pub mem: u64,
    #[serde(skip)]
    cpu_ticks: u64,
}

#[derive(Serialize)]
pub struct Totals {
    pub count: usize,
    pub self_cpu: f32,
    pub self_mem: u64,
}

#[derive(Clone, Serialize)]
pub struct RecentProcess {
    pub id: u64,
    pub ended_at: u64,
    #[serde(flatten)]
    process: Proc,
}

pub struct Shared {
    pub procs: Mutex<HashMap<u32, Proc>>,
    recent: Mutex<VecDeque<RecentProcess>>,
    pub trusted: Mutex<HashSet<String>>,
    /// Last time the window polled us.
    pub ui_seen: Mutex<Option<Instant>>,
    pub self_usage: Mutex<(f32, u64)>,
    trusted_file: PathBuf,
}

impl Shared {
    pub fn new(data_dir: PathBuf) -> Arc<Self> {
        let trusted_file = data_dir.join("trusted.json");
        let trusted = std::fs::read_to_string(&trusted_file)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Arc::new(Self {
            procs: Mutex::new(HashMap::new()),
            recent: Mutex::new(VecDeque::new()),
            trusted: Mutex::new(trusted),
            ui_seen: Mutex::new(None),
            self_usage: Mutex::new((0.0, 0)),
            trusted_file,
        })
    }

    pub fn recent(&self, revision: Option<u64>) -> Option<Vec<RecentProcess>> {
        let recent = self.recent.lock().unwrap();
        let latest = recent.back().map_or(0, |entry| entry.id);
        (revision != Some(latest)).then(|| recent.iter().rev().cloned().collect())
    }

    fn remember_exits(&self, exited: impl Iterator<Item = Proc>) {
        let mut exited = exited.peekable();
        if exited.peek().is_none() {
            return;
        }
        let ended_at = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64;
        let mut recent = self.recent.lock().unwrap();
        for process in exited {
            let id = recent.back().map_or(1, |entry| entry.id + 1);
            if recent.len() == RECENT_LIMIT {
                recent.pop_front();
            }
            recent.push_back(RecentProcess { id, ended_at, process });
        }
    }

    pub fn trust(&self, path: &str) {
        let mut t = self.trusted.lock().unwrap();
        t.insert(path.to_lowercase());
        if let Some(dir) = self.trusted_file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(&self.trusted_file, serde_json::to_string_pretty(&*t).unwrap_or_default());
        drop(t);
        for p in self.procs.lock().unwrap().values_mut() {
            if p.path.as_deref().is_some_and(|x| x.eq_ignore_ascii_case(path)) {
                p.trusted = true;
                p.score = 0;
                p.level = "calm";
                p.reasons.clear();
            }
        }
    }
}

/// `alert` gets newly found threats, but never while a fullscreen game is running.
pub fn spawn(shared: Arc<Shared>, alert: impl Fn(Vec<Proc>) + Send + 'static) {
    std::thread::Builder::new()
        .name("watcher".into())
        .spawn(move || run(shared, alert))
        .expect("watcher thread");
}

fn run(shared: Arc<Shared>, alert: impl Fn(Vec<Proc>)) {
    let mut sig_cache: HashMap<String, bool> = HashMap::new();
    let mut pending: Vec<u32> = Vec::new();
    let mut last_sample = Instant::now();
    let mut sampled_procs = false;
    let cpus = win::cpu_count();
    let me = std::process::id();
    let mut self_ticks = 0u64;

    loop {
        let Some(raw) = win::list_processes() else {
            std::thread::sleep(TICK);
            continue;
        };
        let names: HashMap<u32, &str> = raw.iter().map(|r| (r.pid, r.name.as_str())).collect();
        let trusted = shared.trusted.lock().unwrap().clone();

        let mut new_procs = Vec::new();
        {
            let procs = shared.procs.lock().unwrap();
            for r in &raw {
                // PIDs 0 and 4 are the idle and kernel pseudo-processes.
                if r.pid <= 4 || procs.get(&r.pid).is_some_and(|p| p.name == r.name) {
                    continue;
                }
                new_procs.push(r);
            }
        }

        let analysed: Vec<Proc> = new_procs
            .into_iter()
            .map(|r| {
                let path = win::exe_path(r.pid);
                let signed = path.as_deref().filter(|p| needs_signature_check(p)).map(|p| {
                    *sig_cache.entry(p.to_lowercase()).or_insert_with(|| win::is_signed(p))
                });
                let parent_name = names.get(&r.ppid).map(|s| s.to_string());
                let is_trusted = path.as_deref().is_some_and(|p| trusted.contains(&p.to_lowercase()));
                let v = if is_trusted {
                    Verdict::default()
                } else {
                    rules::analyze(&ProcFacts {
                        name: &r.name,
                        path: path.as_deref(),
                        parent_name: parent_name.as_deref(),
                        signed,
                    })
                };
                Proc {
                    pid: r.pid,
                    ppid: r.ppid,
                    name: r.name.clone(),
                    path,
                    parent_name,
                    signed,
                    trusted: is_trusted,
                    score: v.score,
                    level: rules::level(v.score),
                    reasons: v.reasons,
                    cpu: 0.0,
                    mem: 0,
                    cpu_ticks: 0,
                }
            })
            .collect();

        {
            let mut procs = shared.procs.lock().unwrap();
            shared.remember_exits(procs.extract_if(|pid, p| {
                names.get(pid).is_none_or(|name| *name != p.name)
            }).map(|(_, process)| process));
            for p in analysed {
                if p.score >= ALERT_SCORE {
                    pending.push(p.pid);
                }
                procs.insert(p.pid, p);
            }

            let elapsed = last_sample.elapsed().as_nanos() as u64 / 100;
            last_sample = Instant::now();
            let ui_open = shared.ui_seen.lock().unwrap().is_some_and(|t| t.elapsed() < Duration::from_secs(6));
            if ui_open {
                for p in procs.values_mut() {
                    if let Some((ticks, mem)) = win::usage(p.pid) {
                        if sampled_procs && p.cpu_ticks > 0 && elapsed > 0 {
                            p.cpu = (ticks.saturating_sub(p.cpu_ticks)) as f32 / (elapsed * cpus) as f32 * 100.0;
                        } else {
                            p.cpu = 0.0;
                        }
                        p.cpu_ticks = ticks;
                        p.mem = mem;
                    }
                }
            }
            sampled_procs = ui_open;
            if let Some((ticks, mem)) = win::usage(me) {
                let cpu = if self_ticks > 0 && elapsed > 0 {
                    ticks.saturating_sub(self_ticks) as f32 / (elapsed * cpus) as f32 * 100.0
                } else {
                    0.0
                };
                self_ticks = ticks;
                *shared.self_usage.lock().unwrap() = (cpu, mem);
            }

            if !pending.is_empty() && !win::user_is_busy() {
                let found: Vec<Proc> = pending.drain(..).filter_map(|pid| procs.get(&pid).cloned()).collect();
                if !found.is_empty() {
                    alert(found);
                }
            }
        }

        std::thread::sleep(TICK);
    }
}

/// Windows binaries are catalog-signed and often have no embedded signature,
/// so only files outside the Windows folder are checked.
fn needs_signature_check(path: &str) -> bool {
    !path.to_lowercase().starts_with(r"c:\windows\")
}

pub fn totals(shared: &Shared) -> Totals {
    let count = shared.procs.lock().unwrap().len();
    let (self_cpu, self_mem) = *shared.self_usage.lock().unwrap();
    Totals { count, self_cpu, self_mem }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_preserves_risk_and_distinguishes_reused_pids() {
        let shared = Shared::new(std::env::temp_dir().join("winglance-history-test"));
        let process = Proc {
            pid: 42,
            ppid: 7,
            name: "example.exe".into(),
            path: None,
            parent_name: None,
            signed: None,
            trusted: false,
            score: 65,
            level: "danger",
            reasons: vec![rules::Reason { pts: 65, title: "Example rule".into(), text: "Recorded reason".into() }],
            cpu: 0.0,
            mem: 0,
            cpu_ticks: 0,
        };
        shared.remember_exits((0..=RECENT_LIMIT).map(|_| process.clone()));
        let rows = shared.recent(None).unwrap();
        assert_eq!(rows.len(), RECENT_LIMIT);
        assert_eq!(rows.last().unwrap().id, 2);
        let newest = &rows[0];
        assert!(newest.id > rows[1].id);
        assert!(newest.ended_at > 0);
        assert_eq!(newest.process.pid, rows[1].process.pid);
        assert_eq!(newest.process.score, 65);
        assert_eq!(newest.process.reasons[0].text, "Recorded reason");
        assert!(shared.recent(Some(newest.id)).is_none());
    }

    #[test]
    fn process_snapshot_includes_this_process() {
        let processes = win::list_processes().expect("complete process snapshot");
        assert!(processes.iter().any(|p| p.pid == std::process::id()));
    }
}
