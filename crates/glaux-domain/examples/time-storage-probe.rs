//! Bounded TSV bridge for the disposable PostgreSQL exact-time proof.
//!
//! This example neither connects to a database nor constructs SQL. The Python
//! harness moves actual domain output through PostgreSQL and back through the
//! checked reconstruction boundary. It is not an application storage adapter.

use glaux_domain::temporal::ExactInstant;
use std::io::{self, Read};

const MAX_INPUT_BYTES: usize = 1_048_576;
const MAX_ROWS: usize = 128;
const MAX_LINE_BYTES: usize = 16_384;

fn run() -> Result<(), String> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let mode = match arguments.as_slice() {
        [mode] if mode == "parse" || mode == "reconstruct" => mode.as_str(),
        _ => return Err("specify exactly parse or reconstruct".to_owned()),
    };
    let mut input = String::new();
    io::stdin()
        .lock()
        .take((MAX_INPUT_BYTES + 1) as u64)
        .read_to_string(&mut input)
        .map_err(|error| format!("input: {error}"))?;
    if input.is_empty() || input.len() > MAX_INPUT_BYTES {
        return Err("input is empty or exceeds the byte budget".to_owned());
    }
    let lines: Vec<&str> = input.split_terminator('\n').collect();
    if lines.len() > MAX_ROWS {
        return Err("input exceeds the row budget".to_owned());
    }
    let mut output = Vec::with_capacity(lines.len());
    for line in lines {
        if line.is_empty() || line.len() > MAX_LINE_BYTES || line.contains('\r') {
            return Err("empty, oversized or CR-containing input row".to_owned());
        }
        let value = if mode == "parse" {
            if line.contains('\t') {
                return Err("parse expects exactly one timestamp per row".to_owned());
            }
            ExactInstant::parse_rfc3339(line).map_err(|error| format!("parse: {error}"))?
        } else {
            let fields: Vec<&str> = line.split('\t').collect();
            let [second, leap, fraction, source] = fields.as_slice() else {
                return Err("reconstruct expects second, leap, fraction and source".to_owned());
            };
            let second = second
                .parse::<i64>()
                .map_err(|error| format!("second: {error}"))?;
            let leap = match *leap {
                "true" => true,
                "false" => false,
                _ => return Err("leap must be true or false".to_owned()),
            };
            ExactInstant::from_storage_parts(second, leap, fraction, source)
                .map_err(|error| format!("reconstruct: {error}"))?
        };
        output.push(format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}",
            value.civil_second(),
            value.is_leap_second(),
            value.fraction_decimal(),
            value.source_lexeme(),
            value.fraction_digits(),
            value.offset_seconds(),
            value.offset_known(),
        ));
    }
    // Publish no successful prefix if a later input row is rejected.
    println!("{}", output.join("\n"));
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("Time storage probe rejected input: {error}");
        std::process::exit(1);
    }
}
