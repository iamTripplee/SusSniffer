# SusSniffer

A lightweight Task Manager alternative for Windows that also watches running processes and tells you, in plain words, why one of them looks suspicious.

Built with gamers in mind: in the background it uses about 5 MB of RAM and next to no CPU.

## Tabs

- **Processes** — grouped by risk, or click CPU / Memory to rank the top consumers.
- **Startup** — Run keys and Startup folders with publisher, install date, current memory use, an on/off switch and the same risk score as processes.
- **Hardware** — CPU and GPU load over the last minute, per-thread load, VRAM, donut charts of who is using the CPU and what is in memory, disks and network.

## What it flags

| Signal | Points |
|---|---|
| System name (`svchost.exe`, `lsass.exe`, `explorer.exe`, …) running from outside its system folder | +40 |
| Name one letter away from a system process (`scvhost.exe`, `expl0rer.exe`) | +35 |
| Office or a PDF reader launching a shell (Word → PowerShell) | +30 |
| Running from Temp, Downloads, the Recycle Bin or the Public folder | +20 |
| No digital signature (only adds to other findings) | +15 |

60+ is **Suspicious** and triggers a notification. 30+ is **Worth a look**.

Each process opens a breakdown of what matched, with actions to end it, show it in Explorer, look its hash up on VirusTotal, or trust it so it stops coming up.

## Why it's light

- The core lives in the tray, takes one process snapshot every 2 seconds and only analyses new processes.
- Per-process CPU and memory are sampled only while the window is open.
- The window is a separate process. Closing it exits that process along with WebView2, hardware counters and startup scans, so the memory goes back to the system.
- During fullscreen games notifications are held back until you're done.

## Building

Requires [Rust](https://rustup.rs), Node.js and Visual Studio Build Tools (C++).

```bash
npm install
npm run build
```

The installer ends up in `src-tauri/target/release/bundle/nsis/`.

Rule tests:

```bash
cd src-tauri
cargo test
```
