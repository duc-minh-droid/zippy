// JavaScript port of src/huffman.rs, used only for live typing in the page.
// It produces the same trace shape as `zippy stats --trace` and is checked
// against the Rust binary by web/verify.js (identical merges and codes on
// every sample). Node ids and the (weight, id) queue order match the Rust
// code exactly, so there are no ties and the tree is deterministic.
(function (root) {
  'use strict';

  // Minimal binary min-heap keyed on (weight, id).
  function less(a, b) { return a.weight < b.weight || (a.weight === b.weight && a.id < b.id); }
  class Heap {
    constructor() { this.a = []; }
    get size() { return this.a.length; }
    push(x) {
      const a = this.a; a.push(x);
      let i = a.length - 1;
      while (i > 0) {
        const p = (i - 1) >> 1;
        if (!less(a[i], a[p])) break;
        [a[i], a[p]] = [a[p], a[i]]; i = p;
      }
    }
    pop() {
      const a = this.a, top = a[0], last = a.pop();
      if (a.length) {
        a[0] = last;
        let i = 0;
        for (;;) {
          const l = 2 * i + 1, r = l + 1;
          let m = i;
          if (l < a.length && less(a[l], a[m])) m = l;
          if (r < a.length && less(a[r], a[m])) m = r;
          if (m === i) break;
          [a[i], a[m]] = [a[m], a[i]]; i = m;
        }
      }
      return top;
    }
  }

  function toBytes(input) {
    if (input instanceof Uint8Array) return input;
    return new TextEncoder().encode(String(input));
  }

  function trace(input, name) {
    const data = toBytes(input);
    const freq = new Array(256).fill(0);
    for (const b of data) freq[b]++;

    const heap = new Heap();
    const nodes = {};
    for (let s = 0; s < 256; s++) {
      if (freq[s] > 0) {
        const n = { id: s, weight: freq[s], sym: s, left: null, right: null };
        nodes[s] = n; heap.push(n);
      }
    }
    const merges = [];
    let nextId = 256;
    while (heap.size > 1) {
      const l = heap.pop(), r = heap.pop();
      const p = { id: nextId++, weight: l.weight + r.weight, sym: null, left: l.id, right: r.id };
      nodes[p.id] = p;
      merges.push({ step: merges.length, left: l.id, left_weight: l.weight, right: r.id,
        right_weight: r.weight, parent: p.id, weight: p.weight });
      heap.push(p);
    }
    const rootNode = heap.size ? heap.pop() : null;

    // Codes: left = 0, right = 1; a lone leaf gets "0".
    const codeOf = new Array(256).fill(null);
    if (rootNode) {
      if (rootNode.sym !== null) codeOf[rootNode.sym] = '0';
      else {
        const stack = [[rootNode.id, '']];
        while (stack.length) {
          const [id, path] = stack.pop();
          const n = nodes[id];
          if (n.sym !== null) { codeOf[n.sym] = path; continue; }
          stack.push([n.left, path + '0']);
          stack.push([n.right, path + '1']);
        }
      }
    }

    const frequencies = [], codes = [];
    let payloadBits = 0, entropy = 0, maxLen = 0, headerBytes = 5;
    const total = data.length;
    for (let s = 0; s < 256; s++) {
      if (!freq[s]) continue;
      frequencies.push({ sym: s, count: freq[s] });
      codes.push({ sym: s, count: freq[s], len: codeOf[s].length, code: codeOf[s] });
      payloadBits += freq[s] * codeOf[s].length;
      maxLen = Math.max(maxLen, codeOf[s].length);
      const p = freq[s] / total; entropy -= p * Math.log2(p);
      // header entry: 1 symbol byte + LEB128 varint count
      let v = freq[s], vb = 1; while (v >= 128) { v = Math.floor(v / 128); vb++; }
      headerBytes += 1 + vb;
    }

    const BITS_LIMIT = 8192;
    let bits = '';
    for (const b of data) { if (bits.length >= BITS_LIMIT) break; bits += codeOf[b]; }
    bits = bits.slice(0, BITS_LIMIT);
    let hex = '';
    for (let i = 0; i < bits.length; i += 8) {
      hex += parseInt(bits.slice(i, i + 8).padEnd(8, '0'), 2).toString(16).padStart(2, '0');
    }

    const payloadBytes = Math.ceil(payloadBits / 8);
    const compressed = headerBytes + payloadBytes;
    const TEXT_LIMIT = 4096;
    return {
      format: 'zippy-trace', version: 1, source: 'js',
      input: { name: name || 'typed', bytes: total, truncated: total > TEXT_LIMIT,
        text: new TextDecoder().decode(data.slice(0, TEXT_LIMIT)) },
      frequencies, merges, root: rootNode ? rootNode.id : null, codes,
      bitstream: { total_bits: payloadBits, sample_bits: bits, sample_bytes_hex: hex },
      stats: {
        original_bytes: total, distinct_symbols: frequencies.length, header_bytes: headerBytes,
        payload_bits: payloadBits, payload_bytes: payloadBytes, compressed_bytes: compressed,
        ratio: total ? compressed / total : 0, entropy_bits: entropy,
        avg_code_len: total ? payloadBits / total : 0, max_code_len: maxLen,
      },
    };
  }

  const api = { trace, toBytes };
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
  else root.ZippyJS = api;
})(typeof self !== 'undefined' ? self : this);
