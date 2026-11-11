use std::mem::size_of;

use windows::core::{GUID, PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, FILETIME, HANDLE, HWND};
use windows::Win32::Security::WinTrust::{
    WinVerifyTrust, WINTRUST_ACTION_GENERIC_VERIFY_V2, WINTRUST_DATA, WINTRUST_DATA_0,
    WINTRUST_FILE_INFO, WTD_CACHE_ONLY_URL_RETRIEVAL, WTD_CHOICE_FILE, WTD_REVOKE_NONE,
    WTD_STATEACTION_CLOSE, WTD_STATEACTION_VERIFY, WTD_UI_NONE,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
use windows::Win32::System::Threading::{
    GetPriorityClass, GetProcessTimes, OpenProcess, QueryFullProcessImageNameW, SetPriorityClass,
    TerminateProcess, ABOVE_NORMAL_PRIORITY_CLASS, BELOW_NORMAL_PRIORITY_CLASS, HIGH_PRIORITY_CLASS,
    IDLE_PRIORITY_CLASS, NORMAL_PRIORITY_CLASS, PROCESS_CREATION_FLAGS, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_INFORMATION, PROCESS_TERMINATE,
    REALTIME_PRIORITY_CLASS,
};
use windows::Win32::UI::Shell::{
    SHQueryUserNotificationState, QUNS_BUSY, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN,
};

pub struct RawProc {
    pub pid: u32,
    pub ppid: u32,
    pub name: String,
}

pub fn list_processes() -> Vec<RawProc> {
    let mut out = Vec::with_capacity(400);
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return out;
        };
        let mut e = PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        if Process32FirstW(snap, &mut e).is_ok() {
            loop {
                let len = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len());
                out.push(RawProc {
                    pid: e.th32ProcessID,
                    ppid: e.th32ParentProcessID,
                    name: String::from_utf16_lossy(&e.szExeFile[..len]),
                });
                if Process32NextW(snap, &mut e).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
    }
    out
}

struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

fn open(pid: u32, access: windows::Win32::System::Threading::PROCESS_ACCESS_RIGHTS) -> Option<Handle> {
    unsafe { OpenProcess(access, false, pid).ok().map(Handle) }
}

pub fn exe_path(pid: u32) -> Option<String> {
    let h = open(pid, PROCESS_QUERY_LIMITED_INFORMATION)?;
    let mut buf = [0u16; 1024];
    let mut len = buf.len() as u32;
    unsafe {
        QueryFullProcessImageNameW(h.0, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).ok()?;
    }
    Some(String::from_utf16_lossy(&buf[..len as usize]))
}

/// Total CPU time in 100 ns ticks and working set in bytes.
pub fn usage(pid: u32) -> Option<(u64, u64)> {
    let h = open(pid, PROCESS_QUERY_LIMITED_INFORMATION)?;
    let ft = |f: FILETIME| ((f.dwHighDateTime as u64) << 32) | f.dwLowDateTime as u64;
    unsafe {
        let (mut c, mut e, mut k, mut u) = Default::default();
        GetProcessTimes(h.0, &mut c, &mut e, &mut k, &mut u).ok()?;
        let mut mem = PROCESS_MEMORY_COUNTERS { cb: size_of::<PROCESS_MEMORY_COUNTERS>() as u32, ..Default::default() };
        let mem_bytes = if K32GetProcessMemoryInfo(h.0, &mut mem, mem.cb).as_bool() {
            mem.WorkingSetSize as u64
        } else {
            0
        };
        Some((ft(k) + ft(u), mem_bytes))
    }
}

pub fn kill(pid: u32) -> bool {
    match open(pid, PROCESS_TERMINATE) {
        Some(h) => unsafe { TerminateProcess(h.0, 1).is_ok() },
        None => false,
    }
}

const PRIORITIES: [(&str, PROCESS_CREATION_FLAGS); 5] = [
    ("high", HIGH_PRIORITY_CLASS),
    ("above", ABOVE_NORMAL_PRIORITY_CLASS),
    ("normal", NORMAL_PRIORITY_CLASS),
    ("below", BELOW_NORMAL_PRIORITY_CLASS),
    ("low", IDLE_PRIORITY_CLASS),
];

