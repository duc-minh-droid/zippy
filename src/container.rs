//! The `.zpy` file format.
//!
//! ```text
//! offset  size      field
//! 0       4         magic "ZPY" + format version 0x01
//! 4       1         n - 1, where n = number of distinct symbols (1..=256)
//! 5       n entries symbol byte, then its count as an unsigned LEB128 varint
//! ...     rest      Huffman payload, MSB first, last byte zero-padded
//! ```
//!
//! Empty input is stored as the magic plus a single `0xFF` byte and nothing
//! else (a real 256-symbol table would be followed by entries). The original
//! length is the sum of the counts, and the tree is rebuilt deterministically
//! from the counts, so nothing else needs to be stored.

use crate::huffman::Freq;

pub const MAGIC: [u8; 4] = *b"ZPY\x01";

pub struct Header {
    pub freq: Freq,
    pub original_len: u64,
    /// Size of magic + symbol table in bytes.
    pub header_len: usize,
}

fn write_varint(out: &mut Vec<u8>, mut v: u64) {
    loop {
        let byte = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

fn read_varint(data: &[u8], pos: &mut usize) -> Result<u64, String> {
    let mut v: u64 = 0;
    for shift in (0..64).step_by(7) {
        let byte = *data.get(*pos).ok_or("truncated symbol table")?;
        *pos += 1;
        v |= ((byte & 0x7f) as u64) << shift;
        if byte & 0x80 == 0 {
            return Ok(v);
        }
    }
    Err("varint too long".into())
}

pub fn write_header(freq: &Freq) -> Vec<u8> {
    let mut out = MAGIC.to_vec();
    let symbols: Vec<usize> = (0..256).filter(|&s| freq[s] > 0).collect();
    if symbols.is_empty() {
        out.push(0xff);
        return out;
    }
    out.push((symbols.len() - 1) as u8);
    for s in symbols {
        out.push(s as u8);
        write_varint(&mut out, freq[s]);
    }
    out
}

pub fn read_header(data: &[u8]) -> Result<Header, String> {
    if data.len() < 5 || data[..3] != MAGIC[..3] {
        return Err("not a zippy file (bad magic)".into());
    }
    if data[3] != MAGIC[3] {
        return Err(format!("unsupported format version {}", data[3]));
    }
    let mut freq = [0u64; 256];
    let mut pos = 5;
    // An 0xFF count byte followed by nothing is the empty file.
    if data[4] == 0xff && data.len() == 5 {
        return Ok(Header { freq, original_len: 0, header_len: 5 });
    }
    let n = data[4] as usize + 1;
    let mut total: u64 = 0;
    for _ in 0..n {
        let sym = *data.get(pos).ok_or("truncated symbol table")?;
        pos += 1;
        let count = read_varint(data, &mut pos)?;
        if count == 0 || freq[sym as usize] != 0 {
            return Err("corrupt symbol table".into());
        }
        freq[sym as usize] = count;
        total = total.checked_add(count).ok_or("symbol counts overflow")?;
    }
    Ok(Header { freq, original_len: total, header_len: pos })
}
