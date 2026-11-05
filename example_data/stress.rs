//! Scale / soak harness: many employees x many CEL tax rules, with an independent oracle.
//!
//!   cargo run --release -p swift_payroll_engine --example stress -- run --employees 1000000 --rules 10000
//!   cargo run --release -p swift_payroll_engine --example stress -- gen --rules 10000 --employees 1000 --out example_data/stress_structure_10k.json
//!
//! `run` builds the structure in memory (or reads `--structure FILE`), makes every employee from its
//! index (so a million people need no input file), and checks each payslip three ways:
//!   1. invariants: line count, totals are the sums of their lines, net = gross - deductions;
//!      (`--full` builds and checks the whole payslip of everyone; otherwise most people use `summarize`);
//!   2. an oracle: every rule is recomputed in plain Rust from the same parameters that generated its
//!      CEL text (`--oracle-every 1` = every employee, default 997 = a sample, always the first 200);
//!   3. fault isolation: ~0.1% of employees are deliberately broken and must fail *alone*.
//! It prints throughput, peak memory, and an order-independent checksum of all net pay, which must be
//! identical for any `--threads` value.

use std::collections::BTreeMap;
use std::time::Instant;

use rayon::prelude::*;
use rust_decimal::{Decimal, RoundingStrategy};
use serde_json::{json, Value as Json};
use sp_engine::{Compiled, Input, Payslip, Structure};

// ---------------------------------------------------------------------------------------------------
// Rules: one description produces both the CEL text and the plain-Rust oracle.

#[derive(Clone, Debug)]
enum Tmpl {
    Pct { rate: String },
    Bracket { table: usize },
    Capped { rate: String, cap: u32 },
    Region { rate: String },
    Age { rate: String },
    Excess { thresh: u32, frac: String },
    Chain { prev: usize },
    Dependents { amt: u32, cap: u32 },
    Union { rate: String },
    Prorate { days: u32, frac: String },
}

#[derive(Clone, Debug)]
struct Rule {
    id: String,
    kind: &'static str,
    tag: String,
    tmpl: Tmpl,
    region: u32,
}

const TABLES: usize = 100;
const REGIONS: u32 = 20;

fn splitmix(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E3779B97F4A7C15);
    let mut z = x;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
    z ^ (z >> 31)
}

/// Keeps total deductions near a quarter of gross however many rules there are.
fn shrink(max: u64, scale: f64) -> u64 {
    ((max as f64 / scale).ceil() as u64).max(1)
}

fn frac4(h: u64, max: u64) -> String {
    let v = 1 + h % max;
    format!("{}.{:04}", v / 10000, v % 10000)
}

fn make_rules(count: usize) -> Vec<Rule> {
    let scale = (count as f64 / 300.0).max(1.0);
    (0..count)
        .map(|i| {
            let h = splitmix(i as u64 * 7919 + 13);
            let m = |x: u64| shrink(x, scale);
            let tmpl = match i % 10 {
                0 => Tmpl::Pct { rate: frac4(h, m(40)) },
                1 => Tmpl::Bracket { table: (h as usize >> 8) % TABLES },
                2 => Tmpl::Capped { rate: frac4(h, m(60)), cap: 1 + (h >> 20) as u32 % m(40) as u32 },
                3 => Tmpl::Region { rate: frac4(h, m(400)) },
                4 => Tmpl::Age { rate: frac4(h, m(40)) },
                5 => Tmpl::Excess { thresh: 1000 + (h >> 12) as u32 % 20000, frac: format!("0.{:05}", 1 + h % m(30)) },
                6 => Tmpl::Chain { prev: i - 1 },
                7 => Tmpl::Dependents { amt: 1 + (h >> 9) as u32 % m(20) as u32, cap: 1 + (h >> 33) as u32 % m(100) as u32 },
                8 => Tmpl::Union { rate: frac4(h, m(60)) },
                _ => Tmpl::Prorate { days: 1 + (h >> 17) as u32 % 30, frac: format!("0.{:05}", 1 + h % m(25)) },
            };
            Rule {
                id: format!("r{i:05}"),
                kind: match i % 5 { 0..=2 => "deduction", 3 => "employer", _ => "info" },
                tag: format!("grp_{}", i % 50),
                tmpl,
                region: (h >> 40) as u32 % REGIONS,
            }
        })
        .collect()
}