pub fn process_controls(pid: u32) -> (Option<&'static str>, bool, bool) {
    let priority = open(pid, PROCESS_QUERY_LIMITED_INFORMATION).and_then(|h| {
        let class = unsafe { GetPriorityClass(h.0) };
        if class == REALTIME_PRIORITY_CLASS.0 {
            Some("realtime")
        } else {
            PRIORITIES.iter().find(|(_, value)| value.0 == class).map(|(name, _)| *name)
        }
    });
    (
        priority,
        open(pid, PROCESS_SET_INFORMATION).is_some(),
        open(pid, PROCESS_TERMINATE).is_some(),
    )
}

pub fn set_priority(pid: u32, name: &str) -> Result<(), String> {
    let (_, class) = PRIORITIES.iter().find(|(key, _)| *key == name).ok_or("Invalid priority")?;
    let action = || {
        let h = Handle(unsafe { OpenProcess(PROCESS_SET_INFORMATION, false, pid)? });
        unsafe { SetPriorityClass(h.0, *class) }
    };
    action().map_err(|e| {
        if e.code() == windows::core::HRESULT::from_win32(5) {
            "Access denied. The process may be protected or require administrator rights.".into()
        } else {
            format!("Couldn't set priority: {e}")
        }
    })
}

pub fn reveal_path(path: &str) -> Result<(), String> {
    let file = std::path::Path::new(path);
    if !file.is_absolute() || !file.is_file() {
        return Err("The file is missing or inaccessible.".into());
    }
    std::process::Command::new("explorer.exe")
        .arg(format!("/select,{path}"))
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Couldn't open File Explorer: {e}"))
}

/// Embedded Authenticode check. Revocation lookups are off to keep it offline and fast.
pub fn is_signed(path: &str) -> bool {
    let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
    let mut file = WINTRUST_FILE_INFO {
        cbStruct: size_of::<WINTRUST_FILE_INFO>() as u32,
        pcwszFilePath: PCWSTR(wide.as_ptr()),
        ..Default::default()
    };
    let mut data = WINTRUST_DATA {
        cbStruct: size_of::<WINTRUST_DATA>() as u32,
        dwUIChoice: WTD_UI_NONE,
        fdwRevocationChecks: WTD_REVOKE_NONE,
        dwUnionChoice: WTD_CHOICE_FILE,
        Anonymous: WINTRUST_DATA_0 { pFile: &mut file },
        dwStateAction: WTD_STATEACTION_VERIFY,
        dwProvFlags: WTD_CACHE_ONLY_URL_RETRIEVAL,
        ..Default::default()
    };
    let mut action: GUID = WINTRUST_ACTION_GENERIC_VERIFY_V2;
    unsafe {
        let status = WinVerifyTrust(HWND::default(), &mut action, &mut data as *mut _ as *mut _);
        data.dwStateAction = WTD_STATEACTION_CLOSE;
        WinVerifyTrust(HWND::default(), &mut action, &mut data as *mut _ as *mut _);
        status == 0
    }
}

/// True while a fullscreen game or presentation is running.
pub fn user_is_busy() -> bool {
    unsafe {
        matches!(
            SHQueryUserNotificationState(),
            Ok(s) if s == QUNS_RUNNING_D3D_FULL_SCREEN || s == QUNS_BUSY || s == QUNS_PRESENTATION_MODE
        )
    }
}

pub fn cpu_count() -> u64 {
    std::thread::available_parallelism().map(|n| n.get() as u64).unwrap_or(1)
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn from_wide(buf: &[u16]) -> String {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}

/// Target path of a .lnk file.
pub fn shortcut_target(lnk: &str) -> Option<String> {
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, IPersistFile, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, STGM_READ,
    };
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink, SLGP_RAWPATH};
    use windows::core::Interface;
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
        let file: IPersistFile = link.cast().ok()?;
        let w = wide(lnk);
        file.Load(PCWSTR(w.as_ptr()), STGM_READ).ok()?;
        let mut buf = [0u16; 1024];
        link.GetPath(&mut buf, std::ptr::null_mut(), SLGP_RAWPATH.0 as u32).ok()?;
        let target = from_wide(&buf);
        (!target.is_empty()).then(|| expand_env(&target))
    }
}

