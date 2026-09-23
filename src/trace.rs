//! Byte-accurate statistics and the `--trace` JSON dump used by the web
//! visualizer. JSON is written by hand to keep the crate dependency-free
//! apart from rayon.

use crate::huffman::{Code, Freq, Merge};
use std::fmt::Write;

pub struct Stats {
    pub original_bytes: u64,
    pub distinct_symbols: usize,
    pub header_bytes: u64,
    pub payload_bits: u64,
    pub payload_bytes: u64,
    pub compressed_bytes: u64,
    /// compressed / original (lower is better).
    pub ratio: f64,
    pub entropy_bits: f64,
    pub avg_code_len: f64,
    pub max_code_len: u8,
}

impl Stats {
    pub fn new(freq: &Freq, codes: &[Option<Code>; 256], header_bytes: usize, payload_bits: u64) -> Stats {
        let original_bytes: u64 = freq.iter().sum();
        let payload_bytes = payload_bits.div_ceil(8);
        let compressed_bytes = header_bytes as u64 + payload_bytes;
        let avg_code_len = if original_bytes == 0 {
            0.0
        } else {
            payload_bits as f64 / original_bytes as f64
        };
        Stats {
            original_bytes,
            distinct_symbols: freq.iter().filter(|&&c| c > 0).count(),
            header_bytes: header_bytes as u64,
            payload_bits,
            payload_bytes,
            compressed_bytes,
            ratio: if original_bytes == 0 { 0.0 } else { compressed_bytes as f64 / original_bytes as f64 },
            entropy_bits: crate::huffman::entropy(freq),
            avg_code_len,
            max_code_len: codes.iter().flatten().map(|c| c.len).max().unwrap_or(0),
        }
    }

    pub fn print(&self) {
        let pct = |a: u64| {
            if self.original_bytes == 0 { 0.0 } else { 100.0 * a as f64 / self.original_bytes as f64 }
        };
        println!("original        {:>12} B", self.original_bytes);
        println!("distinct bytes  {:>12}", self.distinct_symbols);
        println!("header          {:>12} B", self.header_bytes);
        println!("payload         {:>12} B  ({} bits)", self.payload_bytes, self.payload_bits);
        println!("compressed      {:>12} B  ({:.1}% of original)", self.compressed_bytes, pct(self.compressed_bytes));
        println!("payload only    {:>12.1} %  of original", pct(self.payload_bytes));
        println!("entropy H       {:>12.4} bits/byte", self.entropy_bits);
        println!("avg code len L  {:>12.4} bits/byte", self.avg_code_len);
        if self.avg_code_len > 0.0 {
            println!("efficiency H/L  {:>12.2} %", 100.0 * self.entropy_bits / self.avg_code_len);
        }
        println!("longest code    {:>12} bits", self.max_code_len);
    }

    fn write_json(&self, out: &mut String) {
        write!(
            out,
            "{{\"original_bytes\":{},\"distinct_symbols\":{},\"header_bytes\":{},\"payload_bits\":{},\
             \"payload_bytes\":{},\"compressed_bytes\":{},\"ratio\":{},\"entropy_bits\":{},\
             \"avg_code_len\":{},\"max_code_len\":{}}}",
            self.original_bytes,
            self.distinct_symbols,
            self.header_bytes,
            self.payload_bits,
            self.payload_bytes,
            self.compressed_bytes,
            num(self.ratio),
            num(self.entropy_bits),
            num(self.avg_code_len),
            self.max_code_len
        )
        .unwrap();
    }
}

fn num(x: f64) -> String {
    if x.is_finite() { format!("{:.6}", x) } else { "0".into() }
}

fn json_str(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => write!(out, "\\u{:04x}", c as u32).unwrap(),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// How much of the input text / bit stream to embed in the trace.
const TEXT_LIMIT: usize = 4096;
const BITS_LIMIT: usize = 8192;

pub struct TraceInput<'a> {
    pub name: &'a str,
    pub data: &'a [u8],
    pub freq: &'a Freq,
    pub merges: &'a [Merge],
    pub root: Option<u32>,
    pub codes: &'a [Option<Code>; 256],
    pub payload: &'a [u8],
    pub stats: &'a Stats,
}

pub fn to_json(t: &TraceInput) -> String {
    let mut o = String::new();
    o.push_str("{\"format\":\"zippy-trace\",\"version\":1,\"input\":{\"name\":");
    json_str(t.name, &mut o);
    let shown = &t.data[..t.data.len().min(TEXT_LIMIT)];
    write!(o, ",\"bytes\":{},\"truncated\":{},\"text\":", t.data.len(), shown.len() < t.data.len()).unwrap();
    json_str(&String::from_utf8_lossy(shown), &mut o);
    o.push_str("},");

    o.push_str("\"frequencies\":[");
    let mut first = true;
    for (s, &c) in t.freq.iter().enumerate() {
        if c == 0 {
            continue;
        }
        if !first {
            o.push(',');
        }
        first = false;
        write!(o, "{{\"sym\":{},\"count\":{}}}", s, c).unwrap();
    }
    o.push_str("],");

    // Merge steps in the order the priority queue produced them. Leaf ids are
    // byte values; internal ids are 256 + step.
    o.push_str("\"merges\":[");
    for (i, m) in t.merges.iter().enumerate() {
        if i > 0 {
            o.push(',');
        }
        write!(
            o,
            "{{\"step\":{},\"left\":{},\"left_weight\":{},\"right\":{},\"right_weight\":{},\"parent\":{},\"weight\":{}}}",
            i, m.left.0, m.left.1, m.right.0, m.right.1, m.parent.0, m.parent.1
        )
        .unwrap();
    }
    o.push_str("],");
    match t.root {
        Some(r) => write!(o, "\"root\":{},", r).unwrap(),
        None => o.push_str("\"root\":null,"),
    }

    o.push_str("\"codes\":[");
    let mut first = true;
    for (s, code) in t.codes.iter().enumerate() {
        let Some(code) = code else { continue };
        if !first {
            o.push(',');
        }
        first = false;
        write!(o, "{{\"sym\":{},\"count\":{},\"len\":{},\"code\":\"{}\"}}", s, t.freq[s], code.len, code.bit_string()).unwrap();
    }
    o.push_str("],");

    // A sample of the actual packed payload, as a bit string.
    let bits_shown = (t.stats.payload_bits as usize).min(BITS_LIMIT);
    let mut bits = String::with_capacity(bits_shown);
    for i in 0..bits_shown {
        let byte = t.payload[i / 8];
        bits.push(if (byte >> (7 - i % 8)) & 1 == 1 { '1' } else { '0' });
    }
    write!(o, "\"bitstream\":{{\"total_bits\":{},\"sample_bits\":\"{}\",\"sample_bytes_hex\":\"", t.stats.payload_bits, bits).unwrap();
    for b in &t.payload[..t.payload.len().min(BITS_LIMIT / 8)] {
        write!(o, "{:02x}", b).unwrap();
    }
    o.push_str("\"},");

    o.push_str("\"stats\":");
    t.stats.write_json(&mut o);
    o.push_str("}\n");
    o
}
