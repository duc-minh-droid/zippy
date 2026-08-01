use std::collections::HashMap;
use std::cmp::{Ordering, Reverse};
use std::collections::BinaryHeap;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::time::Instant;
use std::env;

struct Node {
    value: i32,
    letter: Option<char>,
    left: Option<Box<Node>>,
    right: Option<Box<Node>>,
}

impl PartialEq for Node {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
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
        match self.value.cmp(&other.value) {
            Ordering::Equal => {
                self.letter.cmp(&other.letter)
            }
            other => other
        }
    }
}

fn build_frequency(text: &String) -> HashMap<char, i32> {
    let mut freq = HashMap::new();
    for c in text.chars() {
        *freq.entry(c).or_insert(0) += 1;
    }
    freq
}

fn build_tree(freq: &HashMap<char, i32>) -> Box<Node> {
    let mut heap: BinaryHeap<Reverse<Box<Node>>> = BinaryHeap::new();
    for (letter, count) in freq {
        heap.push(Reverse(Box::new(Node {
            value: *count,
            letter: Some(*letter),
            left: None,
            right: None,
        })));
    }

    while heap.len() > 1 {
        let left = heap.pop().unwrap().0;
        let right = heap.pop().unwrap().0;
        let parent = Node {
            value: left.value + right.value,
            letter: None,
            left: Some(left),
            right: Some(right),
        };
        heap.push(Reverse(Box::new(parent)));
    }
    heap.pop().unwrap().0
}

fn build_codes(root: Box<Node>) -> HashMap<char, String> {
    let mut codes = HashMap::new();
    let mut stack = Vec::new();
    stack.push((root, String::new()));
    while let Some((node, path)) = stack.pop() {
        if node.left.is_none() && node.right.is_none() {
            codes.insert(node.letter.unwrap(), path);
            continue;
        }
        if let Some(left) = node.left {
            stack.push((left, path.clone() + "0"));
        }
        if let Some(right) = node.right {
            stack.push((right, path + "1"));
        }
    }
    codes
}

fn encode(text: &String, codes: &HashMap<char,String>) -> String {
    let mut bits = String::new();
    for c in text.chars() {
        bits.push_str(codes.get(&c).unwrap());
    }
    bits
}

fn bits_to_bytes(bits: String) -> (Vec<u8>, usize) {
    let total_bits = bits.len();
    let mut bytes = Vec::new();
    let mut current = 0u8;
    let mut count = 0;
    for bit in bits.chars() {
        current <<= 1;
        if bit == '1' {
            current |= 1;
        }
        count += 1;
        if count == 8 {
            bytes.push(current);
            current = 0;
            count = 0;
        }
    }
    let valid_bits;
    if count > 0 {
        current <<= 8-count;
        bytes.push(current);
        valid_bits = count;
    }
    else {
        valid_bits = 8;
    }

    (bytes, total_bits)
}

fn compress_file(
    freq: &HashMap<char,i32>,
    bytes: Vec<u8>,
    total_bits: usize
) {
    let mut header = String::new();
    for (c,count) in freq {
        header.push_str(&format!("{} {}\n", c, count));
    }
    header.push_str("---\n");
    header.push_str(&format!("{}\n", total_bits));
    let mut file = File::create("compressed.huff").unwrap();
    let size = header.len() as u32;
    file.write_all(&size.to_le_bytes()).unwrap();
    file.write_all(header.as_bytes()).unwrap();
    file.write_all(&bytes).unwrap();
}

fn decompress_file() -> (HashMap<char,i32>, Vec<u8>, usize) {
    let mut file = File::open("compressed.huff").unwrap();
    let mut size_buffer = [0u8;4];
    file.read_exact(&mut size_buffer).unwrap();
    let size = u32::from_le_bytes(size_buffer) as usize;
    let mut header_bytes = vec![0u8; size];
    file.read_exact(&mut header_bytes).unwrap();
    let header = String::from_utf8(header_bytes).unwrap();
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).unwrap();
    let mut freq = HashMap::new();
    let mut total_bits = 0;
    for line in header.lines() {
        if line == "---" {
            continue;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() == 1 {
            total_bits = parts[0].parse().unwrap();
        }
        else {
            let c = parts[0].chars().next().unwrap();
            let count = parts[1].parse().unwrap();
            freq.insert(c,count);

        }
    }

    (freq, bytes, total_bits)
}

fn decode(
    root: Box<Node>,
    bytes: Vec<u8>,
    total_bits:usize
) -> String {
    let mut result = String::new();
    let mut node = &root;
    let mut read_bits = 0;
    for byte in bytes {
        for i in (0..8).rev() {
            if read_bits >= total_bits {
                break;
            }
            read_bits += 1;
            let bit = (byte >> i) & 1;
            if bit == 0 {
                node = node.left.as_ref().unwrap();
            }
            else {
                node = node.right.as_ref().unwrap();
            }
            if node.left.is_none() && node.right.is_none() {
                result.push(node.letter.unwrap());
                node = &root;
            }
        }
    }

    result
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        println!("Usage: {} <input_file>", args[0]);
        return;
    }
    let input_file = &args[1];
    let text = fs::read_to_string(input_file).unwrap();

    let start = Instant::now();
    let freq = build_frequency(&text);
    println!("Build frequency: {:?}", start.elapsed());

    let start = Instant::now();
    let tree = build_tree(&freq);
    println!("Build tree: {:?}", start.elapsed());

    let start = Instant::now();
    let codes = build_codes(tree);
    println!("Build codes: {:?}", start.elapsed());

    let start = Instant::now();
    let bits = encode(&text,&codes);
    println!("Encode: {:?}", start.elapsed());

    let start = Instant::now();
    let (bytes,total_bits)=bits_to_bytes(bits);
    println!("Pack bits: {:?}", start.elapsed());

    let start = Instant::now();
    compress_file(
        &freq,
        bytes,
        total_bits
    );
    println!("Write compressed file: {:?}", start.elapsed());

    let start = Instant::now();
    let (freq2,bytes2,total_bits2)=decompress_file();
    println!("Read compressed file: {:?}", start.elapsed());

    let start = Instant::now();
    let tree2 = build_tree(&freq2);
    println!("Rebuild tree: {:?}", start.elapsed());

    let start = Instant::now();
    let decoded = decode(
        tree2,
        bytes2,
        total_bits2
    );
    println!("Decode: {:?}", start.elapsed());

    let start = Instant::now();
    fs::write(
        "output.txt",
        decoded
    ).unwrap();
    println!("Write output: {:?}", start.elapsed());
    println!("Total: {:?}", start.elapsed());
}