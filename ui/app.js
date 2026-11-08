const { invoke } = window.__TAURI__.core;
const appWindow = window.__TAURI__.window.getCurrentWindow();

async function core(cmd, pid, arg) {
  const res = await invoke('core', { cmd, pid: pid ?? null, arg: arg ?? null });
  if (res && res.error) throw new Error(res.error);
  return res;
}

const $ = (id) => document.getElementById(id);

// Process names, paths and registry values are untrusted input.
const esc = (s) => String(s ?? '').replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));

const plural = (n, word) => `${n} ${word}${n === 1 ? '' : word.endsWith('s') ? 'es' : 's'}`;

function fmtBytes(bytes) {
  if (!bytes) return '—';
  const mb = bytes / 1048576;
  if (mb >= 1048576) return (mb / 1048576).toFixed(2) + ' TB';
  if (mb >= 1024) return (mb / 1024).toFixed(1) + ' GB';
  return Math.round(mb) + ' MB';
}

function fmtRate(bps) {
  if (bps >= 1048576) return (bps / 1048576).toFixed(1) + ' MB/s';
  if (bps >= 1024) return Math.round(bps / 1024) + ' KB/s';
  return Math.round(bps) + ' B/s';
}

function fmtCpu(v) {
  if (!v || v < 0.05) return '0%';
  return (v < 10 ? v.toFixed(1) : Math.round(v)) + '%';
}

function tileColor(name) {
  let h = 0;
  for (const c of name) h = (h * 31 + c.charCodeAt(0)) >>> 0;
  return `hsl(${h % 360} 28% 38%)`;
}

const toneVar = { danger: 'var(--danger)', warn: 'var(--warn)', calm: 'var(--calm)' };

function riskHtml(score, level) {
  return `<span class="risk"><span class="bar"><i style="width:${score}%;background:${toneVar[level]}"></i></span><b style="color:${toneVar[level]}">${score}</b></span>`;
}

const App = {
  views: {},
  current: 'processes',
  procs: [],

  register(name, view) {
    this.views[name] = view;
  },

  show(name) {
    this.current = name;
    document.querySelectorAll('#nav button').forEach((b) => b.classList.toggle('on', b.dataset.view === name));
    for (const v of Object.keys(this.views)) $(`view-${v}`).hidden = v !== name;
    $('search').hidden = name === 'hardware';
    $('hw-note').hidden = name !== 'hardware';
    $('q').value = '';
    $('foot-info').textContent = '';
    this.views[name].enter?.();
  },

  async tick() {
    try {
      const snap = await core('snapshot');
      this.procs = snap.procs;
      const t = snap.totals;
      $('self').textContent = `WinGlance core: ${fmtCpu(t.self_cpu)} CPU · ${fmtBytes(t.self_mem)}`;
      this.renderUpdate(snap.update);
      if (snap.focus) {
        await appWindow.unminimize();
        await appWindow.setFocus();
      }
      await this.views[this.current].refresh?.();
    } catch (e) {
      console.error(e);
    }
  },

  renderUpdate(u) {
    const btn = $('update');
    if (!u) return;
    $('version').textContent = `v${u.current}`;
    const busy = u.phase === 'checking' || u.phase === 'downloading';
    btn.hidden = false;
    btn.disabled = busy;
    btn.classList.toggle('go', !!u.available && !busy);
    btn.title = u.error || '';
    btn.textContent = u.phase === 'checking' ? 'Checking…'
      : u.phase === 'downloading' ? 'Updating…'
      : u.available ? `Update to ${u.available}`
      : u.phase === 'up_to_date' ? 'Up to date'
      : u.phase === 'failed' ? 'Check failed, retry'
      : 'Check for updates';
  },

  start() {
    document.querySelectorAll('#nav button').forEach((b) => b.addEventListener('click', () => this.show(b.dataset.view)));
    $('q').addEventListener('input', (e) => this.views[this.current].search?.(e.target.value));
    $('update').addEventListener('click', () => core('update').then(() => this.tick()));
    $('min').addEventListener('click', () => appWindow.minimize());
    $('close').addEventListener('click', () => appWindow.close());

    const themes = ['glass', 'black', 'light'];
    const apply = (t) => {
      document.documentElement.dataset.theme = t;
      try { localStorage.setItem('theme', t); } catch {}
      this.views.hardware?.repaint?.();
    };
    $('theme').addEventListener('click', () => {
      const cur = document.documentElement.dataset.theme;
      apply(themes[(themes.indexOf(cur) + 1) % themes.length]);
    });
    try { apply(localStorage.getItem('theme') || 'glass'); } catch {}

    for (const v of Object.values(this.views)) v.init?.();
    this.show('processes');
    this.tick();
    setInterval(() => this.tick(), 2000);
  },
};

window.addEventListener('DOMContentLoaded', () => App.start());
