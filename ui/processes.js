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
      : `<div class="reason"><div><div class="rt">Nothing suspicious</div><div class="rx">None of the WinGlance rules matched.</div></div></div>`;
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

  function openDetail(p) {
    state.detailPid = p.pid;
    $('detail').hidden = false;
    fillDetail();
    if (!$('detail').hidden) $('d-close').focus();
  }

  async function processAction(cmd, p, failure) {
    if (!p) return;
    if (!(await core(cmd, p.pid))) throw new Error(failure);
    if (cmd === 'kill' || cmd === 'trust') App.tick();
  }

  async function endTask(p) {
    if (p && confirm(`End ${p.name}? Any unsaved work in it will be lost.`)) {
      await processAction('kill', p, "Couldn't end the process. It may have exited, be protected, or require administrator rights.");
    }
  }

  const priorityClasses = [
    ['high', 'High'], ['above', 'Above normal'], ['normal', 'Normal'],
    ['below', 'Below normal'], ['low', 'Low'],
  ];

  function processMenu(p, controls) {
    const unavailable = controls ? 'This process may be protected or require administrator rights.' : 'Checking process permissions…';
    const pathHint = p.path ? '' : 'The file path is unavailable.';
    return [
      { label: 'Open file location', disabled: !p.path, hint: pathHint, action: () => processAction('reveal', p, "Couldn't open the file location.") },
      { label: 'End task…', danger: true, disabled: !controls?.can_end, hint: controls?.can_end ? '' : unavailable, action: () => endTask(p) },
      {
        label: 'Set priority', disabled: !controls?.can_set,
        hint: !controls?.can_set ? unavailable : controls.current === 'realtime' ? 'Current priority: Realtime' : '',
        items: priorityClasses.map(([key, label]) => ({
          label, checked: controls?.current === key,
          action: () => core('priority', p.pid, key),
        })),
      },
      { label: 'Copy path', separator: true, disabled: !p.path, hint: pathHint, action: () => copyText(p.path) },
      { label: 'Check on VirusTotal', disabled: !p.path, hint: pathHint, action: () => processAction('virustotal', p, "Couldn't open VirusTotal.") },
      { label: 'Trust it', disabled: !p.path || p.trusted, hint: pathHint || (p.trusted ? 'Already trusted.' : ''), action: () => processAction('trust', p, "Couldn't trust the process.") },
      { label: 'Details', action: () => openDetail(p) },
    ];
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
        const p = row && procs().find((x) => x.pid === Number(row.dataset.pid));
        if (p) openDetail(p);
      });
      $('list').addEventListener('contextmenu', async (event) => {
        const row = event.target.closest('[data-pid]');
        const p = row && procs().find((x) => x.pid === Number(row.dataset.pid));
        if (!p) return;
        const update = ContextMenu.open(event, row, `[data-pid="${p.pid}"]`, processMenu(p));
        try {
          update(processMenu(p, await core('priority', p.pid)));
        } catch (error) {
          if (update(processMenu(p, {}))) reportActionError(error);
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
      const current = () => procs().find((p) => p.pid === state.detailPid);
      $('a-kill').addEventListener('click', () => runAction(() => endTask(current())));
      $('a-reveal').addEventListener('click', () => runAction(() => processAction('reveal', current(), "Couldn't open the file location.")));
      $('a-vt').addEventListener('click', () => runAction(() => processAction('virustotal', current(), "Couldn't open VirusTotal.")));
      $('a-trust').addEventListener('click', () => runAction(() => processAction('trust', current(), "Couldn't trust the process.")));
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
