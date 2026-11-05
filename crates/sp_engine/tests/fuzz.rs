//! Fuzz and property tests for the payroll engine. Seeded, dependency-free, replayable.
//! Scale with `FUZZ_ITERS=200000`.

use std::collections::BTreeMap;
use std::panic::{catch_unwind, AssertUnwindSafe};

use rust_decimal::Decimal;
use serde_json::{json, Value as Json};
use sp_engine::{Compiled, Input, Kind, NegativeNet, Payslip, Structure};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545F4914F6CDD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}

/// `FUZZ_ITERS` sets the count; by default a quick pass (debug builds are ten times slower).
fn iters(default: usize) -> usize {
    let default = if cfg!(debug_assertions) { default / 10 } else { default };
    std::env::var("FUZZ_ITERS").ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

const NUMBERS: &[&str] = &["0", "1", "0.01", "0.005", "2.5", "1000", "123456.789", "-5", "-0.004", "79228162514264337593543950335", "-79228162514264337593543950335", "0.0000000000000000000000000001", "1e5", "12345678901234567890.123456789"];
const KINDS: &[&str] = &["earning", "deduction", "employer", "info"];
const MODES: &[&str] = &["half_up", "half_even", "down", "up"];

/// A random structure whose formulas only refer to earlier components, so it is always acyclic.
fn random_structure(rng: &mut Rng) -> Json {
    let n_inputs = 1 + rng.below(4);
    let inputs: Vec<Json> = (0..n_inputs).map(|i| if rng.below(2) == 0 { json!({"name": format!("in{i}"), "default": 0}) } else { json!({"name": format!("in{i}")}) }).collect();
    let mut comps: Vec<Json> = Vec::new();
    let n = 1 + rng.below(12);
    for i in 0..n {
        let kind = *rng.pick(KINDS);
        let mut c = json!({"id": format!("c{i}"), "label": format!("component {i}"), "kind": kind});
        let earlier: Vec<String> = (0..i).map(|j| format!("c{j}")).collect();
        let term = |rng: &mut Rng| -> String {
            match rng.below(5) {
                0 if !earlier.is_empty() => rng.pick(&earlier).clone(),
                1 => format!("in{}", rng.below(n_inputs)),
                2 => rng.pick(NUMBERS).trim_start_matches('-').to_string(),
                3 => format!("total(\"{}\")", rng.pick(&["earning", "deduction", "info"])),
                _ => format!("in{}", rng.below(n_inputs)),
            }
        };
        match rng.below(5) {
            0 => c["amount"] = json!(rng.pick(&[0, 1, 100, 250])),
            1 => c["input"] = json!(format!("in{}", rng.below(n_inputs))),
            _ => {
                let a = term(rng);
                let b = term(rng);
                c["formula"] = json!(match rng.below(6) {
                    0 => format!("{a} + {b}"),
                    1 => format!("{a} * {b}"),
                    2 => format!("{a} - {b}"),
                    3 => format!("pct({a}, {b})"),
                    4 => format!("max({a}, {b}) / 3"),
                    _ => format!("{a} > {b} ? {a} : {b}"),
                });
            }
        }
        if rng.below(4) == 0 {
            c["when"] = json!(format!("in{} > 0", rng.below(n_inputs)));
        }
        if rng.below(3) == 0 {
            c["tags"] = json!([*rng.pick(&["pre_tax", "pensionable", "earning", "x"])]);
        }
        if rng.below(5) == 0 {
            c["round"] = json!({"digits": rng.below(9), "mode": *rng.pick(MODES)});
        }
        if rng.below(3) == 0 {
            c["allow_negative"] = json!(true);
        }
        comps.push(c);
    }
    json!({
        "id": "fuzz", "version": "1", "currency": "XXX",
        "rounding": {"digits": rng.below(9), "mode": *rng.pick(MODES)},
        "negative_net": *rng.pick(&["error", "zero", "allow"]),
        "inputs": inputs, "components": comps
    })
}

fn random_input(rng: &mut Rng, structure: &Json, id: usize) -> Input {
    let mut variables = BTreeMap::new();
    for spec in structure["inputs"].as_array().unwrap() {
        let name = spec["name"].as_str().unwrap();
        match rng.below(8) {
            0 => {} // missing: an error unless it has a default
            1 => {
                variables.insert(name.to_string(), json!("not a number"));
            }
            2 => {
                variables.insert(name.to_string(), Json::Null);
            }
            _ => {
                let text = *rng.pick(NUMBERS);
                variables.insert(name.to_string(), serde_json::from_str(text).unwrap());
            }
        }
    }
    if rng.below(20) == 0 {
        variables.insert("surprise".to_string(), json!(1));
    }
    Input { employee_id: format!("e{id}"), variables }
}

fn check_slip(slip: &Payslip, s: &Structure) {
    let digits = s.rounding.digits;
    let sum = |k: Kind| slip.lines.iter().filter(|l| l.kind == k).fold(Decimal::ZERO, |a, l| a + l.amount);
    for l in &slip.lines {
        if let Some(c) = s.components.iter().find(|c| c.id == l.id) {
            // Near the top of the number type there is no room left for decimals.
            if l.amount.abs() < Decimal::from(10u64.pow(15)) {
                assert_eq!(l.amount.scale(), c.round.map_or(digits, |r| r.digits), "{} has the wrong scale: {}", l.id, l.amount);
            }
        }
        if !l.applied {
            assert!(l.amount.is_zero(), "{} not applied but has an amount", l.id);
        }
    }
    // Totals are exact sums of the rounded lines (the round of an already-rounded sum can only change it
    // when the line digits exceed the structure's).
    if s.components.iter().all(|c| c.round.is_none()) {
        assert_eq!(slip.gross, sum(Kind::Earning));
        assert_eq!(slip.deductions, sum(Kind::Deduction));
        assert_eq!(slip.employer_cost, sum(Kind::Employer));
    }
    if s.negative_net == NegativeNet::Allow && s.components.iter().all(|c| c.round.is_none()) {
        assert_eq!(slip.net, slip.gross - slip.deductions);
    }
    if s.negative_net == NegativeNet::Zero {
        assert!(!slip.net.is_sign_negative() || slip.net.is_zero(), "net below zero: {}", slip.net);
    }
    if s.negative_net == NegativeNet::Error {
        assert!(!slip.net.is_sign_negative() || slip.net.is_zero(), "net below zero under `error`: {}", slip.net);
    }
}

#[test]
fn random_structures_and_people_never_panic_and_keep_their_invariants() {
    let mut rng = Rng(0xC0FFEE_u64 << 8 | 1);
    let (mut ok, mut failed) = (0usize, 0usize);
    // A fingerprint of every answer (payslip or error text), to compare one version of the engine with another.
    let mut digest = 0xcbf29ce484222325u64;
    let mut mix = |text: String| text.bytes().for_each(|b| digest = (digest ^ b as u64).wrapping_mul(0x100000001b3));
    for round in 0..iters(30_000) {
        let structure_json = random_structure(&mut rng);
        let structure: Structure = serde_json::from_value(structure_json.clone()).unwrap();
        let compiled = match Compiled::new(&structure) {
            Ok(c) => c,
            Err(_) => continue,
        };
        for p in 0..4 {
            let input = random_input(&mut rng, &structure_json, p);
            let outcome = catch_unwind(AssertUnwindSafe(|| compiled.run(&input, round % 2 == 0)));
            match outcome {
                Err(_) => panic!("PANIC\nstructure: {structure_json}\ninput: {:?}", input),
                Ok(Ok(slip)) => {
                    ok += 1;
                    mix(format!("{slip:?}"));
                    check_slip(&slip, &structure);
                    // Same answer twice, and the totals-only path agrees with the full one.
                    assert_eq!(slip, compiled.run(&input, round % 2 == 0).unwrap());
                    let summary = compiled.summarize(&input).unwrap();
                    assert_eq!((summary.gross, summary.deductions, summary.net, summary.employer_cost, summary.shortfall), (slip.gross, slip.deductions, slip.net, slip.employer_cost, slip.shortfall));
                }
                Ok(Err(e)) => {
                    failed += 1;
                    mix(e.to_string());
                }
            }
        }
    }
    eprintln!("{ok} payslips, {failed} reported errors, DIGEST {digest:016x}");
    assert!(ok > 1000, "the generator should produce mostly calculable people (got {ok})");
}

/// The order components are written in must not change what any of them comes to.
#[test]
fn component_order_does_not_change_the_numbers() {
    let mut rng = Rng(77);
    for _ in 0..iters(10_000) {
        let structure_json = random_structure(&mut rng);
        let mut shuffled = structure_json.clone();
        let comps = shuffled["components"].as_array_mut().unwrap();
        for i in (1..comps.len()).rev() {
            comps.swap(i, rng.below(i + 1));
        }
        let (a, b): (Structure, Structure) = (serde_json::from_value(structure_json.clone()).unwrap(), serde_json::from_value(shuffled).unwrap());
        let (Ok(ca), Ok(cb)) = (Compiled::new(&a), Compiled::new(&b)) else { continue };
        for p in 0..3 {
            let input = random_input(&mut rng, &structure_json, p);
            match (ca.run(&input, false), cb.run(&input, false)) {
                (Ok(x), Ok(y)) => {
                    // Beyond ~1e15 the 96-bit decimal drops digits and addition order can matter.
                    if x.lines.iter().any(|l| l.amount.abs() >= Decimal::from(10u64.pow(15))) {
                        continue;
                    }
                    let map = |s: &Payslip| s.lines.iter().map(|l| (l.id.clone(), l.amount)).collect::<BTreeMap<_, _>>();
                    assert_eq!(map(&x), map(&y), "order changed numbers\n{structure_json}");
                    assert_eq!((x.gross, x.deductions, x.net, x.employer_cost), (y.gross, y.deductions, y.net, y.employer_cost));
                }
                (Err(_), Err(_)) => {}
                // A component that fails in one order only (for example a negative `info`-free line) is
                // acceptable only if both orders agree whether the person can be paid at all.
                (x, y) if format!("{x:?}{y:?}").contains("too large") => {}
                // Amounts at the very edge of the number type: addition order decides which digits are lost.
                (Ok(p), _) | (_, Ok(p)) if p.lines.iter().any(|l| l.amount.abs() >= Decimal::from(10u64.pow(15))) => {}
                (x, y) => panic!("order changed whether the person can be calculated: {x:?} vs {y:?}\n{structure_json}"),
            }
        }
    }
}

#[test]
fn run_all_equals_one_at_a_time() {
    let mut rng = Rng(5);
    for _ in 0..iters(2_000) {
        let structure_json = random_structure(&mut rng);
        let structure: Structure = serde_json::from_value(structure_json.clone()).unwrap();
        let Ok(compiled) = Compiled::new(&structure) else { continue };
        let inputs: Vec<Input> = (0..40).map(|p| random_input(&mut rng, &structure_json, p)).collect();
        let together = compiled.run_all(&inputs, false);
        for (input, got) in inputs.iter().zip(&together) {
            assert_eq!(got, &compiled.run(input, false));
        }
    }
}

/// Corrupt a valid structure's JSON in small ways: loading it may fail, never panic.
#[test]
fn mutated_structures_never_panic() {
    let mut rng = Rng(99);
    let weird: &[Json] = &[json!(null), json!(-1), json!(1e300), json!(""), json!([]), json!({}), json!("x y"), json!(100), json!(29), json!(u64::MAX)];
    for _ in 0..iters(30_000) {
        let mut s = random_structure(&mut rng);
        for _ in 0..1 + rng.below(3) {
            let comps = s["components"].as_array_mut().unwrap();
            let i = rng.below(comps.len());
            let key = *rng.pick(&["id", "label", "kind", "formula", "amount", "input", "when", "tags", "round", "allow_negative"]);
            let value = rng.pick(weird).clone();
            if rng.below(3) == 0 {
                comps[i].as_object_mut().unwrap().remove(key);
            } else {
                comps[i][key] = value;
            }
            if rng.below(6) == 0 {
                s["rounding"] = json!({"digits": *rng.pick(&[0, 8, 9, 28, 29, 100, 4294967295u64]), "mode": "half_up"});
            }
        }
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            if let Ok(structure) = serde_json::from_value::<Structure>(s.clone()) {
                if let Ok(compiled) = Compiled::new(&structure) {
                    let input = random_input(&mut Rng(1), &s, 0);
                    let _ = compiled.run(&input, true);
                }
            }
        }));
        assert!(outcome.is_ok(), "PANIC on structure: {s}");
    }
}

/// Money near the limit of the number type must give an error, not abort the whole payroll run.
#[test]
fn amounts_at_the_edge_of_the_number_type_fail_cleanly() {
    let max = "79228162514264337593543950335";
    let s: Structure = serde_json::from_value(json!({
        "id": "edge", "version": "1", "currency": "XXX", "negative_net": "allow",
        "inputs": [{"name": "a"}, {"name": "b"}],
        "components": [
            {"id": "x", "label": "x", "kind": "earning", "input": "a"},
            {"id": "y", "label": "y", "kind": "earning", "input": "b"},
            {"id": "d", "label": "d", "kind": "deduction", "input": "a"}
        ]
    })).unwrap();
    let c = Compiled::new(&s).unwrap();
    for (a, b) in [(max, max), (max, "1"), ("1", max), (max, "0")] {
        let input = Input { employee_id: "e".into(), variables: BTreeMap::from([("a".into(), serde_json::from_str(a).unwrap()), ("b".into(), serde_json::from_str(b).unwrap())]) };
        let outcome = catch_unwind(AssertUnwindSafe(|| c.run(&input, false)));
        assert!(outcome.is_ok(), "panic for {a} + {b}");
    }
}
