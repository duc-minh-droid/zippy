//! Core Huffman coding: byte frequencies, tree construction, code table,
//! bit packing and decoding.
//!
//! The tree build is fully deterministic. Every node gets an id (leaves use
//! their byte value 0..=255, internal nodes use 256 + merge step) and the
//! priority queue orders nodes by `(weight, id)`. Because that key is unique,
//! the tree only depends on the frequency table, so the decompressor can
//! rebuild the exact same tree from the header alone.

use rayon::prelude::*;
use std::cmp::{Ordering, Reverse};
use std::collections::BinaryHeap;

pub type Freq = [u64; 256];

pub struct Node {
    pub weight: u64,
    pub id: u32,
    pub symbol: Option<u8>,
    pub left: Option<Box<Node>>,
    pub right: Option<Box<Node>>,
}

impl Node {
    fn is_leaf(&self) -> bool {
        self.left.is_none() && self.right.is_none()
    }
}

impl PartialEq for Node {
    fn eq(&self, other: &Self) -> bool {
        self.weight == other.weight && self.id == other.id
    }
}
impl Eq for Node {}
impl PartialOrd for Node {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Node {
    fn cmp(&self, other: &Self) -> Ordering {
        // Weight first, then id. Ids are unique, so there are never ties.
        self.weight
            .cmp(&other.weight)
            .then(self.id.cmp(&other.id))
    }
}

/// One step of the tree build: the two lightest nodes popped from the queue
/// and the parent pushed back.
#[derive(Clone, Debug)]
pub struct Merge {
    pub left: (u32, u64),
    pub right: (u32, u64),
    pub parent: (u32, u64),
}

/// Count byte frequencies. Large inputs are split into chunks that are
/// counted in parallel and summed.
pub fn build_frequency(data: &[u8]) -> Freq {
    data.par_chunks(1 << 16)
        .map(|chunk| {
            let mut local = [0u64; 256];
            for &b in chunk {
                local[b as usize] += 1;
            }
            local
        })
        .reduce(
            || [0u64; 256],
            |mut a, b| {
                for i in 0..256 {
                    a[i] += b[i];
                }
                a
            },
        )
}

/// Build the Huffman tree. Returns `None` for empty input. The list of merges
/// is returned too so the tracer can replay the priority queue.
pub fn build_tree(freq: &Freq) -> (Option<Box<Node>>, Vec<Merge>) {
    let mut heap: BinaryHeap<Reverse<Box<Node>>> = BinaryHeap::new();
    for (sym, &count) in freq.iter().enumerate() {
        if count > 0 {
            heap.push(Reverse(Box::new(Node {
                weight: count,
                id: sym as u32,
                symbol: Some(sym as u8),
                left: None,
                right: None,
            })));
        }
    }

    let mut merges = Vec::new();
    let mut next_id = 256u32;
    while heap.len() > 1 {
        let left = heap.pop().unwrap().0;
        let right = heap.pop().unwrap().0;
        let parent = Node {
            weight: left.weight + right.weight,
            id: next_id,
            symbol: None,
            left: None,
            right: None,
        };
        merges.push(Merge {
            left: (left.id, left.weight),
            right: (right.id, right.weight),
            parent: (parent.id, parent.weight),
        });
        next_id += 1;
        heap.push(Reverse(Box::new(Node {
            left: Some(left),
            right: Some(right),
            ..parent
        })));
    }
    (heap.pop().map(|r| r.0), merges)
}

/// A code word: the low `len` bits of `bits`, most significant bit first.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Code {
    pub bits: u128,
    pub len: u8,
}

impl Code {
    pub fn bit_string(self) -> String {
        (0..self.len)
            .rev()
            .map(|i| if (self.bits >> i) & 1 == 1 { '1' } else { '0' })
            .collect()
    }
}

/// Walk the tree and assign codes: left edge = 0, right edge = 1.
/// A tree with a single leaf gets the one-bit code "0" so every symbol
/// still costs at least one bit and decoding stays unambiguous.
pub fn build_codes(root: &Node) -> [Option<Code>; 256] {
    let mut codes = [None; 256];
    if root.is_leaf() {
        codes[root.symbol.unwrap() as usize] = Some(Code { bits: 0, len: 1 });
        return codes;
    }
    let mut stack = vec![(root, Code::default())];
    while let Some((node, code)) = stack.pop() {
        if node.is_leaf() {
            codes[node.symbol.unwrap() as usize] = Some(code);
            continue;
        }
        if let Some(left) = &node.left {
            stack.push((left, Code { bits: code.bits << 1, len: code.len + 1 }));
        }
        if let Some(right) = &node.right {
            stack.push((right, Code { bits: (code.bits << 1) | 1, len: code.len + 1 }));
        }
    }
    codes
}