fn tables() -> Vec<Json> {
    (0..TABLES)
        .map(|k| {
            let a = 50_000 + 5_000 * k as u64;
            json!([[a, 0], [a * 3, format!("0.{:05}", 1 + k % 9)], [null, format!("0.{:05}", 3 + k % 11)]])
        })
        .map(|t| {
            // Numbers, not strings: constants read JSON numbers exactly as written.
            let rows: Vec<Json> = t.as_array().unwrap().iter().map(|row| {
                let r = row.as_array().unwrap();
                json!([r[0], r[1].as_str().map_or(r[1].clone(), |s| serde_json::from_str(s).unwrap())])
            }).collect();
            Json::Array(rows)
        })
        .collect()
}

fn structure_json(rules: &[Rule]) -> Json {
    let mut components = vec![
        json!({"id": "base", "label": "Base salary", "kind": "earning", "input": "base_salary", "tags": ["pensionable"]}),
        json!({"id": "bonus_pay", "label": "Bonus", "kind": "earning", "input": "bonus"}),
        json!({"id": "overtime", "label": "Overtime", "kind": "earning", "formula": "overtime_hours * (base_salary / 160) * 1.5"}),
        json!({"id": "gross", "label": "Gross", "kind": "info", "formula": "total(\"earning\")"}),
        json!({"id": "pension_ee", "label": "Pension 5%", "kind": "deduction", "tags": ["pre_tax"], "formula": "pct(total(\"pensionable\"), 5)"}),
        json!({"id": "taxable", "label": "Taxable pay", "kind": "info", "formula": "gross - total(\"pre_tax\")"}),
    ];
    for r in rules {
        let formula = match &r.tmpl {
            Tmpl::Pct { rate } => format!("pct(taxable, {rate})"),
            Tmpl::Bracket { table } => format!("bracket(taxable * periods_per_year, tbl_{table}) / periods_per_year"),
            Tmpl::Capped { rate, cap } => format!("min(pct(gross, {rate}), {cap})"),
            Tmpl::Region { rate } => format!("pct(taxable, {rate})"),
            Tmpl::Age { rate } => format!("age >= 60 ? 0 : pct(taxable, {rate})"),
            Tmpl::Excess { thresh, frac } => format!("max(0, taxable - {thresh}) * {frac}"),
            Tmpl::Chain { prev } => format!("r{prev:05} * 0.05"),
            Tmpl::Dependents { amt, cap } => format!("dependents > 0 ? clamp(dependents * {amt}, 0, {cap}) : 0"),
            Tmpl::Union { rate } => format!("union_member ? pct(base, {rate}) : 0"),
            Tmpl::Prorate { days, frac } => format!("prorate(taxable, {days}, 30) * {frac}"),
        };
        let mut c = json!({"id": r.id, "label": format!("Demo rule {}", r.id), "kind": r.kind, "tags": [r.tag], "formula": formula});
        if matches!(r.tmpl, Tmpl::Region { .. }) {
            c["when"] = json!(format!("region == {}", r.region));
        }
        components.push(c);
    }
    for g in 0..50.min(rules.len()) {
        components.push(json!({"id": format!("sub_{g}"), "label": format!("Subtotal {g}"), "kind": "info", "formula": format!("total(\"grp_{g}\")")}));
    }
    let constants: BTreeMap<String, Json> = tables().into_iter().enumerate().map(|(k, t)| (format!("tbl_{k}"), t)).collect();
    json!({
        "id": "stress", "version": "1", "currency": "GMD",
        "inputs": [
            {"name": "base_salary"}, {"name": "bonus", "default": 0}, {"name": "overtime_hours", "default": 0},
            {"name": "dependents", "default": 0}, {"name": "region", "default": 0}, {"name": "age", "default": 30},
            {"name": "periods_per_year", "default": 12}, {"name": "union_member", "default": false}
        ],
        "constants": constants,
        "components": components
    })
}

