//! `swift_payroll_engine --input run.json [--explain]`, or `swift_payroll_engine check-rules <dir>`
//!
//! The input is `{ "structure": {...}, "employees": [ { "employee_id": "..", "variables": {...} } ] }`
//! (see docs/data-format.md). The output is one JSON object with a payslip or an error per person.

use std::process::ExitCode;

use serde::Deserialize;
use sp_engine::{Compiled, Input, RulePack, Structure};

#[derive(Deserialize)]
struct Run {
    structure: Structure,
    employees: Vec<Input>,
}

fn main() -> ExitCode {
    match real_main() {
        Ok(failed) => if failed { ExitCode::from(2) } else { ExitCode::SUCCESS },
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

/// `check-rules <dir>`: load every rule pack under a directory and run its tests.
fn check_rules(dir: &std::path::Path) -> Result<bool, String> {
    let mut failed = false;
    let mut stack = vec![dir.to_path_buf()];
    let mut packs = 0;
    while let Some(folder) = stack.pop() {
        for entry in std::fs::read_dir(&folder).map_err(|e| format!("{}: {e}", folder.display()))? {
            let path = entry.map_err(|e| e.to_string())?.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "cel") {
                let meta = path.with_extension("json");
                let expression = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
                if !meta.exists() {
                    println!("pending  {} (no description yet)", path.display());
                    continue;
                }
                let description = std::fs::read_to_string(&meta).map_err(|e| format!("{}: {e}", meta.display()))?;
                match RulePack::parse(&description, &expression) {
                    Err(error) => {
                        failed = true;
                        println!("FAIL     {}: {error}", path.display());
                    }
                    Ok(pack) => {
                        packs += 1;
                        let results = pack.run_tests();
                        let bad: Vec<_> = results.iter().filter(|(_, r)| r.is_err()).collect();
                        if results.is_empty() {
                            failed = true;
                            println!("FAIL     {}: a rule needs at least one test", pack.info.id);
                        } else if bad.is_empty() {
                            println!("ok       {} ({} tests{})", pack.info.id, results.len(), if pack.info.verified { "" } else { ", NOT VERIFIED against the law" });
                        } else {
                            failed = true;
                            for (name, outcome) in bad {
                                println!("FAIL     {} / {name}: {}", pack.info.id, outcome.as_ref().err().map_or("", String::as_str));
                            }
                        }
                    }
                }
            }
        }
    }
    println!("{packs} rule pack(s) loaded");
    Ok(failed)
}

fn real_main() -> Result<bool, String> {
    let mut raw = std::env::args().skip(1).peekable();
    if raw.peek().map(String::as_str) == Some("check-rules") {
        raw.next();
        let dir = raw.next().ok_or("give the folder: check-rules <dir>")?;
        return check_rules(std::path::Path::new(&dir));
    }
    let mut path: Option<String> = None;
    let mut explain = false;
    let mut args = raw;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--input" => path = args.next(),
            "--explain" => explain = true,
            other => return Err(format!("unknown argument `{other}`; use --input <file> [--explain]")),
        }
    }
    let path = path.ok_or("give the run with --input <file>")?;
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{path}: {e}"))?;
    let run: Run = serde_json::from_str(&text).map_err(|e| format!("{path}: {e}"))?;
    let compiled = Compiled::new(&run.structure).map_err(|e| e.to_string())?;
    let results = compiled.run_all(&run.employees, explain);
    let mut failed = false;
    let output: Vec<serde_json::Value> = results
        .into_iter()
        .zip(&run.employees)
        .map(|(result, input)| match result {
            Ok(slip) => serde_json::to_value(slip).unwrap_or_default(),
            Err(error) => {
                failed = true;
                serde_json::json!({ "employee_id": input.employee_id, "error": error.to_string() })
            }
        })
        .collect();
    println!("{}", serde_json::to_string_pretty(&output).map_err(|e| e.to_string())?);
    Ok(failed)
}
