use std::collections::HashMap;
use std::cmp::{Ordering, Reverse};
use std::collections::BinaryHeap;
use std::fs;

struct Node {
    value: i32,
    letter: Option<char>,
    left: Option<Box<Node>>,
    right: Option<Box<Node>>
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
        self.value.cmp(&other.value)
    }
}

fn main() {
    let text = fs::read_to_string("text.txt").expect("Failed to read file");

    let mut freq : HashMap<char, i32> = HashMap::new();
    for letter in text.chars() {
        *freq.entry(letter).or_insert(0) += 1;
    }

    let mut heap : BinaryHeap<Reverse<Box<Node>>> = BinaryHeap::new();
    for (letter, count) in freq {
        let node = Node {
            value: count,
            letter: Some(letter),
            left: None,
            right: None,
        };
        heap.push(Reverse(Box::new(node)));
    }

    while heap.len() > 1 {
        let c1 = heap.pop().unwrap().0;
        let c2 = heap.pop().unwrap().0;
        // println!("left {} right {}", c1.value, c2.value);

        let p = Node {
            value: c1.value + c2.value,
            letter: None,
            left: Some(c1),
            right: Some(c2),
        };
        // println!("parent {}", p.value);
        heap.push(Reverse(Box::new(p)));
    }

    let root = heap.pop().unwrap().0;

    let mut birep : HashMap<char, String> = HashMap::new();
    let mut stack : Vec<(Box<Node>, String)> = Vec::new();
    stack.push((root, "".to_string()));

    while (!stack.is_empty()) {
        let (node, path) = stack.pop().unwrap();

        if (node.left.is_none() && node.right.is_none()) {
            birep.insert(node.letter.unwrap(), path.clone());
        }
        if (node.left.is_some()) {
            stack.push((node.left.unwrap(), path.clone() + "0"));
        }
        if (node.right.is_some()) {
            stack.push((node.right.unwrap(), path + "1"));
        }
    }

    // for (letter, path) in &birep {
    //     println!("letter {}", letter);
    //     println!("path: {}", path);
    // }

    let mut binary_text : String = "".to_string();
    for letter in text.chars() {
        binary_text.push_str(birep.get(&letter).unwrap());
    }

    fs::write("output.txt", binary_text).expect("Failed to write file");

    // save bytes in .huff

    // decompress .huff to output.txt
}