// ---------------------------------------------------------------------------------------------------
// Employees, made from their index.

#[derive(Clone)]
struct Person {
    base: Decimal,
    bonus: Decimal,
    overtime: Decimal,
    dependents: i64,
    region: u32,
    age: i64,
    ppy: i64,
    union: bool,
    /// Some(reason) = deliberately broken, must fail.
    broken: Option<&'static str>,
}

fn person(seed: u64, index: u64) -> (Person, Input) {
    let h = splitmix(seed ^ index.wrapping_mul(0x9E3779B97F4A7C15));
    let h2 = splitmix(h);
    let base = Decimal::new(200_000 + (h % 6_000_000) as i64, 2);
    let has_bonus = h2 % 4 == 0;
    let bonus = if has_bonus { Decimal::new(((h2 >> 8) % 500_000) as i64, 2) } else { Decimal::ZERO };
    let has_ot = (h2 >> 3) % 10 < 3;
    let overtime = if has_ot { Decimal::new((((h2 >> 16) % 60) * 10 + 5) as i64, 1) } else { Decimal::ZERO };
    let dependents = ((h2 >> 24) % 7) as i64;
    let region = ((h2 >> 28) % REGIONS as u64) as u32;
    let age = 18 + ((h2 >> 36) % 53) as i64;
    let ppy = [12, 12, 12, 26, 52][((h2 >> 44) % 5) as usize];
    let union = (h2 >> 50) % 5 == 0;
    let mut vars: BTreeMap<String, Json> = BTreeMap::new();
    let num = |d: Decimal| serde_json::from_str::<Json>(&d.to_string()).unwrap();
    vars.insert("base_salary".into(), num(base));
    if has_bonus {
        vars.insert("bonus".into(), num(bonus));
    }
    if has_ot {
        vars.insert("overtime_hours".into(), num(overtime));
    }
    vars.insert("dependents".into(), json!(dependents));
    vars.insert("region".into(), json!(region));
    vars.insert("age".into(), json!(age));
    if ppy != 12 {
        vars.insert("periods_per_year".into(), json!(ppy));
    }
    if union {
        vars.insert("union_member".into(), json!(true));
    }
    let mut broken = None;
    if h % 1000 == 7 {
        match (h >> 20) % 6 {
            0 => {
                vars.remove("base_salary");
                broken = Some("missing required input");
            }
            1 => {
                vars.insert("base_salary".into(), json!("abc"));
                broken = Some("text where a number belongs");
            }
            2 => {
                vars.insert("bogus".into(), json!(1));
                broken = Some("unknown variable");
            }
            3 => {
                vars.insert("base_salary".into(), serde_json::from_str("79228162514264337593543950335").unwrap());
                broken = Some("amount at the limit of the number type");
            }
            4 => {
                vars.insert("base_salary".into(), json!(-500));
                broken = Some("negative salary");
            }
            _ => {
                vars.insert("age".into(), json!("sixty"));
                broken = Some("text where a number belongs (age)");
            }
        }
    }
    (Person { base, bonus, overtime, dependents, region, age, ppy, union, broken }, Input { employee_id: format!("E{index:08}"), variables: vars })
}

// ---------------------------------------------------------------------------------------------------
// The oracle: the same rules in plain Rust.

fn round2(d: Decimal) -> Decimal {
    let mut r = d.round_dp_with_strategy(2, RoundingStrategy::MidpointAwayFromZero);
    r.rescale(2);
    r
}

