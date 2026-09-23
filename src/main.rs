mod container;
mod huffman;
mod trace;

use std::env;
use std::fs;
use std::process::ExitCode;
use std::time::Instant;

use huffman::{build_codes, build_frequency, build_tree, decode, encode};
use trace::{Stats, TraceInput};

const USAGE: &str = "\
zippy - Huffman compressor

usage:
  zippy compress   <input> [-o <output>] [--trace <file.json>|-] [--timings]
  zippy decompress <input> [-o <output>] [--timings]
  zippy stats      <input> [--trace <file.json>|-]

compress     writes <input>.zpy by default and verifies the round-trip in memory
decompress   needs only the .zpy file; writes <input> minus .zpy (or <input>.out)
stats        compression statistics without writing anything
--trace      dump frequencies, heap merges, codes and a bit-stream sample as JSON
             ('-' writes to stdout)";

struct Args {
    command: String,
    input: String,
    output: Option<String>,
    trace: Option<String>,
    timings: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut it = env::args().skip(1);
    let command = it.next().ok_or("missing command")?;
    if command == "-h" || command == "--help" || command == "help" {
        return Err(String::new());
    }
    let mut input = None;
    let mut output = None;
    let mut trace = None;
    let mut timings = false;
    while let Some(a) = it.next() {
        match a.as_str() {
            "-o" | "--output" => output = Some(it.next().ok_or("-o needs a path")?),
            "--trace" => trace = Some(it.next().ok_or("--trace needs a path or '-'")?),
            "--timings" => timings = true,
            "-h" | "--help" => return Err(String::new()),
            s if s.starts_with('-') && s != "-" => return Err(format!("unknown option {s}")),
            _ if input.is_none() => input = Some(a),
            _ => return Err(format!("unexpected argument {a}")),
        }
    }
    let input = input.ok_or("missing input file")?;
    Ok(Args { command, input, output, trace, timings })
}

/// Tiny stage timer, printed only with --timings (to stderr so stdout stays clean).
struct Timer {
    on: bool,
    t: Instant,
}
impl Timer {
    fn lap(&mut self, what: &str) {
        if self.on {
            eprintln!("  {:<22} {:?}", what, self.t.elapsed());
        }
        self.t = Instant::now();
    }
}

/// Everything the compressor produces for one input.
struct Compressed {
    file: Vec<u8>,
    stats: Stats,
    trace_json: Option<String>,
}

fn compress_bytes(name: &str, data: &[u8], want_trace: bool, timer: &mut Timer) -> Result<Compressed, String> {
    let freq = build_frequency(data);
    timer.lap("count frequencies");
    let (tree, merges) = build_tree(&freq);
    timer.lap("build tree");
    let codes = match &tree {
        Some(t) => build_codes(t),
        None => [None; 256],
    };
    timer.lap("build codes");
    let (payload, bits) = encode(data, &codes);
    timer.lap("encode + pack");

    let mut file = container::write_header(&freq);
    let header_len = file.len();
    file.extend_from_slice(&payload);

    // Verify: decode from the finished file exactly as `decompress` would.
    let back = decompress_bytes(&file)?;
    if back != data {
        return Err("round-trip verification failed".into());
    }
    timer.lap("verify round-trip");

    let stats = Stats::new(&freq, &codes, header_len, bits);
    let trace_json = want_trace.then(|| {
        trace::to_json(&TraceInput {
            name,
            data,
            freq: &freq,
            merges: &merges,
            root: tree.as_ref().map(|t| t.id),
            codes: &codes,
            payload: &payload,
            stats: &stats,
        })
    });
    Ok(Compressed { file, stats, trace_json })
}

fn decompress_bytes(file: &[u8]) -> Result<Vec<u8>, String> {
    let header = container::read_header(file)?;
    let (tree, _) = build_tree(&header.freq);
    match tree {
        None => Ok(Vec::new()),
        Some(root) => decode(&root, &file[header.header_len..], header.original_len),
    }
}

fn write_trace(dest: &str, json: &str) -> Result<(), String> {
    if dest == "-" {
        print!("{json}");
        Ok(())
    } else {
        fs::write(dest, json).map_err(|e| format!("{dest}: {e}"))
    }
}

fn run(args: Args) -> Result<(), String> {
    let read = |p: &str| fs::read(p).map_err(|e| format!("{p}: {e}"));
    let mut timer = Timer { on: args.timings, t: Instant::now() };
    let quiet = args.trace.as_deref() == Some("-");

    match args.command.as_str() {
        "compress" | "c" => {
            let data = read(&args.input)?;
            timer.lap("read input");
            let out = compress_bytes(&args.input, &data, args.trace.is_some(), &mut timer)?;
            let dest = args.output.unwrap_or_else(|| format!("{}.zpy", args.input));
            fs::write(&dest, &out.file).map_err(|e| format!("{dest}: {e}"))?;
            timer.lap("write output");
            if let (Some(t), Some(json)) = (&args.trace, &out.trace_json) {
                write_trace(t, json)?;
            }
            let msg = format!(
                "{} -> {}: {} B -> {} B ({:.1}%), round-trip OK",
                args.input,
                dest,
                out.stats.original_bytes,
                out.stats.compressed_bytes,
                100.0 * out.stats.ratio
            );
            if quiet { eprintln!("{msg}") } else { println!("{msg}") }
        }
        "decompress" | "d" => {
            let file = read(&args.input)?;
            timer.lap("read input");
            let data = decompress_bytes(&file)?;
            timer.lap("decode");
            let dest = args.output.unwrap_or_else(|| match args.input.strip_suffix(".zpy") {
                Some(s) => s.to_string(),
                None => format!("{}.out", args.input),
            });
            fs::write(&dest, &data).map_err(|e| format!("{dest}: {e}"))?;
            timer.lap("write output");
            println!("{} -> {}: {} B -> {} B", args.input, dest, file.len(), data.len());
        }
        "stats" | "s" => {
            let data = read(&args.input)?;
            let out = compress_bytes(&args.input, &data, args.trace.is_some(), &mut timer)?;
            if let (Some(t), Some(json)) = (&args.trace, &out.trace_json) {
                write_trace(t, json)?;
            }
            if !quiet {
                println!("{}", args.input);
                out.stats.print();
                println!("round-trip      {:>12}", "OK");
            }
        }
        other => return Err(format!("unknown command '{other}'")),
    }
    Ok(())
}

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            if !e.is_empty() {
                eprintln!("error: {e}\n");
            }
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match run(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file_roundtrip(data: &[u8]) -> usize {
        let mut t = Timer { on: false, t: Instant::now() };
        let c = compress_bytes("test", data, true, &mut t).unwrap();
        assert_eq!(decompress_bytes(&c.file).unwrap(), data);
        assert_eq!(c.stats.compressed_bytes as usize, c.file.len());
        c.file.len()
    }

    #[test]
    fn container_roundtrips() {
        assert_eq!(file_roundtrip(b""), 5);
        file_roundtrip(b"x");
        file_roundtrip(b"zzzzzzzzzzzzzzzzz");
        file_roundtrip(b"ABCBBAAAAAADDZBB");
        let all: Vec<u8> = (0..=255u8).collect();
        file_roundtrip(&all);
        file_roundtrip("caf\u{e9} \u{1f980}\n\ttabs \"quotes\"".as_bytes());
    }

    #[test]
    fn rejects_garbage() {
        assert!(decompress_bytes(b"hello").is_err());
        assert!(decompress_bytes(b"ZPY\x01\x01A").is_err());
    }
}
