//! Hardware sampling via PDH counters. Runs in the window process only.

use std::collections::{HashMap, VecDeque};

use serde::Serialize;
use windows::core::{w, PCWSTR};
use windows::Win32::System::Performance::{
    PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW, PdhOpenQueryW,
    PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY, PDH_MORE_DATA,
};

use crate::win;

const HISTORY: usize = 30;

struct Query {
    handle: PDH_HQUERY,
    cpu: PDH_HCOUNTER,
    gpu: PDH_HCOUNTER,
    vram: PDH_HCOUNTER,
    disk_read: PDH_HCOUNTER,
    disk_write: PDH_HCOUNTER,
    net_down: PDH_HCOUNTER,
    net_up: PDH_HCOUNTER,
}

impl Query {
    fn open() -> Option<Self> {
        unsafe {
            let mut handle = PDH_HQUERY::default();
            if PdhOpenQueryW(PCWSTR::null(), 0, &mut handle) != 0 {
                return None;
            }
            let add = |path: PCWSTR| {
                let mut c = PDH_HCOUNTER::default();
                let _ = PdhAddEnglishCounterW(handle, path, 0, &mut c);
                c
            };
            let q = Self {
                handle,
                cpu: add(w!(r"\Processor(*)\% Processor Time")),
                gpu: add(w!(r"\GPU Engine(*)\Utilization Percentage")),
                vram: add(w!(r"\GPU Adapter Memory(*)\Dedicated Usage")),
                disk_read: add(w!(r"\PhysicalDisk(_Total)\Disk Read Bytes/sec")),
                disk_write: add(w!(r"\PhysicalDisk(_Total)\Disk Write Bytes/sec")),
                net_down: add(w!(r"\Network Interface(*)\Bytes Received/sec")),
                net_up: add(w!(r"\Network Interface(*)\Bytes Sent/sec")),
            };
            PdhCollectQueryData(handle);
            Some(q)
        }
    }

    fn values(counter: PDH_HCOUNTER) -> Vec<(String, f64)> {
        if counter.is_invalid() {
            return Vec::new();
        }
        unsafe {
            let (mut size, mut count) = (0u32, 0u32);
            if PdhGetFormattedCounterArrayW(counter, PDH_FMT_DOUBLE, &mut size, &mut count, None) != PDH_MORE_DATA {
                return Vec::new();
            }
            let mut buf = vec![0u8; size as usize];
            let items = buf.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W;
            if PdhGetFormattedCounterArrayW(counter, PDH_FMT_DOUBLE, &mut size, &mut count, Some(items)) != 0 {
                return Vec::new();
            }
            std::slice::from_raw_parts(items, count as usize)
                .iter()
                .filter(|i| i.FmtValue.CStatus == 0)
                .map(|i| (i.szName.to_string().unwrap_or_default(), i.FmtValue.Anonymous.doubleValue))
                .collect()
        }
    }
}

impl Drop for Query {
    fn drop(&mut self) {
        unsafe {
            PdhCloseQuery(self.handle);
        }
    }
}

#[derive(Default, Clone, Serialize)]
pub struct Live {
    pub cpu: f64,
    pub cores: Vec<f64>,
    pub gpu: f64,
    pub vram_used: u64,
    pub disk_read: f64,
    pub disk_write: f64,
    pub net_down: f64,
    pub net_up: f64,
    pub cpu_hist: VecDeque<f64>,
    pub gpu_hist: VecDeque<f64>,
    pub down_hist: VecDeque<f64>,
    pub up_hist: VecDeque<f64>,
}

#[derive(Default)]
pub struct Hardware {
    query: Option<Query>,
    pub live: Live,
}

// PDH handles are plain integers owned by this struct.
unsafe impl Send for Hardware {}

impl Hardware {
    pub fn sample(&mut self) {
        if self.query.is_none() {
            self.query = Query::open();
            return;
        }
        let q = self.query.as_ref().unwrap();
        unsafe {
            PdhCollectQueryData(q.handle);
        }

        let mut cores: Vec<(usize, f64)> = Vec::new();
        let mut total = 0.0;
        for (name, v) in Query::values(q.cpu) {
            match name.parse::<usize>() {
                Ok(i) => cores.push((i, v)),
                Err(_) if name == "_Total" => total = v,
                _ => {}
            }
        }
        cores.sort_by_key(|(i, _)| *i);

        // Task Manager style: busiest engine type, summed across processes.
        let mut by_engine: HashMap<String, f64> = HashMap::new();
        for (name, v) in Query::values(q.gpu) {
            if let Some(kind) = name.rsplit("engtype_").next() {
                *by_engine.entry(kind.to_string()).or_default() += v;
            }
        }
        let gpu = by_engine.values().copied().fold(0.0, f64::max).min(100.0);
        let vram_used = Query::values(q.vram).iter().map(|(_, v)| *v).fold(0.0, f64::max) as u64;
        let sum = |c| Query::values(c).iter().map(|(_, v)| v).sum::<f64>();

        let l = &mut self.live;
        l.cpu = total.clamp(0.0, 100.0);
        l.cores = cores.into_iter().map(|(_, v)| v.clamp(0.0, 100.0)).collect();
        l.gpu = gpu;
        l.vram_used = vram_used;
        l.disk_read = sum(q.disk_read);
        l.disk_write = sum(q.disk_write);
        l.net_down = sum(q.net_down);
        l.net_up = sum(q.net_up);
        for (hist, v) in [
            (&mut l.cpu_hist, l.cpu),
            (&mut l.gpu_hist, l.gpu),
            (&mut l.down_hist, l.net_down),
            (&mut l.up_hist, l.net_up),
        ] {
            hist.push_back(v);
            if hist.len() > HISTORY {
                hist.pop_front();
            }
        }
    }
}

#[derive(Serialize)]
pub struct Static {
    pub cpu_name: String,
    pub cpu_mhz: u32,
    pub threads: usize,
    pub gpu_name: Option<String>,
    pub vram_total: u64,
}

pub fn static_info() -> Static {
    use winreg::enums::HKEY_LOCAL_MACHINE;
    let key = winreg::RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(r"HARDWARE\DESCRIPTION\System\CentralProcessor\0").ok();
    let cpu_name: String = key.as_ref().and_then(|k| k.get_value("ProcessorNameString").ok()).unwrap_or_default();
    let cpu_mhz: u32 = key.as_ref().and_then(|k| k.get_value("~MHz").ok()).unwrap_or(0);
    let gpu = win::main_gpu();
    Static {
        cpu_name: cpu_name.trim().to_string(),
        cpu_mhz,
        threads: win::cpu_count() as usize,
        vram_total: gpu.as_ref().map(|g| g.1).unwrap_or(0),
        gpu_name: gpu.map(|g| g.0),
    }
}