fn pct(x: Decimal, p: Decimal) -> Decimal {
    x * p / Decimal::from(100)
}

fn bracket(x: Decimal, table: &[(Option<Decimal>, Decimal)]) -> Decimal {
    if x <= Decimal::ZERO {
        return Decimal::ZERO;
    }
    let (mut tax, mut lower) = (Decimal::ZERO, Decimal::ZERO);
    for (upper, rate) in table {
        let top = upper.map_or(x, |u| u.min(x));
        if top > lower {
            tax += (top - lower) * rate;
        }
        match upper {
            Some(u) if x > *u => lower = *u,
            _ => break,
        }
    }
    tax
}

struct Oracle {
    rules: Vec<Rule>,
    tables: Vec<Vec<(Option<Decimal>, Decimal)>>,
}

impl Oracle {
    fn new(rules: Vec<Rule>) -> Self {
        let tables = tables()
            .iter()
            .map(|t| t.as_array().unwrap().iter().map(|row| {
                let r = row.as_array().unwrap();
                (r[0].as_u64().map(Decimal::from), r[1].to_string().parse::<Decimal>().unwrap())
            }).collect())
            .collect();
        Self { rules, tables }
    }

    /// Net pay by the oracle's own sums (earnings minus deductions), to say whether the engine was right to refuse someone.
    fn net(&self, p: &Person) -> Decimal {
        let expected = self.expect(p);
        let mut net = Decimal::ZERO;
        for (id, amount) in &expected {
            match id.as_str() {
                "base" | "bonus_pay" | "overtime" => net += amount,
                "pension_ee" => net -= amount,
                _ => {
                    if let Some(rule) = self.rules.iter().find(|r| &r.id == id) {
                        if rule.kind == "deduction" {
                            net -= amount;
                        }
                    }
                }
            }
        }
        net
    }

    /// (id, amount) for every component in the structure's written order.
    fn expect(&self, p: &Person) -> Vec<(String, Decimal)> {
        let mut out: Vec<(String, Decimal)> = Vec::new();
        let ot = round2(p.overtime * (p.base / Decimal::from(160)) * Decimal::new(15, 1));
        let base = round2(p.base);
        let bonus = round2(p.bonus);
        let gross = base + bonus + ot;
        let pension = round2(pct(base, Decimal::from(5)));
        let taxable = gross - pension;
        out.push(("base".into(), base));
        out.push(("bonus_pay".into(), bonus));
        out.push(("overtime".into(), ot));
        out.push(("gross".into(), round2(gross)));
        out.push(("pension_ee".into(), pension));
        out.push(("taxable".into(), round2(taxable)));
        let ppy = Decimal::from(p.ppy);
        let mut amounts: Vec<Decimal> = Vec::with_capacity(self.rules.len());
        let mut group = vec![Decimal::ZERO; 50];
        for (i, r) in self.rules.iter().enumerate() {
            let dec = |s: &str| s.parse::<Decimal>().unwrap();
            let raw = match &r.tmpl {
                Tmpl::Pct { rate } => pct(taxable, dec(rate)),
                Tmpl::Bracket { table } => bracket(taxable * ppy, &self.tables[*table]) / ppy,
                Tmpl::Capped { rate, cap } => pct(gross, dec(rate)).min(Decimal::from(*cap)),
                Tmpl::Region { rate } => if p.region == r.region { pct(taxable, dec(rate)) } else { Decimal::ZERO },
                Tmpl::Age { rate } => if p.age >= 60 { Decimal::ZERO } else { pct(taxable, dec(rate)) },
                Tmpl::Excess { thresh, frac } => Decimal::ZERO.max(taxable - Decimal::from(*thresh)) * dec(frac),
                Tmpl::Chain { prev } => amounts[*prev] * Decimal::new(5, 2),
                Tmpl::Dependents { amt, cap } => if p.dependents > 0 { (Decimal::from(p.dependents) * Decimal::from(*amt)).max(Decimal::ZERO).min(Decimal::from(*cap)) } else { Decimal::ZERO },
                Tmpl::Union { rate } => if p.union { pct(base, dec(rate)) } else { Decimal::ZERO },
                Tmpl::Prorate { days, frac } => taxable * Decimal::from(*days) / Decimal::from(30) * dec(frac),
            };
            let applied = !matches!(r.tmpl, Tmpl::Region { .. }) || p.region == r.region;
            let amount = if applied { round2(raw) } else { round2(Decimal::ZERO) };
            amounts.push(amount);
            if applied {
                group[i % 50] += amount;
            }
            out.push((r.id.clone(), amount));
        }
        for (g, total) in group.iter().enumerate().take(50.min(self.rules.len())) {
            out.push((format!("sub_{g}"), round2(*total)));
        }
        out
    }
}

