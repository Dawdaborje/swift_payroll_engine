//! `swift_payroll_engine --input run.json [--explain]`, or `swift_payroll_engine check-rules <dir>`
//!
//! The input is `{ "structure": {...}, "employees": [ { "employee_id": "..", "variables": {...} } ] }`
//! (see docs/data-format.md). The output is one JSON object with a payslip or an error per person.
//!
//! For a large payroll use the streaming form, which never holds more than a window of people in memory:
//!
//! `swift_payroll_engine --structure s.json --employees people.ndjson [--summary] [--explain] [--window N]`
//!
//! `--employees -` reads standard input. Each line is one person; each output line is that person's
//! payslip (or only the totals with `--summary`), or `{"employee_id":..,"error":..}` for a person who
//! could not be calculated, or `{"line":..,"error":..}` for a line that is not a person. One bad person
//! never stops the others. Exit status 2 means somebody failed.

use std::collections::HashSet;
use std::io::{BufRead, Write};
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
    let mut structure_path: Option<String> = None;
    let mut employees_path: Option<String> = None;
    let mut explain = false;
    let mut summary = false;
    let mut window = 4096usize;
    let mut args = raw;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--input" => path = Some(args.next().ok_or("--input needs a file")?),
            "--structure" => structure_path = Some(args.next().ok_or("--structure needs a file")?),
            "--employees" => employees_path = Some(args.next().ok_or("--employees needs a file, or - for standard input")?),
            "--window" => window = args.next().and_then(|w| w.parse().ok()).filter(|w| *w > 0).ok_or("--window needs a number above zero")?,
            "--summary" => summary = true,
            "--explain" => explain = true,
            other => return Err(format!("unknown argument `{other}`; use --input <file> [--explain], or --structure <file> --employees <file|-> [--summary] [--explain] [--window N]")),
        }
    }
    if structure_path.is_some() || employees_path.is_some() {
        if path.is_some() {
            return Err("use either --input, or --structure with --employees".to_string());
        }
        let structure_path = structure_path.ok_or("--employees needs --structure <file>")?;
        let employees_path = employees_path.ok_or("--structure needs --employees <file|->")?;
        return stream(&structure_path, &employees_path, explain, summary, window);
    }
    let path = path.ok_or("give the run with --input <file>")?;
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{path}: {e}"))?;
    let run: Run = serde_json::from_str(&text).map_err(|e| format!("{path}: {e}"))?;
    let compiled = Compiled::new(&run.structure).map_err(|e| e.to_string())?;
    let results = compiled.run_all(&run.employees, explain);
    let mut failed = false;
    let mut seen: HashSet<&str> = HashSet::new();
    let output: Vec<serde_json::Value> = results
        .into_iter()
        .zip(&run.employees)
        .map(|(result, input)| match result {
            // The same person twice is paid twice: refuse the second, loudly.
            Ok(_) if !seen.insert(input.employee_id.as_str()) => {
                failed = true;
                serde_json::json!({ "employee_id": input.employee_id, "error": "this employee_id appears more than once in the run" })
            }
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

/// Calculate a payroll one window at a time: people in as lines, payslips out as lines.
fn stream(structure_path: &str, employees_path: &str, explain: bool, summary: bool, window: usize) -> Result<bool, String> {
    let text = std::fs::read_to_string(structure_path).map_err(|e| format!("{structure_path}: {e}"))?;
    let structure: Structure = serde_json::from_str(&text).map_err(|e| format!("{structure_path}: {e}"))?;
    let compiled = Compiled::new(&structure).map_err(|e| e.to_string())?;
    let reader: Box<dyn BufRead> = if employees_path == "-" {
        Box::new(std::io::BufReader::new(std::io::stdin()))
    } else {
        Box::new(std::io::BufReader::new(std::fs::File::open(employees_path).map_err(|e| format!("{employees_path}: {e}"))?))
    };
    let mut out = std::io::BufWriter::new(std::io::stdout().lock());
    let mut failed = false;
    let mut seen: HashSet<String> = HashSet::new();
    let mut lines = reader.lines().enumerate();
    let write = |out: &mut std::io::BufWriter<std::io::StdoutLock<'_>>, value: serde_json::Value| -> Result<(), String> {
        serde_json::to_writer(&mut *out, &value).map_err(|e| e.to_string())?;
        out.write_all(b"\n").map_err(|e| e.to_string())
    };
    loop {
        // Read a window of people. A line that is not a person is reported and skipped.
        let mut batch: Vec<Input> = Vec::with_capacity(window);
        while batch.len() < window {
            let Some((number, line)) = lines.next() else { break };
            let line = line.map_err(|e| format!("{employees_path}: line {}: {e}", number + 1))?;
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<Input>(&line) {
                Ok(input) if input.employee_id.trim().is_empty() => {
                    failed = true;
                    write(&mut out, serde_json::json!({ "line": number + 1, "error": "employee_id is empty" }))?;
                }
                Ok(input) if !seen.insert(input.employee_id.clone()) => {
                    failed = true;
                    write(&mut out, serde_json::json!({ "employee_id": input.employee_id, "line": number + 1, "error": "this employee_id appears more than once in the run" }))?;
                }
                Ok(input) => batch.push(input),
                Err(error) => {
                    failed = true;
                    write(&mut out, serde_json::json!({ "line": number + 1, "error": error.to_string() }))?;
                }
            }
        }
        if batch.is_empty() {
            break;
        }
        let mut problem: Option<String> = None;
        compiled.for_each(batch.iter(), explain, window, |input, result| {
            let value = match result {
                Ok(slip) if summary => serde_json::json!({
                    "employee_id": slip.employee_id, "gross": slip.gross, "deductions": slip.deductions,
                    "net": slip.net, "employer_cost": slip.employer_cost, "shortfall": slip.shortfall
                }),
                Ok(slip) => serde_json::to_value(slip).unwrap_or_default(),
                Err(error) => {
                    failed = true;
                    serde_json::json!({ "employee_id": input.employee_id, "error": error.to_string() })
                }
            };
            if let Err(e) = write(&mut out, value) {
                problem.get_or_insert(e);
            }
        });
        if let Some(e) = problem {
            return Err(format!("writing the output: {e}"));
        }
    }
    out.flush().map_err(|e| e.to_string())?;
    Ok(failed)
}
