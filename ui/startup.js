(() => {
  const state = { items: [], query: '', loading: false };

  function fmtDate(secs) {
    if (!secs) return '—';
    const d = new Date(secs * 1000);
    const days = Math.floor((Date.now() - d) / 86400000);
    if (days < 1) return 'Today';
    if (days < 30) return `${days} day${days === 1 ? '' : 's'} ago`;
    return d.toLocaleDateString('en', { month: 'short', year: 'numeric' });
  }

  function usingNow(it) {
    if (!it.path) return '—';
    const path = it.path.toLowerCase();
    const mem = App.procs.filter((p) => p.path && p.path.toLowerCase() === path).reduce((m, p) => m + p.mem, 0);
    return mem ? fmtBytes(mem) : '—';
  }

  function rowHtml(it) {
    const reason = it.reasons[0];
    const sub = reason
      ? `<span class="sub" style="color:${toneVar[it.level]}">${esc(reason.title)}</span>`
      : `<span class="sub mono">${esc(it.path || it.command)}</span>`;
    const fresh = it.installed && Date.now() / 1000 - it.installed < 7 * 86400;
    const publisher = it.publisher || (it.path ? 'Unknown' : 'File missing');
    const lock = it.all_users ? ' title="Changing all-user entries needs admin rights"' : '';
    return `<div class="row st-grid ${it.level === 'danger' ? 'danger' : ''} ${it.enabled ? '' : 'off'}">
      <span class="who">
        <span class="tile" style="background:${tileColor(it.name)}" aria-hidden="true">${esc(it.name[0]?.toUpperCase() || '?')}</span>
        <span class="names"><span class="nm">${esc(it.name)}</span>${sub}</span>
      </span>
      <span class="cell ellip dim" style="${it.publisher || it.level === 'calm' ? '' : `color:${toneVar[it.level]}`}">${esc(publisher)}</span>
      <span class="cell ellip dim">${esc(it.location)}</span>
      <span class="cell ${fresh ? 'strong' : 'dim'}">${fmtDate(it.installed)}</span>
      <span class="cell r dim">${usingNow(it)}</span>
      ${riskHtml(it.score, it.level)}
      <span class="r"><button class="switch" role="switch" aria-checked="${it.enabled}" aria-label="Start ${esc(it.name)} with Windows" data-id="${esc(it.id)}"${lock}><i></i></button></span>
    </div>`;
  }

  function render() {
    const q = state.query.trim().toLowerCase();
    const items = state.items.filter((it) => !q || it.name.toLowerCase().includes(q) || (it.publisher || '').toLowerCase().includes(q));
    const sus = state.items.filter((it) => it.level === 'danger').length;
    const newest = state.items.reduce((m, it) => Math.max(m, it.installed || 0), 0);
    $('st-head').textContent = `${plural(state.items.length, 'app')} start with Windows`;
    $('st-sub').textContent = [sus ? `${sus} look${sus === 1 ? 's' : ''} suspicious` : 'Nothing suspicious', newest ? `newest installed ${fmtDate(newest).toLowerCase()}` : ''].filter(Boolean).join(' · ');
    const on = state.items.filter((it) => it.enabled).length;
    $('foot-info').textContent = `${on} enabled · ${state.items.length - on} disabled`;
    $('st-list').innerHTML = items.length ? items.map(rowHtml).join('') : `<div class="empty">${state.loading ? 'Loading…' : 'Nothing found'}</div>`;
  }

  async function load() {
    state.loading = true;
    render();
    try {
      state.items = await invoke('startup_list');
    } catch (e) {
      console.error(e);
    }
    state.loading = false;
    render();
  }

  App.register('startup', {
    init() {
      $('st-list').addEventListener('click', async (e) => {
        const sw = e.target.closest('.switch');
        if (!sw) return;
        const it = state.items.find((x) => x.id === sw.dataset.id);
        if (!it) return;
        try {
          await invoke('startup_set', { id: it.id, enabled: !it.enabled });
          it.enabled = !it.enabled;
          render();
        } catch (err) {
          const msg = String(err?.message ?? err);
          alert(msg === 'Needs admin rights'
            ? `${it.name} is set up for all users. Turning it off needs WinGlance to run as administrator.`
            : msg);
        }
      });
    },
    enter: load,
    refresh: render,
    search(q) {
      state.query = q;
      render();
    },
  });
})();
