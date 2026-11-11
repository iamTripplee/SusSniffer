(() => {
  const CALM_LIMIT = 40;
  const sortLabels = { cpu: 'CPU', mem: 'Memory', risk: 'Risk' };
  const state = {
    mode: 'running', tab: 'all', query: '', sort: 'risk', ascending: false, closed: new Set(),
    showAllCalm: false, detail: null, recent: null, recentPending: false, recentError: false,
  };
  const procs = () => App.procs;
  const displayName = (p) => p.display_name || p.name;
  const exitTime = new Intl.DateTimeFormat('en', { month: 'short', day: 'numeric', hour: 'numeric', minute: '2-digit', second: '2-digit' });
  const icons = new Map();
  let iconQueue = Promise.resolve();

  function iconImage(src) {
    return `<img src="${esc(src)}" alt="">`;
  }

  function appIcon(path) {
    if (!path) return null;
    if (icons.has(path)) return icons.get(path).src;
    if (icons.size >= 256) {
      const active = new Set(procs().filter((p) => p.is_app).map((p) => p.path));
      const unused = [...icons.keys()].find((key) => !active.has(key));
      if (!unused) return null;
      icons.delete(unused);
    }
    const entry = { src: null };
    icons.set(path, entry);
    iconQueue = iconQueue.then(async () => {
      if (icons.get(path) !== entry) return;
      try {
        entry.src = await invoke('app_icon', { path });
      } catch {
        return;
      }
      if (!entry.src || icons.get(path) !== entry) return;
      for (const tile of document.querySelectorAll('[data-icon-path]')) {
        if (tile.dataset.iconPath === path) {
          tile.classList.add('has-icon');
          tile.innerHTML = iconImage(entry.src);
        }
      }
    });
    return null;
  }

  function updateThreatCount(count, danger) {
    const badge = $('threat-count');
    badge.hidden = count === 0;
    badge.textContent = count;
    badge.classList.toggle('high', danger);
  }

  function whoHtml(p) {
    const name = displayName(p);
    const reason = p.reasons[0];
    const sub = reason
      ? `<span class="sub" style="color:${toneVar[p.level]}">${esc(reason.title)}</span>`
      : `<span class="sub mono">${esc(p.display_name ? p.name : p.path || 'Path unavailable')}</span>`;
    const status = p.is_app && p.not_responding ? '<span class="app-status">Not responding</span>' : '';
    const iconPath = p.is_app ? p.path : null;
    const src = appIcon(iconPath);
    return `<span class="who">
      <span class="tile${src ? ' has-icon' : ''}" ${iconPath ? `data-icon-path="${esc(iconPath)}"` : ''} style="--tile-bg:${tileColor(name)}" aria-hidden="true">${src ? iconImage(src) : esc(name[0]?.toUpperCase() || '?')}</span>
      <span class="names"><span class="name-line"><span class="nm">${esc(name)}</span>${status}</span>${sub}</span>
    </span>`;
  }

  function rowHtml(p, maxCpu, maxMem) {
    const meter = (v, max) => `<span class="meter"><i style="width:${max ? (v / max) * 100 : 0}%"></i></span>`;
    return `<button class="row row-grid ${p.level === 'danger' ? 'danger' : ''}" data-pid="${p.pid}">
      ${whoHtml(p)}
      <span class="cell r ${state.sort === 'cpu' ? 'strong' : ''}">${fmtCpu(p.cpu)}${state.sort === 'cpu' ? meter(p.cpu, maxCpu) : ''}</span>
      <span class="cell r ${state.sort === 'mem' ? 'strong' : 'dim'}">${fmtBytes(p.mem)}${state.sort === 'mem' ? meter(p.mem, maxMem) : ''}</span>
      ${riskHtml(p.score, p.level)}
    </button>`;
  }

  function recentRowHtml(p) {
    return `<button class="row recent-grid ${p.level === 'danger' ? 'danger' : ''}" data-recent-id="${p.id}">
      ${whoHtml(p)}
      <span class="cell r dim">${exitTime.format(p.ended_at)}</span>
      ${riskHtml(p.score, p.level)}
    </button>`;
  }

  function visibleRows(rows, q) {
    return rows.filter((p) =>
      (state.tab === 'all' || p.level !== 'calm') &&
      (!q || p.name.toLowerCase().includes(q) || displayName(p).toLowerCase().includes(q)));
  }

  function emptyList(q, fallback) {
    return `<div class="empty">${q ? 'Nothing found' : state.tab === 'threats' ? 'No suspicious processes found' : fallback}</div>`;
  }

  function render() {
    const q = state.query.trim().toLowerCase();
    const recent = state.mode === 'recent';
    $('running-cols').hidden = recent;
    $('recent-cols').hidden = !recent;
    $('sort-pill').hidden = recent || (state.sort === 'risk' && !state.ascending);
    if (recent) {
      const history = state.recent || [];
      const rows = visibleRows(history, q);
      const flagged = history.filter((p) => p.level !== 'calm').length;
      updateThreatCount(flagged, history.some((p) => p.level === 'danger'));
      $('headline').textContent = flagged ? `${plural(flagged, 'process')} worth a look` : 'Process history';
      $('sub').textContent = history.length ? `${plural(history.length, 'process')} seen closing` : '';
      $('list').innerHTML = rows.length ? rows.map(recentRowHtml).join('')
        : !state.recent ? `<div class="empty">${state.recentError ? "Couldn't load history. Retrying…" : 'Loading…'}</div>`
        : emptyList(q, 'No recently closed processes');
      return;
    }

    const all = procs();
    const visible = App.hasAppList ? visibleRows(all, q) : [];
    const danger = all.filter((p) => p.level === 'danger').length;
    const warn = all.filter((p) => p.level === 'warn').length;
    updateThreatCount(danger + warn, danger > 0);
    $('headline').textContent = danger
      ? plural(danger, 'suspicious process')
      : warn ? `${plural(warn, 'process')} worth a look` : all.length ? 'All clear' : 'Scanning…';
    $('sub').textContent = `Watching ${plural(all.length, 'process')}`;

    for (const button of $('running-cols').querySelectorAll('[data-sort]')) {
      const active = button.dataset.sort === state.sort;
      const label = sortLabels[button.dataset.sort];
      const direction = state.ascending ? 'ascending' : 'descending';
      button.classList.toggle('on', active);
      button.classList.toggle('ascending', active && state.ascending);
      button.setAttribute('aria-label', active ? `${label}, sorted ${direction}. Reverse sort order` : `Sort by ${label}`);
    }
    $('sort-pill').innerHTML = `Sort by ${sortLabels[state.sort]} <svg width="10" height="10" viewBox="0 0 12 12" aria-hidden="true"><path d="M3 3l6 6M9 3l-6 6"/></svg>`;

    if (!visible.length) {
      $('list').innerHTML = !App.hasAppList ? '<div class="empty">Waiting for process data…</div>'
        : emptyList(q, 'No running processes found');
      return;
    }

    const key = state.sort === 'risk' ? 'score' : state.sort;
    const direction = state.ascending ? 1 : -1;
    visible.sort((a, b) => direction * (a[key] - b[key]));
    const maxCpu = key === 'cpu' ? visible.reduce((max, p) => Math.max(max, p.cpu), 0) : 0;
    const maxMem = key === 'mem' ? visible.reduce((max, p) => Math.max(max, p.mem), 0) : 0;

    const groups = [
      { id: 'attention', title: 'Needs attention', items: visible.filter((p) => p.level !== 'calm') },
      { id: 'apps', title: 'Apps', items: visible.filter((p) => p.level === 'calm' && p.is_app) },
      { id: 'background', title: 'Background processes', items: visible.filter((p) => p.level === 'calm' && !p.is_app) },
    ].filter((g) => g.items.length);

    $('list').innerHTML = groups.map((g) => {
      const open = !state.closed.has(g.id);
      let items = g.items;
      let more = '';
      if (g.id === 'background' && !state.showAllCalm && !q && items.length > CALM_LIMIT) {
        more = `<button class="more" data-more>Show all ${items.length} background processes</button>`;
        items = items.slice(0, CALM_LIMIT);
      }
      return `<section>
        <button class="group-h ${open ? 'open' : ''}" data-group="${g.id}" aria-expanded="${open}">
          <svg class="chev" viewBox="0 0 10 10" aria-hidden="true"><path d="M3 2l4 3-4 3"/></svg>
          <span class="t">${g.title}</span><span class="c">${g.items.length}</span>
        </button>
        ${open ? items.map((p) => rowHtml(p, maxCpu, maxMem)).join('') + more : ''}
      </section>`;
    }).join('');
  }

  async function refreshRecent() {
    if (state.recentPending) return;
    state.recentPending = true;
    try {
      const revision = state.recent ? String(state.recent[0]?.id ?? 0) : null;
      const rows = await core('recent', null, revision);
      if (rows === null) return;
      state.recent = rows;
      state.recentError = false;
    } catch (error) {
      console.error(error);
      if (state.recentError || state.recent) return;
      state.recentError = true;
    } finally {
      state.recentPending = false;
    }
    if (App.current === 'processes' && state.mode === 'recent') render();
  }

  const levelText = { danger: 'High risk', warn: 'Medium risk', calm: 'Low risk' };
  const currentLive = () => procs().find((p) => p.pid === state.detail?.pid);

  function fillDetail() {
    const record = state.detail?.record;
    const p = record || currentLive();
    if (!p) {
      closeDetail();
      return;
    }
    const name = displayName(p);
    const tile = $('d-tile');
    const iconPath = p.is_app ? p.path : null;
    const src = appIcon(iconPath);
    if (iconPath) tile.dataset.iconPath = iconPath;
    else delete tile.dataset.iconPath;
    tile.style.setProperty('--tile-bg', tileColor(name));
    tile.classList.toggle('has-icon', !!src);
    if (src) {
      if (tile.firstElementChild?.getAttribute('src') !== src) tile.innerHTML = iconImage(src);
    } else {
      tile.textContent = name[0]?.toUpperCase() || '?';
    }
    $('d-name').textContent = name;
    $('d-meta').textContent = `PID ${p.pid}${record ? ` · Seen closed ${exitTime.format(p.ended_at)}` : ''}${p.trusted ? ' · trusted by you' : ''}`;
    $('d-score').textContent = p.score;
    $('d-score').style.color = toneVar[p.level];
    $('d-level').textContent = levelText[p.level];
    $('d-level').style.color = toneVar[p.level];
    $('d-why-label').textContent = p.reasons.length ? 'Why it looks suspicious' : 'Checks';
    $('d-reasons').innerHTML = p.reasons.length
      ? p.reasons.map((r) => `<div class="reason"><div class="pts">+${r.pts}</div><div><div class="rt">${esc(r.title)}</div><div class="rx">${esc(r.text)}</div></div></div>`).join('')
      : `<div class="reason"><div><div class="rt">Nothing suspicious</div><div class="rx">None of the WinGlance rules matched.</div></div></div>`;
    $('d-file').textContent = p.name;
    $('d-path').textContent = p.path || 'Unavailable';
    $('d-parent').textContent = p.parent_name ? `${p.parent_name} (PID ${p.ppid})` : 'exited or unknown';
    $('d-sign').textContent = p.signed == null ? 'not checked' : p.signed ? 'valid' : 'missing';
    $('d-actions').hidden = !!record;
    $('a-reveal').disabled = !p.path;
    $('a-vt').disabled = !p.path;
    $('a-trust').hidden = !p.path || p.trusted || p.score === 0;
  }

  function closeDetail() {
    state.detail = null;
    $('detail').hidden = true;
  }

  function openDetail(p, recent = false) {
    state.detail = recent ? { record: p } : { pid: p.pid };
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
    if (!p) return;
    const label = displayName(p) === p.name ? p.name : `${displayName(p)} (${p.name})`;
    if (confirm(`End ${label}? Any unsaved work in it will be lost.`)) {
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

  function selectButton(group, attribute, value) {
    group.querySelectorAll('button').forEach((button) => {
      const selected = button.dataset[attribute] === value;
      button.classList.toggle('on', selected);
      button.setAttribute('aria-pressed', selected);
    });
  }

  function recentFromRow(row) {
    return row && state.recent?.find((p) => p.id === Number(row.dataset.recentId));
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
        const record = recentFromRow(e.target.closest('[data-recent-id]'));
        if (record) return openDetail(record, true);
        const row = e.target.closest('[data-pid]');
        const p = row && procs().find((x) => x.pid === Number(row.dataset.pid));
        if (p) openDetail(p);
      });
      $('list').addEventListener('contextmenu', async (event) => {
        const recentRow = event.target.closest('[data-recent-id]');
        const record = recentFromRow(recentRow);
        if (record) {
          ContextMenu.open(event, recentRow, `[data-recent-id="${record.id}"]`, [
            { label: 'Details', action: () => openDetail(record, true) },
            { label: 'Copy path', disabled: !record.path, action: () => copyText(record.path) },
          ]);
          return;
        }
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
        selectButton($('filter'), 'tab', state.tab);
        render();
      }));
      document.querySelectorAll('#process-mode button').forEach((b) => b.addEventListener('click', () => {
        if (state.mode === b.dataset.mode) return;
        state.mode = b.dataset.mode;
        selectButton($('process-mode'), 'mode', state.mode);
        ContextMenu.close();
        closeDetail();
        $('list').scrollTop = 0;
        render();
        if (state.mode === 'recent') refreshRecent();
        else App.tick();
      }));
      $('running-cols').querySelectorAll('[data-sort]').forEach((b) => b.addEventListener('click', () => {
        state.ascending = state.sort === b.dataset.sort && !state.ascending;
        state.sort = b.dataset.sort;
        render();
      }));
      $('sort-pill').addEventListener('click', () => {
        state.sort = 'risk';
        state.ascending = false;
        render();
      });

      $('d-close').addEventListener('click', closeDetail);
      $('detail').addEventListener('click', (e) => { if (e.target === $('detail')) closeDetail(); });
      document.addEventListener('keydown', (e) => { if (e.key === 'Escape' && state.detail) closeDetail(); });
      $('a-kill').addEventListener('click', () => runAction(() => endTask(currentLive())));
      $('a-reveal').addEventListener('click', () => runAction(() => processAction('reveal', currentLive(), "Couldn't open the file location.")));
      $('a-vt').addEventListener('click', () => runAction(() => processAction('virustotal', currentLive(), "Couldn't open VirusTotal.")));
      $('a-trust').addEventListener('click', () => runAction(() => processAction('trust', currentLive(), "Couldn't trust the process.")));
    },
    enter() {
      state.query = '';
      render();
      if (state.mode === 'recent') refreshRecent();
    },
    snapshotArg: () => state.mode === 'running' ? 'apps' : 'recent',
    refresh() {
      if (state.mode === 'recent') return refreshRecent();
      render();
      if (state.detail) fillDetail();
    },
    search(q) {
      state.query = q;
      render();
    },
  });
})();