/// Encode `data` into packed bytes (MSB first, last byte zero-padded).
/// Returns the bytes and the exact number of meaningful bits.
pub fn encode(data: &[u8], codes: &[Option<Code>; 256]) -> (Vec<u8>, u64) {
    let mut out = Vec::with_capacity(data.len() / 2);
    let mut acc: u128 = 0;
    let mut acc_len: u32 = 0;
    let mut total_bits: u64 = 0;
    for &b in data {
        let code = codes[b as usize].expect("symbol missing from code table");
        let mut len = code.len as u32;
        total_bits += len as u64;
        // Feed the code in pieces of at most 64 bits so `acc` never overflows.
        while len > 0 {
            let take = len.min(64);
            let piece = (code.bits >> (len - take)) & ((1u128 << take) - 1);
            acc = (acc << take) | piece;
            acc_len += take;
            len -= take;
            while acc_len >= 8 {
                acc_len -= 8;
                out.push((acc >> acc_len) as u8);
            }
            acc &= (1u128 << acc_len) - 1;
        }
    }
    if acc_len > 0 {
        out.push((acc << (8 - acc_len)) as u8);
    }
    (out, total_bits)
}

/// Decode exactly `count` symbols from `bytes` by walking the tree.
pub fn decode(root: &Node, bytes: &[u8], count: u64) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(count as usize);
    if count == 0 {
        return Ok(out);
    }
    if root.is_leaf() {
        // Single-symbol input: every bit is one copy of the symbol.
        if (bytes.len() as u64) < count.div_ceil(8) {
            return Err("payload ended early".into());
        }
        out.resize(count as usize, root.symbol.unwrap());
        return Ok(out);
    }
    let mut node = root;
    'outer: for &byte in bytes {
        for i in (0..8).rev() {
            let bit = (byte >> i) & 1;
            node = if bit == 0 {
                node.left.as_deref().unwrap()
            } else {
                node.right.as_deref().unwrap()
            };
            if node.is_leaf() {
                out.push(node.symbol.unwrap());
                node = root;
                if out.len() as u64 == count {
                    break 'outer;
                }
            }
        }
    }
    if (out.len() as u64) < count {
        return Err(format!(
            "payload ended early: decoded {} of {} symbols",
            out.len(),
            count
        ));
    }
    Ok(out)
}

/// Shannon entropy of the byte distribution, in bits per symbol.
pub fn entropy(freq: &Freq) -> f64 {
    let total: u64 = freq.iter().sum();
    if total == 0 {
        return 0.0;
    }
    freq.iter()
        .filter(|&&c| c > 0)
        .map(|&c| {
            let p = c as f64 / total as f64;
            -p * p.log2()
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(data: &[u8]) {
        let freq = build_frequency(data);
        let (tree, _) = build_tree(&freq);
        let Some(tree) = tree else {
            assert!(data.is_empty());
            return;
        };
        let codes = build_codes(&tree);
        let (bytes, bits) = encode(data, &codes);
        assert_eq!(bytes.len() as u64, bits.div_ceil(8));
        let decoded = decode(&tree, &bytes, data.len() as u64).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn roundtrips() {
        roundtrip(b"");
        roundtrip(b"a");
        roundtrip(b"aaaaaaa");
        roundtrip(b"ABCBBAAAAAADDZBB");
        roundtrip(b"hello world\nwith spaces and\nnewlines\n");
        roundtrip("unicode: caf\u{e9} \u{1f980} \u{4e2d}\u{6587}".as_bytes());
        let all: Vec<u8> = (0..=255u8).cycle().take(10_000).collect();
        roundtrip(&all);
    }

    #[test]
    fn skewed_fibonacci_tree() {
        // Fibonacci weights produce a maximally skewed tree.
        let mut data = Vec::new();
        let (mut a, mut b) = (1u64, 1u64);
        for sym in 0..30u8 {
            data.extend(std::iter::repeat(sym).take(a as usize));
            (a, b) = (b, a + b);
        }
        roundtrip(&data);
    }

    #[test]
    fn tree_is_deterministic() {
        let data = b"abracadabra alakazam";
        let freq = build_frequency(data);
        let a = build_codes(&build_tree(&freq).0.unwrap());
        let b = build_codes(&build_tree(&freq).0.unwrap());
        assert_eq!(a, b);
    }

    #[test]
    fn entropy_bounds_average_length() {
        let data = b"the quick brown fox jumps over the lazy dog";
        let freq = build_frequency(data);
        let codes = build_codes(&build_tree(&freq).0.unwrap());
        let total = data.len() as f64;
        let avg: f64 = freq
            .iter()
            .enumerate()
            .filter(|(_, c)| **c > 0)
            .map(|(s, c)| *c as f64 * codes[s].unwrap().len as f64)
            .sum::<f64>()
            / total;
        let h = entropy(&freq);
        assert!(h <= avg && avg < h + 1.0, "H={h} L={avg}");
    }
}
