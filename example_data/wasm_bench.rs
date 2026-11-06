//! One `calculate_batch` call, as the aether `hr_payroll` plugin does it, for measuring in WebAssembly.
//!
//!   cargo build --release --target wasm32-wasip2 -p swift_payroll_engine --example wasm_bench
//!   wasmtime run -W fuel=50000000 --dir . target/wasm32-wasip2/release/examples/wasm_bench.wasm \
//!       <structure.json> <people.ndjson> <mode> <people>
//!
//! Modes: `plugin` (run + lines to a JSON value, what the plugin does), `text` (lines straight to text), `lines` (built, unused),
//! `compact` (amounts only), `batch` (what hr_payroll does now), `summary` (totals only), `compile` (parse and compile), `read` (start up and read the files).
//! Prints the time of each phase. The fuel the whole call used is found by lowering `-W fuel=` until it runs out
//! (see example_data/README.md); the aether kernel allows 50,000,000 per call.

use std::time::Instant;

use sp_engine::{Compiled, Input, Structure};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (structure_path, people_path, mode, count) = (&args[0], &args[1], args[2].as_str(), args[3].parse::<usize>().unwrap());
    let text = std::fs::read_to_string(structure_path).unwrap();
    let people_text = std::fs::read_to_string(people_path).unwrap();

    let t = Instant::now();
    let structure: Structure = serde_json::from_str(&text).unwrap();
    let t_parse = t.elapsed();
    let t = Instant::now();
    let compiled = Compiled::new(&structure).unwrap();
    let t_compile = t.elapsed();
    if mode == "read" {
        return;
    }
    if mode == "compile" {
        println!("parse {t_parse:?} compile {t_compile:?}");
        return;
    }
    let t = Instant::now();
    let people: Vec<Input> = people_text.lines().take(count).map(|l| serde_json::from_str(l).unwrap()).collect();
    let t_people = t.elapsed();

    let t = Instant::now();
    let mut net_sum = rust_decimal::Decimal::ZERO;
    let mut bytes = 0usize;
    let mut sums: Vec<sp_engine::Decimal> = Vec::new();
    for person in &people {
        match mode {
            "summary" => {
                if let Ok(s) = compiled.summarize(person) {
                    net_sum += s.net;
                }
            }
            // Lines built, nothing done with them.
            "lines" => {
                if let Ok(slip) = compiled.run(person, false) {
                    net_sum += slip.net;
                }
            }
            // Lines turned straight into text (no intermediate JSON value).
            "text" => {
                if let Ok(slip) = compiled.run(person, false) {
                    net_sum += slip.net;
                    bytes += serde_json::to_string(&slip.lines).unwrap().len();
                }
            }
            // What hr_payroll does now: amounts and running sums, lines kept as a compact JSON array.
            "batch" => {
                if let Ok(c) = compiled.calculate_amounts(person) {
                    net_sum += c.summary.net;
                    if sums.is_empty() {
                        sums = vec![Default::default(); c.amounts.len()];
                    }
                    for (sum, amount) in sums.iter_mut().zip(&c.amounts) {
                        *sum += *amount;
                    }
                    let value = serde_json::Value::Array(c.amounts.iter().map(|a| serde_json::Value::String(a.to_string())).collect());
                    bytes += value.to_string().len();
                }
            }
            // Amounts only, in structure order, as a JSON value: what a payslip needs if it keeps its structure id.
            "compact" => {
                if let Ok(c) = compiled.calculate_amounts(person) {
                    net_sum += c.summary.net;
                    let value = serde_json::Value::Array(c.amounts.iter().map(|a| serde_json::Value::String(a.to_string())).collect());
                    bytes += value.to_string().len();
                }
            }
            _ => {
                if let Ok(slip) = compiled.run(person, false) {
                    net_sum += slip.net;
                    // The plugin stores every line on the payslip row.
                    bytes += serde_json::to_value(&slip.lines).unwrap().to_string().len();
                }
            }
        }
    }
    let t_run = t.elapsed();
    println!("parse {t_parse:?} compile {t_compile:?} people {t_people:?} run({}) {t_run:?} = {:?}/person, lines JSON {} bytes/person, net {net_sum}", people.len(), t_run / people.len().max(1) as u32, bytes / people.len().max(1));
}
