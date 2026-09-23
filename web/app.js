// zippy visualizer. Animates a trace produced by `zippy stats <file> --trace -`
// (bundled in traces.js) or, for typed / dropped input, by the JS port in
// huffman.js, which web/build-traces.js verifies against the Rust binary.
(function () {
  'use strict';
  const $ = (id) => document.getElementById(id);
  const SVGNS = 'http://www.w3.org/2000/svg';
  const esc = (s) => String(s).replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));

  // How a byte is drawn in labels.
  function lab(sym) {
    if (sym === 32) return '␣';
    if (sym === 10) return '↵';
    if (sym === 9) return '⇥';
    if (sym === 13) return '␍';
    if (sym > 32 && sym < 127) return String.fromCharCode(sym);
    return 'x' + sym.toString(16).toUpperCase().padStart(2, '0');
  }
  const bitsHtml = (code) => code.replace(/[01]/g, (b) => `<span class="bit${b}">${b}</span>`);
  const fmtPct = (x) => (100 * x).toFixed(1) + '%';

  // ---------------------------------------------------------------- model
  let M = null; // current model built from a trace
  let step = 0, playing = false, timer = null;

  function buildModel(trace, bytes, source) {
    const m = { trace, source };
    m.bytes = bytes || new TextEncoder().encode(trace.input.text);
    m.freq = {}; trace.frequencies.forEach((f) => (m.freq[f.sym] = f.count));
    m.code = {}; trace.codes.forEach((c) => (m.code[c.sym] = c.code));
    m.byCount = trace.codes.slice().sort((a, b) => b.count - a.count || a.sym - b.sym);
    // Nodes: leaves use the byte as id, internal nodes 256 + merge step (same as Rust).
    m.nodes = {};
    trace.frequencies.forEach((f) => (m.nodes[f.sym] = { id: f.sym, w: f.count, sym: f.sym, l: null, r: null }));
    trace.merges.forEach((g) => (m.nodes[g.parent] = { id: g.parent, w: g.weight, sym: null, l: g.left, r: g.right }));
    m.parent = {};
    trace.merges.forEach((g) => { m.parent[g.left] = g.parent; m.parent[g.right] = g.parent; });
    const height = (id) => { const n = m.nodes[id]; return n.sym !== null ? 0 : 1 + Math.max(height(n.l), height(n.r)); };
    m.height = {}; Object.keys(m.nodes).forEach((id) => (m.height[id] = height(+id)));
    m.maxH = trace.root === null ? 0 : m.height[trace.root];

    // Bit stream of the shown symbols. Its prefix must match the payload the binary wrote.
    m.shown = m.bytes.slice(0, 600);
    m.offsets = [0];
    for (const b of m.shown) m.offsets.push(m.offsets[m.offsets.length - 1] + m.code[b].length);
    const sample = trace.bitstream.sample_bits;
    const mine = Array.from(m.shown, (b) => m.code[b]).join('').slice(0, sample.length);
    m.bitsVerified = sample.startsWith(mine) || mine.startsWith(sample);

    // Timeline.
    const s = [];
    const n = m.bytes.length;
    if (n > 0) {
      const K = Math.min(n, 18);
      for (let i = 0; i < K; i++) s.push({ phase: 'count', frac: (i + 1) / K, dur: 110 });
      const mergeDur = Math.max(260, Math.min(1000, 16000 / Math.max(1, trace.merges.length)));
      for (let k = -1; k < trace.merges.length; k++) s.push({ phase: 'merge', k, dur: k < 0 ? 1200 : mergeDur });
      const C = Math.min(trace.codes.length, 14);
      for (let i = 0; i < C; i++) s.push({ phase: 'codes', upto: Math.ceil(trace.codes.length * (i + 1) / C), dur: 380 });
      const S = m.shown.length, E = Math.min(S, 44);
      for (let j = 0; j < E; j++) s.push({ phase: 'encode', upto: Math.ceil(S * (j + 1) / E), dur: Math.max(160, Math.min(520, 7000 / E)) });
    }
    s.push({ phase: 'done', dur: 0 });
    m.steps = s;
    return m;
  }

  // Forest after merges 0..k have been applied, as a list of root ids in queue order.
  function forestAt(k) {
    const alive = new Set(M.trace.frequencies.map((f) => f.sym));
    for (let i = 0; i <= k; i++) {
      const g = M.trace.merges[i];
      alive.delete(g.left); alive.delete(g.right); alive.add(g.parent);
    }
    return [...alive].sort((a, b) => M.nodes[a].w - M.nodes[b].w || a - b);
  }

  // ---------------------------------------------------------------- tree view
  const tree = { pos: {}, from: {}, to: {}, t0: 0, dur: 1, els: {}, raf: 0 };
  const treeSvg = $('tree');
  const gE = document.createElementNS(SVGNS, 'g'), gL = document.createElementNS(SVGNS, 'g'), gN = document.createElementNS(SVGNS, 'g');
  treeSvg.append(gE, gL, gN);
  const qLabel = document.createElementNS(SVGNS, 'text'); qLabel.setAttribute('class', 'qlabel'); treeSvg.append(qLabel);

  function layout(roots) {
    const W = treeSvg.clientWidth || 800, H = treeSvg.clientHeight || 330;
    treeSvg.setAttribute('viewBox', `0 0 ${W} ${H}`);
    const leaves = [];
    const walk = (id) => { const n = M.nodes[id]; if (n.sym !== null) leaves.push(id); else { walk(n.l); walk(n.r); } };
    roots.forEach(walk);
    const padX = 16, slot = (W - 2 * padX) / Math.max(1, leaves.length);
    const r = Math.max(5, Math.min(15, slot * 0.4));
    const top = 22, bottom = H - (r >= 9 ? 30 : 16);
    const lvl = M.maxH ? Math.min(90, (bottom - top) / M.maxH) : 0;
    const pos = {};
    leaves.forEach((id, i) => (pos[id] = { x: padX + (i + 0.5) * slot, y: bottom }));
    const place = (id) => {
      const n = M.nodes[id];
      if (n.sym !== null) return pos[id];
      const a = place(n.l), b = place(n.r);
      return (pos[id] = { x: (a.x + b.x) / 2, y: bottom - M.height[id] * lvl });
    };
    roots.forEach(place);
    return { pos, r, W, H };
  }

  function el(tag, cls) { const e = document.createElementNS(SVGNS, tag); if (cls) e.setAttribute('class', cls); return e; }

  function drawTree(st, animate) {
    const empty = !M.bytes.length;
    let roots = [];
    if (!empty && st.phase !== 'count') roots = st.phase === 'merge' ? forestAt(st.k) : forestAt(M.trace.merges.length - 1);
    if (!empty && st.phase === 'count' && st.frac === 1) roots = forestAt(-1);
    const L = layout(roots);
    const ids = new Set();
    const addTree = (id) => { ids.add(id); const n = M.nodes[id]; if (n.sym === null) { addTree(n.l); addTree(n.r); } };
    roots.forEach(addTree);

    // Remove stale elements.
    for (const id of Object.keys(tree.els)) {
      if (!ids.has(+id)) { const e = tree.els[id]; e.g.remove(); e.edge && e.edge.remove(); e.elab && e.elab.remove(); delete tree.els[id]; delete tree.pos[id]; }
    }
    // Highlights.
    const pop = new Set(), fresh = new Set(), path = new Set();
    let cur = null;
    if (st.phase === 'merge') {
      const next = M.trace.merges[st.k + 1];
      if (next) { pop.add(next.left); pop.add(next.right); }
      if (st.k >= 0) fresh.add(M.trace.merges[st.k].parent);
    }
    if (st.phase === 'codes') cur = M.byCount[st.upto - 1].sym;
    if (st.phase === 'encode') cur = M.shown[st.upto - 1];
    if (cur !== null) { let x = cur; while (x !== undefined) { path.add(x); x = M.parent[x]; } }
    const labelled = ['codes', 'encode', 'done'].includes(st.phase);

    for (const id of ids) {
      const n = M.nodes[id];
      let e = tree.els[id];
      if (!e) {
        e = tree.els[id] = { g: el('g'), c: el('circle'), t: el('text'), w: el('text', 'w') };
        e.g.append(e.c, e.t, e.w); gN.append(e.g);
        if (M.parent[id] !== undefined) { e.edge = el('line', 'edge'); gE.append(e.edge); e.elab = el('text', 'elabel'); gL.append(e.elab); }
        // New internal nodes grow out of the midpoint of their children.
        if (n.sym === null && tree.pos[n.l] && tree.pos[n.r]) {
          tree.pos[id] = { x: (tree.pos[n.l].x + tree.pos[n.r].x) / 2, y: Math.min(tree.pos[n.l].y, tree.pos[n.r].y) };
        }
      }
      const cls = ['n', n.sym !== null ? 'leaf' : 'inner'];
      if (pop.has(id)) cls.push('pop');
      if (fresh.has(id)) cls.push('new');
      if (id === cur) cls.push('cur');
      e.g.setAttribute('class', cls.join(' '));
      e.c.setAttribute('r', n.sym !== null ? L.r : Math.max(5, L.r * 0.85));
      e.t.textContent = n.sym !== null ? lab(n.sym) : (L.r >= 8 ? n.w : '');
      e.t.style.fontSize = Math.max(8, Math.min(12, L.r * 0.9)) + 'px';
      e.w.textContent = n.sym !== null && L.r >= 9 ? n.w : '';
      if (e.edge) {
        const p = M.parent[id];
        const on = ids.has(p);
        e.edge.style.display = on ? '' : 'none';
        e.elab.style.display = on && labelled && L.r >= 6 ? '' : 'none';
        e.edge.setAttribute('class', 'edge' + (path.has(id) ? ' path' : on ? ' on' : ''));
        const bit = M.nodes[p] && M.nodes[p].l === id ? '0' : '1';
        e.elab.textContent = bit; e.elab.setAttribute('class', 'elabel ' + (bit === '0' ? 'z' : 'o'));
      }
    }
    qLabel.setAttribute('x', 12); qLabel.setAttribute('y', 14);
    qLabel.textContent = roots.length === 1 && st.phase !== 'merge' && st.phase !== 'count'
      ? `Huffman tree: root weight ${M.nodes[roots[0]].w}, depth ${M.maxH}`
      : roots.length ? `queue: ${roots.length} node${roots.length > 1 ? 's' : ''}, lightest on the left` : (empty ? 'no input' : 'counting...');

    tree.to = L.pos; tree.r = L.r;
    tree.from = {};
    for (const id of ids) tree.from[id] = tree.pos[id] || L.pos[id];
    tree.t0 = performance.now(); tree.dur = animate ? Math.min(520, st.dur * 0.8 / speed()) : 0;
    cancelAnimationFrame(tree.raf);
    frame();
  }

  function frame() {
    const t = tree.dur ? Math.min(1, (performance.now() - tree.t0) / tree.dur) : 1;
    const e = t < 0.5 ? 2 * t * t : 1 - Math.pow(-2 * t + 2, 2) / 2;
    for (const id of Object.keys(tree.to)) {
      const a = tree.from[id] || tree.to[id], b = tree.to[id];
      tree.pos[id] = { x: a.x + (b.x - a.x) * e, y: a.y + (b.y - a.y) * e };
    }
    for (const [id, el_] of Object.entries(tree.els)) {
      const p = tree.pos[id]; if (!p) continue;
      el_.g.setAttribute('transform', `translate(${p.x.toFixed(1)},${p.y.toFixed(1)})`);
      el_.w.setAttribute('y', tree.r + 9);
      if (el_.edge) {
        const q = tree.pos[M.parent[id]];
        if (q) {
          el_.edge.setAttribute('x1', q.x); el_.edge.setAttribute('y1', q.y);
          el_.edge.setAttribute('x2', p.x); el_.edge.setAttribute('y2', p.y);
          const dx = p.x - q.x;
          el_.elab.setAttribute('x', (q.x + p.x) / 2 + (dx < 0 ? -8 : 8));
          el_.elab.setAttribute('y', (q.y + p.y) / 2);
        }
      }
    }
    if (t < 1) tree.raf = requestAnimationFrame(frame);
  }

  // ---------------------------------------------------------------- other panels
  function drawHist(st) {
    const hist = $('hist');
    const list = M.byCount;
    if (hist.childElementCount !== list.length || hist.dataset.key !== M.key) {
      hist.dataset.key = M.key;
      hist.innerHTML = list.map((c) => `<div class="b" data-s="${c.sym}"><u></u><i></i><s>${esc(lab(c.sym))}</s></div>`).join('');
    }
    const max = list.length ? list[0].count : 1;
    const frac = st.phase === 'count' ? st.frac : 1;
    const hot = new Set();
    if (st.phase === 'encode') hot.add(M.shown[st.upto - 1]);
    if (st.phase === 'codes') hot.add(M.byCount[st.upto - 1].sym);
    if (st.phase === 'merge') { const g = M.trace.merges[st.k + 1]; if (g) { if (g.left < 256) hot.add(g.left); if (g.right < 256) hot.add(g.right); } }
    const wide = hist.clientWidth / Math.max(1, list.length);
    [...hist.children].forEach((b, i) => {
      const c = list[i], v = Math.round(c.count * frac);
      b.querySelector('i').style.height = (70 * v / max) + 'px';
      const u = b.querySelector('u');
      u.textContent = wide >= 16 && v ? v : '';
      u.style.bottom = (16 + 70 * v / max) + 'px'; u.style.transform = 'none';
      b.querySelector('s').style.visibility = wide >= 9 ? '' : 'hidden';
      b.classList.toggle('hot', hot.has(c.sym));
    });
    const seen = list.filter((c) => Math.round(c.count * frac) > 0).length;
    $('histNote').textContent = `${seen} distinct byte value${seen === 1 ? '' : 's'}, sorted by count`;
  }

  function drawCodes(st) {
    const t = $('codes');
    if (t.dataset.key !== M.key) {
      t.dataset.key = M.key;
      t.innerHTML = '<tr><th>byte</th><th>count</th><th>code</th><th>bits</th></tr>' +
        M.byCount.map((c) => `<tr data-s="${c.sym}"><td>${esc(lab(c.sym))}</td><td class="n">${c.count}</td><td class="c">${bitsHtml(c.code)}</td><td class="n c">${c.len}</td></tr>`).join('');
    }
    const shown = st.phase === 'codes' ? st.upto : ['encode', 'done'].includes(st.phase) ? M.byCount.length : 0;
    let cur = null;
    if (st.phase === 'codes') cur = M.byCount[st.upto - 1].sym;
    if (st.phase === 'encode') cur = M.shown[st.upto - 1];
    const rows = t.querySelectorAll('tr[data-s]');
    rows.forEach((r, i) => {
      r.classList.toggle('hidden', i >= shown);
      const isCur = +r.dataset.s === cur;
      r.classList.toggle('cur', isCur);
      if (isCur) {
        const wrap = t.parentElement, top = r.offsetTop - wrap.clientHeight / 2;
        wrap.scrollTop = Math.max(0, top);
      }
    });
    if (st.phase !== 'codes' && st.phase !== 'encode') t.parentElement.scrollTop = 0;
  }

  function drawStream(st) {
    const S = M.shown.length;
    const upto = st.phase === 'encode' ? st.upto : st.phase === 'done' ? S : 0;
    const from = Math.max(0, upto - 90), to = Math.min(S, Math.max(upto, 0) + 70);
    let h = '';
    for (let i = from; i < to; i++) {
      const b = M.shown[i];
      const ch = b === 10 ? '↵' : b === 32 ? ' ' : b > 32 && b < 127 ? String.fromCharCode(b) : lab(b);
      const cls = i === upto - 1 && st.phase === 'encode' ? 'cur' : i < upto ? 'done' : '';
      h += `<span class="${cls}">${esc(ch)}</span>`;
    }
    $('streamText').innerHTML = h || '<span class="done">(empty)</span>';

    let g = '';
    for (let i = Math.max(0, upto - 48); i < upto; i++) {
      const cls = 'g' + (i % 2 ? ' alt' : '') + (i === upto - 1 && st.phase === 'encode' ? ' cur' : '');
      g += `<span class="${cls}">${bitsHtml(M.code[M.shown[i]])}</span>`;
    }
    $('streamBits').innerHTML = `<div>${g}</div>`;

    const bits = M.offsets[upto];
    const hex = M.trace.bitstream.sample_bytes_hex;
    const finished = st.phase === 'done' && upto === M.bytes.length;
    const full = Math.min(finished ? Math.ceil(bits / 8) : Math.floor(bits / 8), hex.length / 2);
    const first = Math.max(0, full - 28);
    let hx = '';
    for (let i = first; i < full; i++) hx += hex.slice(2 * i, 2 * i + 2) + ' ';
    $('streamHex').innerHTML = upto ? `packed bytes: ${first ? '... ' : ''}<b>${hx}</b>${finished ? (bits % 8 ? `(last byte padded with ${8 - bits % 8} zero bit${8 - bits % 8 > 1 ? 's' : ''})` : '') : bits % 8 ? `+ ${bits % 8} bit${bits % 8 > 1 ? 's' : ''} pending` : ''}` : 'packed bytes appear here, MSB first';
    const total = M.bytes.length;
    $('streamNote').textContent = upto
      ? `${upto} of ${total} byte${total === 1 ? '' : 's'} -> ${bits} bits` + (S < total && st.phase === 'done' ? ` (animation shows the first ${S})` : '')
      : '';
  }

  function drawGauge(st) {
    const s = M.trace.stats;
    const known = ['codes', 'encode', 'done'].includes(st.phase) && s.original_bytes > 0;
    let payloadRatio = known ? s.payload_bits / (8 * s.original_bytes) : 0;
    let label = known ? 'payload / original' : '';
    if (st.phase === 'encode') {
      payloadRatio = M.offsets[st.upto] / (8 * st.upto);
      label = `so far (${st.upto} B in)`;
    }
    const cx = 110, cy = 108, R = 88;
    const arc = (a0, a1) => {
      const p = (a) => [cx + R * Math.cos(Math.PI * (1 - a)), cy - R * Math.sin(Math.PI * (1 - a))];
      const [x0, y0] = p(a0), [x1, y1] = p(a1);
      return `M${x0.toFixed(1)},${y0.toFixed(1)} A${R},${R} 0 0 1 ${x1.toFixed(1)},${y1.toFixed(1)}`;
    };
    const f = Math.max(0.001, Math.min(1, payloadRatio));
    const withHeader = s.original_bytes ? s.compressed_bytes / s.original_bytes : 0;
    const hf = Math.min(1, withHeader);
    $('gauge').innerHTML = `
      <path d="${arc(0, 1)}" stroke="var(--panel-2)" stroke-width="16" fill="none" stroke-linecap="round"/>
      ${known ? `<path d="${arc(0, f)}" stroke="var(--one)" stroke-width="16" fill="none" stroke-linecap="round"/>` : ''}
      ${known && st.phase !== 'encode' ? `<line x1="${cx + (R - 12) * Math.cos(Math.PI * (1 - hf))}" y1="${cy - (R - 12) * Math.sin(Math.PI * (1 - hf))}" x2="${cx + (R + 12) * Math.cos(Math.PI * (1 - hf))}" y2="${cy - (R + 12) * Math.sin(Math.PI * (1 - hf))}" stroke="var(--hot)" stroke-width="2.5"/>` : ''}
      <text x="${cx}" y="${cy - 22}" text-anchor="middle" style="font:700 28px var(--mono);fill:var(--text)">${known ? fmtPct(payloadRatio) : '--'}</text>
      <text x="${cx}" y="${cy - 2}" text-anchor="middle" style="font:11px var(--sans);fill:var(--muted)">${label}</text>
      <text x="${cx - R}" y="${cy + 16}" text-anchor="middle" style="font:10px var(--mono);fill:var(--muted)">0%</text>
      <text x="${cx + R}" y="${cy + 16}" text-anchor="middle" style="font:10px var(--mono);fill:var(--muted)">100%</text>`;

    const rows = [
      ['original', s.original_bytes + ' B'],
      ['header', known ? s.header_bytes + ' B' : '--'],
      ['payload', known ? `${s.payload_bytes} B (${s.payload_bits} bits)` : '--'],
      ['total .zpy', known ? `${s.compressed_bytes} B = ${fmtPct(withHeader)}` : '--'],
    ];
    $('sizes').innerHTML = rows.map(([a, b]) => `<span>${a}</span><span${a === 'total .zpy' ? ' style="color:var(--hot)"' : ''}>${b}</span>`).join('');

    const hKnown = st.phase !== 'count' || st.frac === 1;
    const bars = [
      ['raw', 8, 'var(--muted)', true],
      ['avg code L', s.avg_code_len, 'var(--one)', known],
      ['entropy H', s.entropy_bits, 'var(--zero)', hKnown && s.original_bytes > 0],
    ];
    $('hl').innerHTML = bars.map(([n, v, c, on]) =>
      `<div class="row"><span>${n}</span><div class="bar"><i style="width:${on ? (100 * v / 8).toFixed(1) : 0}%;background:${c}"></i></div><span class="v">${on ? v.toFixed(3) : '--'}</span></div>`).join('') +
      (known && s.avg_code_len ? `<div class="muted small">H / L = ${(100 * s.entropy_bits / s.avg_code_len).toFixed(1)}% efficient. Huffman guarantees H &le; L &lt; H + 1.</div>` : '');
  }

  function caption(st) {
    const tr = M.trace, n = M.bytes.length;
    const nm = (id) => id < 256 ? `<em>${esc(lab(id))}</em>(${M.nodes[id].w})` : `[${M.nodes[id].w}]`;
    switch (st.phase) {
      case 'count': return `Counting bytes: <em>${Math.round(n * st.frac)}</em> of ${n} read.` + (st.frac === 1 ? ` Every distinct byte becomes a leaf in a min-priority queue keyed on (count, byte).` : '');
      case 'merge': {
        const next = tr.merges[st.k + 1];
        let s = '';
        if (st.k < 0) s = `Queue holds ${tr.frequencies.length} leaves.`;
        else { const g = tr.merges[st.k]; s = `Merge ${st.k + 1}/${tr.merges.length}: popped ${nm(g.left)} and ${nm(g.right)}, pushed [${g.weight}].`; }
        if (next) s += ` Next: pop the two lightest, ${nm(next.left)} and ${nm(next.right)}.`;
        else s += ` One tree left; its root weighs ${n}, the whole input.`;
        return s;
      }
      case 'codes': {
        const c = M.byCount[st.upto - 1];
        return `Read codes off the tree: left = <code>0</code>, right = <code>1</code>. <em>${esc(lab(c.sym))}</em> seen ${c.count}x gets ${bitsHtml(c.code)} (${c.len} bit${c.len > 1 ? 's' : ''}).`;
      }
      case 'encode': {
        const b = M.shown[st.upto - 1];
        return `Byte ${st.upto}/${n}: <em>${esc(lab(b))}</em> -> ${bitsHtml(M.code[b])}. ${M.offsets[st.upto]} bits so far vs ${8 * st.upto} raw.`;
      }
      default: {
        const s = tr.stats;
        if (!n) return 'Empty input: the .zpy file is just the 5-byte header.';
        return `Done. ${n} B -> ${s.payload_bytes} B payload + ${s.header_bytes} B header = ${s.compressed_bytes} B (${fmtPct(s.ratio)})` + (M.source.kind === 'rust' ? ', round-trip verified by the binary.' : '.');
      }
    }
  }

  // ---------------------------------------------------------------- player
  const speed = () => +$('speed').value;
  function render(animate = true) {
    const st = M.steps[step];
    const order = ['count', 'merge', 'codes', 'encode'];
    const pi = st.phase === 'done' ? 4 : order.indexOf(st.phase);
    document.querySelectorAll('#phases li').forEach((li, i) => {
      li.classList.toggle('active', i === pi);
      li.classList.toggle('done', i < pi);
    });
    $('scrub').max = M.steps.length - 1; $('scrub').value = step;
    $('caption').innerHTML = caption(st);
    drawHist(st); drawTree(st, animate); drawCodes(st); drawStream(st); drawGauge(st);
  }
  function go(i, animate = true) { step = Math.max(0, Math.min(M.steps.length - 1, i)); render(animate); }
  function tick() {
    if (!playing) return;
    if (step >= M.steps.length - 1) { setPlaying(false); return; }
    go(step + 1);
    timer = setTimeout(tick, M.steps[step].dur / speed());
  }
  function setPlaying(p) {
    playing = p; clearTimeout(timer);
    $('play').textContent = p ? 'Pause' : 'Play';
    if (p) {
      if (step >= M.steps.length - 1) go(0, false);
      timer = setTimeout(tick, M.steps[step].dur / speed());
    }
  }

  $('play').onclick = () => setPlaying(!playing);
  $('back').onclick = () => { setPlaying(false); go(step - 1); };
  $('fwd').onclick = () => { setPlaying(false); go(step + 1); };
  $('restart').onclick = () => { setPlaying(false); go(0, false); };
  $('end').onclick = () => { setPlaying(false); go(M.steps.length - 1, false); };
  $('scrub').oninput = (e) => { setPlaying(false); go(+e.target.value, false); };
  document.querySelectorAll('#phases li').forEach((li) => {
    li.onclick = () => {
      setPlaying(false);
      const p = li.dataset.phase;
      const i = M.steps.findIndex((s) => s.phase === p);
      if (i >= 0) go(i, false);
    };
  });
  document.addEventListener('keydown', (e) => {
    if (e.target.tagName === 'TEXTAREA' || e.target.tagName === 'INPUT') return;
    if (e.key === ' ') { e.preventDefault(); setPlaying(!playing); }
    else if (e.key === 'ArrowRight') $('fwd').click();
    else if (e.key === 'ArrowLeft') $('back').click();
    else if (e.key === 'Home') $('restart').click();
    else if (e.key === 'End') $('end').click();
  });
  window.addEventListener('resize', () => M && render(false));

  // ---------------------------------------------------------------- input
  let keySeq = 0;
  function load(trace, bytes, source, { autoplay = false, atEnd = false } = {}) {
    setPlaying(false);
    // Clear the tree so nodes from a different input do not animate in.
    for (const e of Object.values(tree.els)) { e.g.remove(); e.edge && e.edge.remove(); e.elab && e.elab.remove(); }
    tree.els = {}; tree.pos = {};
    M = buildModel(trace, bytes, source);
    M.key = String(++keySeq);
    const src = $('source');
    src.className = 'source ' + source.kind;
    src.innerHTML = source.kind === 'rust'
      ? `trace from <b>zippy stats ${esc(trace.input.name)} --trace -</b>`
      : `live <b>JS port</b> of src/huffman.rs (verified identical on all samples)` + (M.bitsVerified ? '' : ' <b style="color:var(--hot)">bit mismatch</b>');
    const s = trace.stats;
    $('inputInfo').textContent = `${s.original_bytes} bytes, ${s.distinct_symbols} distinct` + (trace.input.truncated ? ' (text preview truncated)' : '');
    go(atEnd ? M.steps.length - 1 : 0, false);
    if (autoplay) setPlaying(true);
  }

  const chips = $('samples');
  const traces = window.ZIPPY_TRACES || [];
  function selectChip(key) { chips.querySelectorAll('.chip').forEach((c) => c.classList.toggle('active', c.dataset.key === key)); }
  traces.forEach((t) => {
    const b = document.createElement('button');
    b.className = 'chip'; b.dataset.key = t.key; b.textContent = t.file.replace('samples/', '');
    b.onclick = () => {
      selectChip(t.key);
      $('text').value = t.trace.input.text;
      load(t.trace, null, { kind: 'rust' }, { autoplay: true });
    };
    chips.append(b);
  });

  let debounce = 0;
  $('text').addEventListener('input', () => {
    clearTimeout(debounce);
    debounce = setTimeout(() => {
      selectChip(null);
      const bytes = new TextEncoder().encode($('text').value);
      load(ZippyJS.trace(bytes, 'typed'), bytes, { kind: 'js' }, { atEnd: true });
    }, 180);
  });

  const drop = $('drop');
  ['dragenter', 'dragover'].forEach((ev) => drop.addEventListener(ev, (e) => { e.preventDefault(); drop.classList.add('over'); }));
  ['dragleave', 'drop'].forEach((ev) => drop.addEventListener(ev, (e) => { e.preventDefault(); drop.classList.remove('over'); }));
  drop.addEventListener('drop', async (e) => {
    const f = e.dataTransfer.files[0];
    if (!f) return;
    const bytes = new Uint8Array(await f.arrayBuffer());
    selectChip(null);
    $('text').value = new TextDecoder().decode(bytes.slice(0, 4096));
    load(ZippyJS.trace(bytes, f.name), bytes, { kind: 'js' }, { autoplay: true });
  });

  // Start with a small sample from the real binary.
  const first = traces.find((t) => t.key === 'abracadabra') || traces[0];
  if (first) { selectChip(first.key); $('text').value = first.trace.input.text; load(first.trace, null, { kind: 'rust' }, { autoplay: true }); }
  else { const b = new TextEncoder().encode('abracadabra'); $('text').value = 'abracadabra'; load(ZippyJS.trace(b), b, { kind: 'js' }); }

  window.__zippy = { go: (i) => go(i, false), steps: () => M.steps, setPlaying };
})();
