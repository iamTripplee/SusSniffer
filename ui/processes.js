(() => {
  const CALM_LIMIT = 40;
  const TOP_LIMIT = 60;
  const state = { tab: 'all', query: '', sort: 'risk', closed: new Set(), showAllCalm: false, detailPid: null };
  const procs = () => App.procs;

  function rowHtml(p, rank, maxCpu, maxMem) {
    const reason = p.reasons[0];
    const sub = reason
      ? `<span class="sub" style="color:${toneVar[p.level]}">${esc(reason.title)}</span>`
      : `<span class="sub mono">${esc(p.path || 'system process')}</span>`;
    const meter = (v, max) => `<span class="meter"><i style="width:${max ? (v / max) * 100 : 0}%"></i></span>`;
    return `<button class="row row-grid ${p.level === 'danger' ? 'danger' : ''}" data-pid="${p.pid}">
      <span class="who">
        ${rank ? `<span class="rank">${rank}</span>` : ''}
        <span class="tile" style="background:${tileColor(p.name)}" aria-hidden="true">${esc(p.name[0]?.toUpperCase() || '?')}</span>
        <span class="names"><span class="nm">${esc(p.name)}</span>${sub}</span>
      </span>
      <span class="cell r ${state.sort === 'cpu' ? 'strong' : ''}">${fmtCpu(p.cpu)}${state.sort === 'cpu' ? meter(p.cpu, maxCpu) : ''}</span>
      <span class="cell r ${state.sort === 'mem' ? 'strong' : 'dim'}">${fmtBytes(p.mem)}${state.sort === 'mem' ? meter(p.mem, maxMem) : ''}</span>
      ${riskHtml(p.score, p.level)}
    </button>`;
  }

  function render() {
    const all = procs();
    const q = state.query.trim().toLowerCase();
    const visible = all.filter((p) => (!q || p.name.toLowerCase().includes(q)) && (state.tab === 'all' || p.level !== 'calm'));

    const danger = all.filter((p) => p.level === 'danger').length;
    const warn = all.filter((p) => p.level === 'warn').length;
    $('headline').textContent = danger
      ? plural(danger, 'suspicious process')
      : warn ? `${plural(warn, 'process')} worth a look` : 'All clear';
    $('sub').textContent = `Watching ${plural(all.length, 'process')}`;

    document.querySelectorAll('.cols [data-sort]').forEach((b) => b.classList.toggle('on', b.dataset.sort === state.sort));
    const pill = $('sort-pill');
    pill.hidden = state.sort === 'risk';
    pill.innerHTML = `Top by ${state.sort === 'cpu' ? 'CPU' : 'memory'} <svg width="10" height="10" viewBox="0 0 12 12" aria-hidden="true"><path d="M3 3l6 6M9 3l-6 6"/></svg>`;

    if (!visible.length) {
      $('list').innerHTML = `<div class="empty">${q ? 'Nothing found' : 'No threats found'}</div>`;
      return;
    }

    if (state.sort !== 'risk') {
      const key = state.sort;
      const ranked = [...visible].sort((a, b) => b[key] - a[key]).slice(0, TOP_LIMIT);
      const maxCpu = ranked.reduce((m, p) => Math.max(m, p.cpu), 0);
      const maxMem = ranked.reduce((m, p) => Math.max(m, p.mem), 0);
      $('list').innerHTML = ranked.map((p, i) => rowHtml(p, i + 1, maxCpu, maxMem)).join('');
      return;
    }

    const groups = [
      { id: 'danger', title: 'Suspicious', color: 'var(--danger)' },
      { id: 'warn', title: 'Worth a look', color: 'var(--warn)' },
      { id: 'calm', title: 'Normal', color: 'var(--ok)' },
    ].map((g) => ({ ...g, items: visible.filter((p) => p.level === g.id) })).filter((g) => g.items.length);

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
        ${open ? items.map((p) => rowHtml(p)).join('') + more : ''}
      </section>`;
    }).join('');
  }

  const levelText = { danger: 'High risk', warn: 'Medium risk', calm: 'Low risk' };

  function fillDetail() {
    const p = procs().find((x) => x.pid === state.detailPid);
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
    $('a-reveal').disabled = !p.path;
    $('a-vt').disabled = !p.path;
    $('a-trust').hidden = !p.path || p.trusted || p.score === 0;
  }

  function closeDetail() {
    state.detailPid = null;
    $('detail').hidden = true;
  }

  App.register('processes', {
    init() {
      $('list').addEventListener('click', (e) => {
        const g = e.target.closest('[data-group]');
        if (g) {
          state.closed.has(g.dataset.group) ? state.closed.delete(g.dataset.group) : state.closed.add(g.dataset.group);
          return render();
        }
        if (e.target.closest('[data-more]')) {
          state.showAllCalm = true;
          return render();
        }
        const row = e.target.closest('[data-pid]');
        if (row) {
          state.detailPid = Number(row.dataset.pid);
          fillDetail();
          $('detail').hidden = false;
        }
      });
      document.querySelectorAll('#filter button').forEach((b) => b.addEventListener('click', () => {
        state.tab = b.dataset.tab;
        document.querySelectorAll('#filter button').forEach((x) => x.classList.toggle('on', x === b));
        render();
      }));
      document.querySelectorAll('.cols [data-sort]').forEach((b) => b.addEventListener('click', () => {
        state.sort = b.dataset.sort;
        render();
      }));
      $('sort-pill').addEventListener('click', () => {
        state.sort = 'risk';
        render();
      });

      $('d-close').addEventListener('click', closeDetail);
      $('detail').addEventListener('click', (e) => { if (e.target === $('detail')) closeDetail(); });
      document.addEventListener('keydown', (e) => { if (e.key === 'Escape' && state.detailPid != null) closeDetail(); });
      $('a-kill').addEventListener('click', async () => {
        const p = procs().find((x) => x.pid === state.detailPid);
        if (!p || !confirm(`End ${p.name}? Any unsaved work in it will be lost.`)) return;
        if (!(await core('kill', p.pid))) alert("Couldn't end the process. It may need admin rights.");
        App.tick();
      });
      $('a-reveal').addEventListener('click', () => core('reveal', state.detailPid));
      $('a-vt').addEventListener('click', () => core('virustotal', state.detailPid).catch((e) => alert(e.message)));
      $('a-trust').addEventListener('click', async () => {
        await core('trust', state.detailPid);
        App.tick();
      });
    },
    enter: render,
    refresh() {
      render();
      if (state.detailPid != null) fillDetail();
    },
    search(q) {
      state.query = q;
      render();
    },
  });
})();