// ---------------------------------------------------------------------------------------------------

struct Tally {
    ok: u64,
    failed_expected: u64,
    negative_net_refused: u64,
    failed_unexpected: u64,
    broken_that_passed: u64,
    invariant_violations: u64,
    oracle_checked: u64,
    oracle_mismatches: u64,
    checksum: u64,
    net_total: Decimal,
    rules_evaluated: u64,
    samples: Vec<String>,
    by_error: BTreeMap<String, u64>,
}

impl Tally {
    fn new() -> Self {
        Self { ok: 0, failed_expected: 0, negative_net_refused: 0, failed_unexpected: 0, broken_that_passed: 0, invariant_violations: 0, oracle_checked: 0, oracle_mismatches: 0, checksum: 0, net_total: Decimal::ZERO, rules_evaluated: 0, samples: Vec::new(), by_error: BTreeMap::new() }
    }
    fn note(&mut self, line: String) {
        if self.samples.len() < 12 {
            self.samples.push(line);
        }
    }
}

fn fnv(text: &str) -> u64 {
    text.bytes().fold(0xcbf29ce484222325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3))
}

fn rss_kb(field: &str) -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| s.lines().find(|l| l.starts_with(field)).and_then(|l| l.split_whitespace().nth(1)).and_then(|v| v.parse().ok()))
        .unwrap_or(0)
}

