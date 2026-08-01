# Huffman Compressor

A simple Huffman coding compressor written in Rust.

## Usage

Compress a file:

```bash
./main input.txt
```
The program will create:
```bash
compressed.huff - compressed binary file
output.txt - decompressed file (for verification)
```

## Features
- Builds Huffman tree from character frequencies
- Generates variable-length binary codes
- Compresses text into bytes
- Decompresses back to the original text