pub fn expand_env(s: &str) -> String {
    use windows::Win32::System::Environment::ExpandEnvironmentStringsW;
    let w = wide(s);
    let mut buf = vec![0u16; 2048];
    let n = unsafe { ExpandEnvironmentStringsW(PCWSTR(w.as_ptr()), Some(&mut buf)) };
    if n == 0 || n as usize > buf.len() { s.to_string() } else { from_wide(&buf) }
}

/// CompanyName from the file's version resource.
pub fn file_company(path: &str) -> Option<String> {
    use windows::Win32::Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW};
    let w = wide(path);
    unsafe {
        let size = GetFileVersionInfoSizeW(PCWSTR(w.as_ptr()), None);
        if size == 0 {
            return None;
        }
        let mut data = vec![0u8; size as usize];
        GetFileVersionInfoW(PCWSTR(w.as_ptr()), Some(0), size, data.as_mut_ptr().cast()).ok()?;

        let mut ptr = std::ptr::null_mut();
        let mut len = 0u32;
        let q = wide(r"\VarFileInfo\Translation");
        let langs: Vec<(u16, u16)> = if VerQueryValueW(data.as_ptr().cast(), PCWSTR(q.as_ptr()), &mut ptr, &mut len).as_bool() && len >= 4 {
            std::slice::from_raw_parts(ptr as *const u16, len as usize / 2).chunks(2).map(|c| (c[0], c[1])).collect()
        } else {
            vec![(0x0409, 0x04b0)]
        };
        for (lang, cp) in langs {
            let q = wide(&format!(r"\StringFileInfo\{lang:04x}{cp:04x}\CompanyName"));
            if VerQueryValueW(data.as_ptr().cast(), PCWSTR(q.as_ptr()), &mut ptr, &mut len).as_bool() && len > 1 {
                let s = from_wide(std::slice::from_raw_parts(ptr as *const u16, len as usize)).trim().to_string();
                if !s.is_empty() {
                    return Some(s);
                }
            }
        }
        None
    }
}

/// (total, available) physical memory in bytes.
pub fn memory_status() -> (u64, u64) {
    use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    let mut m = MEMORYSTATUSEX { dwLength: size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
    unsafe {
        if GlobalMemoryStatusEx(&mut m).is_ok() { (m.ullTotalPhys, m.ullAvailPhys) } else { (0, 0) }
    }
}

pub struct Disk {
    pub root: String,
    pub label: String,
    pub total: u64,
    pub free: u64,
}

pub fn fixed_disks() -> Vec<Disk> {
    use windows::Win32::Storage::FileSystem::{GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDriveStringsW, GetVolumeInformationW};
    const DRIVE_FIXED: u32 = 3;
    let mut buf = [0u16; 512];
    let n = unsafe { GetLogicalDriveStringsW(Some(&mut buf)) } as usize;
    let mut out = Vec::new();
    for root in buf[..n.min(buf.len())].split(|&c| c == 0).filter(|s| !s.is_empty()) {
        let w: Vec<u16> = root.iter().copied().chain(std::iter::once(0)).collect();
        unsafe {
            if GetDriveTypeW(PCWSTR(w.as_ptr())) != DRIVE_FIXED {
                continue;
            }
            let (mut free, mut total) = (0u64, 0u64);
            if GetDiskFreeSpaceExW(PCWSTR(w.as_ptr()), None, Some(&mut total), Some(&mut free)).is_err() {
                continue;
            }
            let mut label = [0u16; 261];
            let _ = GetVolumeInformationW(PCWSTR(w.as_ptr()), Some(&mut label), None, None, None, None);
            out.push(Disk { root: String::from_utf16_lossy(root), label: from_wide(&label), total, free });
        }
    }
    out
}

/// Name and dedicated VRAM of the GPU with the most video memory.
pub fn main_gpu() -> Option<(String, u64)> {
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE};
    unsafe {
        let factory: IDXGIFactory1 = CreateDXGIFactory1().ok()?;
        let mut best: Option<(String, u64)> = None;
        let mut i = 0;
        while let Ok(adapter) = factory.EnumAdapters1(i) {
            i += 1;
            let Ok(desc) = adapter.GetDesc1() else { continue };
            if desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0 {
                continue;
            }
            let vram = desc.DedicatedVideoMemory as u64;
            if best.as_ref().is_none_or(|(_, v)| vram > *v) {
                best = Some((from_wide(&desc.Description), vram));
            }
        }
        best
    }
}