fn check(slip: &Payslip, structure: &Structure) -> bool {
    let sum = |kind: &str| slip.lines.iter().filter(|l| format!("{:?}", l.kind).to_lowercase() == kind).fold(Decimal::ZERO, |a, l| a + l.amount);
    let mut good = slip.lines.len() == structure.components.len();
    good &= slip.gross == sum("earning");
    good &= slip.deductions == sum("deduction");
    good &= slip.employer_cost == sum("employer");
    good &= slip.net == slip.gross - slip.deductions || !slip.shortfall.is_zero();
    good &= slip.net.scale() == 2 && slip.gross.scale() == 2;
    good
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str, default: &str| -> String { args.windows(2).find(|w| w[0] == name).map_or(default.to_string(), |w| w[1].clone()) };
    let mode = args.first().map(String::as_str).unwrap_or("run");
    let rules_n: usize = flag("--rules", "10000").parse().unwrap();
    let rules = make_rules(rules_n);
    if mode == "gen" {
        let out = flag("--out", "example_data/stress_structure.json");
        let text = serde_json::to_string(&structure_json(&rules)).unwrap();
        std::fs::write(&out, &text).unwrap();
        // The same people as lines, for the streaming form: `--structure <out> --employees <ndjson>`.
        let count: u64 = flag("--employees", "20").parse().unwrap();
        let ndjson = out.replace(".json", "_employees.ndjson");
        let mut lines = String::new();
        for i in 0..count {
            let (_, inp) = person(1, i);
            lines.push_str(&serde_json::to_string(&json!({"employee_id": inp.employee_id, "variables": inp.variables})).unwrap());
            lines.push('\n');
        }
        std::fs::write(&ndjson, lines).unwrap();
        println!("wrote {out} ({} bytes, {} rules) and {ndjson} ({count} people)", text.len(), rules_n);
        return;
    }

    let employees: u64 = flag("--employees", "100000").parse().unwrap();
    let seed: u64 = flag("--seed", "1").parse().unwrap();
    let threads: usize = flag("--threads", "0").parse().unwrap();
    let chunk: usize = flag("--chunk", "2048").parse().unwrap();
    let oracle_every: u64 = flag("--oracle-every", "997").parse().unwrap();
    let first: u64 = flag("--first", "200").parse().unwrap();
    let full = args.iter().any(|a| a == "--full");
    if threads > 0 {
        rayon::ThreadPoolBuilder::new().num_threads(threads).build_global().unwrap();
    }
    let started = Instant::now();
    let structure: Structure = match args.windows(2).find(|w| w[0] == "--structure") {
        Some(w) => serde_json::from_str(&std::fs::read_to_string(&w[1]).unwrap()).unwrap(),
        None => serde_json::from_value(structure_json(&rules)).unwrap(),
    };
    let t_parse = started.elapsed();
    let t0 = Instant::now();
    let compiled = Compiled::new(&structure).unwrap_or_else(|e| panic!("compile failed: {e}"));
    let t_compile = t0.elapsed();
    let oracle = Oracle::new(rules);
    println!("structure: {} components ({} rules), parse {:.2?}, compile {:.2?}, rss {} MB", structure.components.len(), rules_n, t_parse, t_compile, rss_kb("VmRSS:") / 1024);

    let mut total = Tally::new();
    let t_run = Instant::now();
    let mut next_report = employees / 20;
    let mut start = 0u64;
    while start < employees {
        let end = (start + chunk as u64).min(employees);
        let made: Vec<(Person, Input)> = (start..end).into_par_iter().map(|i| person(seed, i)).collect();
        let inputs: Vec<&Input> = made.iter().map(|(_, i)| i).collect();
        // Fold each payslip into a Tally right away so memory stays at "threads x one payslip".
        let tally = inputs
            .par_iter()
            .zip(made.par_iter())
            .enumerate()
            .fold(Tally::new, |mut t, (offset, (input, (p, _)))| {
                let index = start + offset as u64;
                let sampled = full || index < first || (oracle_every > 0 && index % oracle_every == 0);
                // Most people: totals only. The sampled ones: the whole payslip, checked line by line.
                let outcome = if sampled { compiled.run(input, false).map(|slip| (slip.net, Some(slip))) } else { compiled.summarize(input).map(|sum| (sum.net, None)) };
                match outcome {
                    Ok((net, slip)) => {
                        if let Some(why) = p.broken {
                            t.broken_that_passed += 1;
                            t.note(format!("{}: broken row ({why}) was accepted", input.employee_id));
                        }
                        t.ok += 1;
                        t.rules_evaluated += structure.components.len() as u64;
                        if let Some(slip) = slip {
                            if !check(&slip, &structure) {
                                t.invariant_violations += 1;
                                t.note(format!("{}: invariant violated", input.employee_id));
                            }
                            t.oracle_checked += 1;
                            let want = oracle.expect(p);
                            let got: Vec<(&str, Decimal)> = slip.lines.iter().map(|l| (l.id.as_str(), l.amount)).collect();
                            let bad = want.iter().zip(&got).find(|((wid, wamt), (gid, gamt))| wid != gid || wamt != gamt);
                            if want.len() != got.len() || bad.is_some() {
                                t.oracle_mismatches += 1;
                                let detail = bad.map_or("length".to_string(), |((wid, wamt), (_, gamt))| format!("{wid}: oracle {wamt}, engine {gamt}"));
                                t.note(format!("{}: oracle disagrees: {detail}", input.employee_id));
                            }
                        }
                        t.checksum = t.checksum.wrapping_add(fnv(&format!("{}:{}", input.employee_id, net)));
                        t.net_total += net;
                    }
                    Err(e) => {
                        let text = e.to_string();
                        if p.broken.is_some() {
                            t.failed_expected += 1;
                        } else if text.contains("below zero") && oracle.net(p) < Decimal::ZERO {
                            // The rules really do take more than this person earns: refusing is the right answer.
                            t.negative_net_refused += 1;
                        } else {
                            t.failed_unexpected += 1;
                            t.note(format!("{}: unexpected error: {e}", input.employee_id));
                        }
                        let key: String = e.to_string().split(": ").skip(1).collect::<Vec<_>>().join(": ").chars().take(70).collect();
                        *t.by_error.entry(key).or_default() += 1;
                    }
                }
                t
            })
            .reduce(Tally::new, |mut a, b| {
                a.ok += b.ok;
                a.failed_expected += b.failed_expected;
                a.negative_net_refused += b.negative_net_refused;
                a.failed_unexpected += b.failed_unexpected;
                a.broken_that_passed += b.broken_that_passed;
                a.invariant_violations += b.invariant_violations;
                a.oracle_checked += b.oracle_checked;
                a.oracle_mismatches += b.oracle_mismatches;
                a.checksum = a.checksum.wrapping_add(b.checksum);
                a.net_total += b.net_total;
                a.rules_evaluated += b.rules_evaluated;
                for s in b.samples {
                    a.note(s);
                }
                for (k, v) in b.by_error {
                    *a.by_error.entry(k).or_default() += v;
                }
                a
            });
        total.ok += tally.ok;
        total.failed_expected += tally.failed_expected;
        total.negative_net_refused += tally.negative_net_refused;
        total.failed_unexpected += tally.failed_unexpected;
        total.broken_that_passed += tally.broken_that_passed;
        total.invariant_violations += tally.invariant_violations;
        total.oracle_checked += tally.oracle_checked;
        total.oracle_mismatches += tally.oracle_mismatches;
        total.checksum = total.checksum.wrapping_add(tally.checksum);
        total.net_total += tally.net_total;
        total.rules_evaluated += tally.rules_evaluated;
        for s in tally.samples {
            total.note(s);
        }
        for (k, v) in tally.by_error {
            *total.by_error.entry(k).or_default() += v;
        }
        start = end;
        if start >= next_report && employees >= 20 {
            let secs = t_run.elapsed().as_secs_f64();
            println!("  {start}/{employees}  {:.0} employees/s  rss {} MB", start as f64 / secs, rss_kb("VmRSS:") / 1024);
            next_report += employees / 20;
        }
    }
    let secs = t_run.elapsed().as_secs_f64();
    println!("\n== result ==");
    println!("employees          {employees}  (ok {}, broken rows refused {}, refused for negative net (oracle agrees) {}, failed unexpectedly {}, broken-but-accepted {})", total.ok, total.failed_expected, total.negative_net_refused, total.failed_unexpected, total.broken_that_passed);
    println!("rule evaluations   {} ({:.1} M/s)", total.rules_evaluated, total.rules_evaluated as f64 / secs / 1e6);
    println!("time               {secs:.2}s = {:.0} employees/s", employees as f64 / secs);
    println!("peak memory        {} MB (resident high-water mark)", rss_kb("VmHWM:") / 1024);
    println!("invariants         {} violations", total.invariant_violations);
    println!("oracle             {} employees recomputed in plain Rust, {} mismatches", total.oracle_checked, total.oracle_mismatches);
    println!("net pay total      {}", total.net_total);
    println!("checksum           {:016x}", total.checksum);
    for (k, v) in &total.by_error {
        println!("  error x{v}: {k}");
    }
    for s in &total.samples {
        println!("  !! {s}");
    }
    let bad = total.failed_unexpected + total.broken_that_passed + total.invariant_violations + total.oracle_mismatches;
    if bad > 0 {
        std::process::exit(1);
    }
}
