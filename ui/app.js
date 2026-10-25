const { invoke } = window.__TAURI__.core;
const appWindow = window.__TAURI__.window.getCurrentWindow();

async function core(cmd, pid) {
  const res = await invoke('core', { cmd, pid: pid ?? null });
  if (res && res.error) throw new Error(res.error);
  return res;
}

const THEMES = ['glass', 'black', 'light'];
const CALM_LIMIT = 40;
const $ = (id) => document.getElementById(id);

const state = {
  tab: 'all',
  query: '',
  closed: new Set(),
  showAllCalm: false,
  procs: [],
  detailPid: null,
};

// Process names and paths are untrusted input.
const esc = (s) => String(s ?? '').replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));

const plural = (n, word) => `${n} ${word}${n === 1 ? '' : 'es'}`;

function fmtMem(bytes) {
  if (!bytes) return '—';
  const mb = bytes / 1048576;
  return mb >= 1024 ? (mb / 1024).toFixed(1) + ' GB' : Math.round(mb) + ' MB';
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

function rowHtml(p) {
  const reason = p.reasons[0];
  const sub = reason
    ? `<span class="sub" style="color:${toneVar[p.level]}">${esc(reason.title)}</span>`
    : `<span class="sub mono">${esc(p.path || 'system process')}</span>`;
  return `<button class="row row-grid ${p.level === 'danger' ? 'danger' : ''}" data-pid="${p.pid}">
    <span class="who">
      <span class="tile" style="background:${tileColor(p.name)}" aria-hidden="true">${esc(p.name[0]?.toUpperCase() || '?')}</span>
      <span class="names"><span class="nm">${esc(p.name)}</span>${sub}</span>
    </span>
    <span class="cell r">${fmtCpu(p.cpu)}</span>
    <span class="cell r dim">${fmtMem(p.mem)}</span>
    <span class="risk"><span class="bar"><i style="width:${p.score}%;background:${toneVar[p.level]}"></i></span><b style="color:${toneVar[p.level]}">${p.score}</b></span>
  </button>`;
}

function render() {
  const q = state.query.trim().toLowerCase();
  const visible = state.procs.filter((p) => !q || p.name.toLowerCase().includes(q));
  const groups = [
    { id: 'danger', title: 'Suspicious', color: 'var(--danger)', items: visible.filter((p) => p.level === 'danger') },
    { id: 'warn', title: 'Worth a look', color: 'var(--warn)', items: visible.filter((p) => p.level === 'warn') },
    { id: 'calm', title: 'Normal', color: 'var(--ok)', items: visible.filter((p) => p.level === 'calm') },
  ].filter((g) => g.items.length && (state.tab === 'all' || g.id !== 'calm'));

  const danger = state.procs.filter((p) => p.level === 'danger').length;
  const warn = state.procs.filter((p) => p.level === 'warn').length;
  $('headline').textContent = danger
    ? `${plural(danger, 'suspicious process')}`
    : warn ? `${plural(warn, 'process')} worth a look` : 'All clear';
  $('sub').textContent = `Watching ${plural(state.procs.length, 'process')}`;

  if (!groups.length) {
    $('list').innerHTML = `<div class="empty">${q ? 'Nothing found' : 'No threats found'}</div>`;
    return;
  }

  $('list').innerHTML = groups.map((g) => {
    const open = !state.closed.has(g.id);
    let items = g.items;
    let more = '';
    if (g.id === 'calm' && !state.showAllCalm && !q && items.length > CALM_LIMIT) {
      more = `<button class="more" data-more>Show all ${items.length}</button>`;
      items = items.slice(0, CALM_LIMIT);
    }
    return `<section>
      <button class="group-h ${open ? 'open' : ''}" data-group="${g.id}" aria-expanded="${open}">
        <svg class="chev" viewBox="0 0 10 10" aria-hidden="true"><path d="M3 2l4 3-4 3"/></svg>
        <span class="dot" style="background:${g.color}"></span>
        <span class="t">${g.title}</span><span class="c">${g.items.length}</span>
      </button>
      ${open ? items.map(rowHtml).join('') + more : ''}
    </section>`;
  }).join('');
}

async function refresh() {
  try {
    const snap = await core('snapshot');
    state.procs = snap.procs;
    const t = snap.totals;
    $('self').textContent = `SusSniffer core: ${fmtCpu(t.self_cpu)} CPU · ${fmtMem(t.self_mem)}`;
    if (snap.focus) {
      await appWindow.unminimize();
      await appWindow.setFocus();
    }
    render();
    if (state.detailPid != null) fillDetail();
  } catch (e) {
    console.error(e);
  }
}

const levelText = { danger: 'High risk', warn: 'Medium risk', calm: 'Low risk' };

function fillDetail() {
  const p = state.procs.find((x) => x.pid === state.detailPid);
  if (!p) {
    closeDetail();
    return;
  }
  $('d-tile').textContent = p.name[0]?.toUpperCase() || '?';
  $('d-tile').style.background = tileColor(p.name);
  $('d-name').textContent = p.name;
  $('d-meta').textContent = `PID ${p.pid}${p.trusted ? ' · trusted by you' : ''}`;
  $('d-score').textContent = p.score;
  $('d-score').style.color = toneVar[p.level];
  $('d-level').textContent = levelText[p.level];
  $('d-level').style.color = toneVar[p.level];

  $('d-why-label').textContent = p.reasons.length ? 'Why it looks suspicious' : 'Checks';
  $('d-reasons').innerHTML = p.reasons.length
    ? p.reasons.map((r) => `<div class="reason"><div class="pts">+${r.pts}</div><div><div class="rt">${esc(r.title)}</div><div class="rx">${esc(r.text)}</div></div></div>`).join('')
    : `<div class="reason"><div><div class="rt">Nothing suspicious</div><div class="rx">None of the SusSniffer rules matched.</div></div></div>`;

  $('d-path').textContent = p.path || 'unavailable (protected system process)';
  $('d-parent').textContent = p.parent_name ? `${p.parent_name} (PID ${p.ppid})` : 'exited or unknown';
  $('d-sign').textContent = p.signed == null ? 'not checked' : p.signed ? 'valid' : 'missing';

  const hasPath = !!p.path;
  $('a-reveal').disabled = !hasPath;
  $('a-vt').disabled = !hasPath;
  $('a-trust').hidden = !hasPath || p.trusted || p.score === 0;
}

function openDetail(pid) {
  state.detailPid = pid;
  fillDetail();
  $('detail').hidden = false;
}

function closeDetail() {
  state.detailPid = null;
  $('detail').hidden = true;
}

$('list').addEventListener('click', (e) => {
  const g = e.target.closest('[data-group]');
  if (g) {
    const id = g.dataset.group;
    state.closed.has(id) ? state.closed.delete(id) : state.closed.add(id);
    render();
    return;
  }
  if (e.target.closest('[data-more]')) {
    state.showAllCalm = true;
    render();
    return;
  }
  const row = e.target.closest('[data-pid]');
  if (row) openDetail(Number(row.dataset.pid));
});

document.querySelectorAll('.seg button').forEach((b) => b.addEventListener('click', () => {
  state.tab = b.dataset.tab;
  document.querySelectorAll('.seg button').forEach((x) => x.classList.toggle('on', x === b));
  render();
}));

$('q').addEventListener('input', (e) => {
  state.query = e.target.value;
  render();
});

$('min').addEventListener('click', () => appWindow.minimize());
$('close').addEventListener('click', () => appWindow.close());

function applyTheme(t) {
  document.documentElement.dataset.theme = t;
  try { localStorage.setItem('theme', t); } catch {}
}
$('theme').addEventListener('click', () => {
  const cur = document.documentElement.dataset.theme;
  applyTheme(THEMES[(THEMES.indexOf(cur) + 1) % THEMES.length]);
});
try { applyTheme(localStorage.getItem('theme') || 'glass'); } catch {}

$('d-close').addEventListener('click', closeDetail);
$('detail').addEventListener('click', (e) => { if (e.target === $('detail')) closeDetail(); });
document.addEventListener('keydown', (e) => { if (e.key === 'Escape' && state.detailPid != null) closeDetail(); });

$('a-kill').addEventListener('click', async () => {
  const p = state.procs.find((x) => x.pid === state.detailPid);
  if (!p || !confirm(`End ${p.name}? Any unsaved work in it will be lost.`)) return;
  const ok = await core('kill', p.pid);
  if (!ok) alert("Couldn't end the process. It may need admin rights.");
  await refresh();
});
$('a-reveal').addEventListener('click', () => core('reveal', state.detailPid));
$('a-vt').addEventListener('click', () => core('virustotal', state.detailPid).catch((e) => alert(e.message)));
$('a-trust').addEventListener('click', async () => {
  await core('trust', state.detailPid);
  await refresh();
});

refresh();
setInterval(refresh, 2000);
