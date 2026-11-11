(() => {
  let last = null;

  // Top consumers grouped by name, so ten chrome.exe count as one.
  function topBy(key, n) {
    const sums = new Map();
    for (const p of App.procs) sums.set(p.name, (sums.get(p.name) || 0) + p[key]);
    const all = [...sums].sort((a, b) => b[1] - a[1]);
    return all.slice(0, n);
  }

  function spark(hist, max, colorVar, h, fill) {
    if (!hist.length) return '';
    const pts = hist.map((v, i) => [hist.length === 1 ? 380 : (i / (hist.length - 1)) * 380, h - (Math.min(v, max) / (max || 1)) * (h - 4)]);
    const line = pts.map(([x, y], i) => `${i ? 'L' : 'M'}${x.toFixed(1)},${y.toFixed(1)}`).join(' ');
    const area = fill ? `<path d="${line} L380,${h} L${pts[0][0].toFixed(1)},${h} Z" style="fill:var(${colorVar});stroke:none" opacity="0.16"/>` : '';
    return `${area}<path d="${line}" style="fill:none;stroke:var(${colorVar})" stroke-width="2" vector-effect="non-scaling-stroke"/>`;
  }

  function sparkSvg(inner, h, label) {
    return `<svg class="spark" viewBox="0 0 380 ${h}" preserveAspectRatio="none" role="img" aria-label="${label}" style="height:${h}px">
      <line x1="0" y1="${h - 0.5}" x2="380" y2="${h - 0.5}" style="stroke:var(--axis)" stroke-width="1" vector-effect="non-scaling-stroke"/>${inner}</svg>`;
  }

  function networkStats(label, color, history) {
    const average = history.length ? fmtRate(history.reduce((sum, rate) => sum + rate, 0) / history.length) : '—';
    const peak = history.length ? fmtRate(Math.max(...history)) : '—';
    return `<tr>
      <th scope="row"><span class="key"><i style="background:var(${color})"></i>${label}</span></th>
      <td class="num">${average}</td><td class="num">${peak}</td>
    </tr>`;
  }

  function gaugeCard(label, model, value, extra, hist, footer) {
    return `<section class="card">
      <div class="card-h"><h2>${label}</h2><span class="model">${esc(model)}</span></div>
      <div class="big"><span>${value}</span><small>${extra}</small></div>
      ${sparkSvg(spark(hist, 100, '--c1', 50, true), 50, `${label} load over the last 60 seconds`)}
      ${footer}
    </section>`;
  }

  // parts: [{name, value, label, color|null}] where null is the empty remainder.
  function donutCard(title, parts, total, center, centerSub, aria) {
    const r = 52, C = 2 * Math.PI * r;
    let start = 0;
    const segs = parts.filter((p) => p.color).map((p) => {
      const len = (p.value / total) * C;
      const s = `<circle cx="66" cy="66" r="${r}" style="fill:none;stroke:var(${p.color})" stroke-width="16"
        stroke-dasharray="${Math.max(len - 2, 0).toFixed(2)} ${C.toFixed(2)}" stroke-dashoffset="${(-start).toFixed(2)}"
        transform="rotate(-90 66 66)"><title>${esc(p.name)}: ${p.label}</title></circle>`;
      start += len;
      return s;
    }).join('');
    const legend = parts.map((p) => `<div class="lg">
      <span class="sw" style="background:${p.color ? `var(${p.color})` : 'var(--track)'};${p.color ? '' : 'box-shadow: inset 0 0 0 1px var(--axis)'}"></span>
      <span class="lg-name ${p.color ? '' : 'dim'}">${esc(p.name)}</span><span class="lg-val">${p.label}</span>
    </div>`).join('');
    return `<section class="card">
      <div class="card-h"><h2>${title}</h2></div>
      <div class="donut-row">
        <div class="donut">
          <svg viewBox="0 0 132 132" width="132" height="132" role="img" aria-label="${aria}">
            <circle cx="66" cy="66" r="${r}" style="fill:none;stroke:var(--track)" stroke-width="16"/>${segs}
          </svg>
          <div class="donut-c"><b>${center}</b><small>${centerSub}</small></div>
        </div>
        <div class="legend">${legend}</div>
      </div>
    </section>`;
  }

  function render() {
    const d = last;
    if (!d) {
      $('hw').innerHTML = `<div class="empty">Collecting data…</div>`;
      return;
    }
    const { info, live } = d;
    const ghz = info.cpu_mhz ? `${(info.cpu_mhz / 1000).toFixed(1)} GHz base` : '';
    const cores = live.cores.length
      ? `<div class="cores" aria-label="Per-thread load">${live.cores.map((v, i) => `<div class="core" title="Thread ${i + 1}: ${Math.round(v)}%"><i style="height:${v}%"></i></div>`).join('')}</div>`
      : '';
    const cpu = gaugeCard('CPU', `${info.cpu_name} · ${info.threads} threads`, `${Math.round(live.cpu)}%`, ghz, live.cpu_hist, cores);

    const vramPct = info.vram_total ? (live.vram_used / info.vram_total) * 100 : 0;
    const vram = info.vram_total
      ? `<div class="vram"><span class="dim">VRAM</span><span class="meter wide"><i style="width:${vramPct}%"></i></span><span class="num">${fmtBytes(live.vram_used)} of ${fmtBytes(info.vram_total)}</span></div>`
      : '';
    const gpu = info.gpu_name
      ? gaugeCard('GPU', info.gpu_name, `${Math.round(live.gpu)}%`, 'busiest engine', live.gpu_hist, vram)
      : `<section class="card"><div class="card-h"><h2>GPU</h2></div><div class="empty">No GPU data</div></section>`;

    const colors = ['--c1', '--c2', '--c3'];
    const busy = Math.min(live.cpu, 100);
    const cpuTop = topBy('cpu', 3);
    const memTop = topBy('mem', 3);
    const topCpu = cpuTop.map(([, v]) => v).reduce((a, b) => a + b, 0);
    const scale = topCpu > busy && topCpu > 0 ? busy / topCpu : 1;
    const cpuParts = [
      ...cpuTop.map(([name, v], i) => ({ name, value: v * scale, label: fmtCpu(v * scale), color: colors[i] })),
      { name: 'Other apps', value: Math.max(busy - topCpu * scale, 0), label: fmtCpu(Math.max(busy - topCpu * scale, 0)), color: '--other' },
      { name: 'Idle', value: 100 - busy, label: fmtCpu(100 - busy), color: null },
    ];
    const topMem = memTop.map(([, v]) => v).reduce((a, b) => a + b, 0);
    const memParts = [
      ...memTop.map(([name, v], i) => ({ name, value: v, label: fmtBytes(v), color: colors[i] })),
      { name: 'Other apps', value: Math.max(d.mem_used - topMem, 0), label: fmtBytes(Math.max(d.mem_used - topMem, 0)), color: '--other' },
      { name: 'Free', value: d.mem_total - d.mem_used, label: fmtBytes(d.mem_total - d.mem_used), color: null },
    ];

    const disks = d.disks.map((k) => {
      const drive = esc(k.root.replace('\\', ''));
      const free = Math.min(k.free, k.total);
      const used = k.total - free;
      const percent = k.total ? (used / k.total) * 100 : 0;
      const usedLabel = used ? fmtBytes(used) : '0 B';
      const freeLabel = free ? fmtBytes(free) : '0 B';
      return `<div class="disk">
        <div class="disk-h"><b>${drive}</b><span class="dim disk-label">${esc(k.label || 'Local disk')}</span><span class="spacer"></span>${k.total ? `<span class="num disk-size">${usedLabel} of ${fmtBytes(k.total)}</span>` : ''}</div>
        ${k.total ? `
          <span class="meter tall" role="meter" aria-label="${drive} disk space used" aria-valuemin="0" aria-valuemax="100" aria-valuenow="${percent}"><i style="width:${percent}%"></i></span>
          <div class="disk-meta dim small num"><span>${freeLabel} free</span><span>${Math.round(percent)}% used</span></div>
        ` : '<div class="dim small">Capacity unavailable</div>'}
      </div>`;
    }).join('');

    const netMax = Math.max(1, ...live.down_hist, ...live.up_hist);
    const net = `<section class="card">
      <div class="card-h"><h2>Network</h2><span class="spacer"></span>
        <span class="key"><i style="background:var(--c1)"></i>Down <b class="num">${fmtRate(live.net_down)}</b></span>
        <span class="key"><i style="background:var(--c2)"></i>Up <b class="num">${fmtRate(live.net_up)}</b></span>
      </div>
      ${sparkSvg(spark(live.down_hist, netMax, '--c1', 70, false) + spark(live.up_hist, netMax, '--c2', 70, false), 70, 'Download and upload over the last 60 seconds')}
      <table class="network-stats" aria-label="Network speeds in the visible graph">
        <thead><tr><th scope="col">In view</th><th scope="col">Average</th><th scope="col">Peak</th></tr></thead>
        <tbody>
          ${networkStats('Download', '--c1', live.down_hist)}
          ${networkStats('Upload', '--c2', live.up_hist)}
        </tbody>
      </table>
    </section>`;

    $('hw').innerHTML = cpu + gpu
      + donutCard("Who's using the CPU", cpuParts, 100, `${Math.round(busy)}%`, 'busy', 'CPU share by app')
      + donutCard("What's in memory", memParts, d.mem_total, fmtBytes(d.mem_used), `of ${fmtBytes(d.mem_total)}`, 'Memory share by app')
      + `<section class="card"><div class="card-h"><h2>Disks</h2></div>${disks}<div class="dim small num io">Read ${fmtRate(live.disk_read)} · Write ${fmtRate(live.disk_write)}</div></section>`
      + net;
  }

  App.register('hardware', {
    enter() {
      render();
      this.refresh();
    },
    async refresh() {
      try {
        last = await invoke('hardware');
      } catch (e) {
        console.error(e);
      }
      render();
    },
    repaint: render,
  });
})();
