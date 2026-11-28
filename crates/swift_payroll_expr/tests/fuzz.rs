//! Fuzz and property tests for the expression language. No external crates: a seeded generator, so
//! every failure prints its seed and source and can be replayed. Scale with `FUZZ_ITERS=2000000`.
//!
//! What must hold for *any* text a rule author (or an attacker with write access to a rule repository)
//! can type: compiling and evaluating never panics, never overflows the stack, never hangs, and gives
//! the same answer every time.

use std::collections::BTreeMap;
use std::panic::{catch_unwind, AssertUnwindSafe};

use rust_decimal::Decimal;
use swift_payroll_expr::{compile, ErrorKind, MapEnv, Precision, Value};

mod oracle;
use oracle::Exact;

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

fn env() -> MapEnv {
    let mut m = BTreeMap::new();
    m.insert("a".to_string(), Value::from(1000));
    m.insert("b".to_string(), Value::Num(Decimal::new(-12345, 2)));
    m.insert("big".to_string(), Value::Num(Decimal::MAX));
    m.insert("tiny".to_string(), Value::Num(Decimal::new(1, 28)));
    m.insert("s".to_string(), Value::from("text"));
    m.insert("t".to_string(), Value::Bool(true));
    m.insert("l".to_string(), Value::List(vec![Value::from(1), Value::from(2), Value::from(3)]));
    m.insert(
        "table".to_string(),
        Value::List(vec![Value::List(vec![Value::from(100), Value::Num(Decimal::new(1, 1))]), Value::List(vec![Value::Null, Value::Num(Decimal::new(2, 1))])]),
    );
    MapEnv(m)
}

/// Compile + evaluate twice; panics are the failure. Returns the outcome text for the determinism check.
fn exercise(source: &str) -> String {
    let outcome = catch_unwind(AssertUnwindSafe(|| match compile(source) {
        Err(e) => format!("compile error: {e:?}"),
        Ok(expr) => {
            let _ = expr.identifiers();
            let _ = expr.literal_arguments("total");
            let env = env();
            let first = format!("{:?}", expr.eval(&env));
            let second = format!("{:?}", expr.eval(&env));
            assert_eq!(first, second, "non-deterministic");
            first
        }
    }));
    match outcome {
        Ok(text) => text,
        Err(_) => panic!("PANIC on source: {source:?}"),
    }
}

const FUNCS: &[&str] = &[
    "min",
    "max",
    "abs",
    "floor",
    "ceil",
    "trunc",
    "round",
    "round_even",
    "clamp",
    "between",
    "pct",
    "prorate",
    "bracket",
    "rate_at",
    "sum",
    "size",
    "coalesce",
    "dec",
    "string",
];
const ATOMS: &[&str] = &[
    "0",
    "1",
    "2",
    "7",
    "100",
    "0.5",
    "0.0000000000000000000000000001",
    "79228162514264337593543950335",
    "99999999999999999999999999999",
    "-1",
    "a",
    "b",
    "big",
    "tiny",
    "s",
    "t",
    "l",
    "table",
    "null",
    "true",
    "false",
    "\"12.5\"",
    "\"\"",
    "[1,2,3]",
    "[]",
    "{}",
    "{\"k\": 1}",
    "[[100,0.1],[null,0.2]]",
    "[[1,2]]",
    "[[null,1],[2,3]]",
];
const BINOPS: &[&str] = &["+", "-", "*", "/", "%", "==", "!=", "<", "<=", ">", ">=", "&&", "||", " in "];

fn gen(rng: &mut Rng, depth: usize) -> String {
    if depth == 0 || rng.below(5) == 0 {
        return rng.pick(ATOMS).to_string();
    }
    match rng.below(8) {
        0..=2 => format!("({} {} {})", gen(rng, depth - 1), rng.pick(BINOPS), gen(rng, depth - 1)),
        3 => format!("{}({})", rng.pick(FUNCS), (0..rng.below(5)).map(|_| gen(rng, depth - 1)).collect::<Vec<_>>().join(", ")),
        4 => format!("({} ? {} : {})", gen(rng, depth - 1), gen(rng, depth - 1), gen(rng, depth - 1)),
        5 => format!("{}{}", rng.pick(&["-", "!"]), gen(rng, depth - 1)),
        6 => format!("{}[{}]", gen(rng, depth - 1), gen(rng, depth - 1)),
        _ => format!("{}.{}", gen(rng, depth - 1), rng.pick(&["k", "x", "size"])),
    }
}

#[test]
fn generated_expressions_never_panic() {
    let mut rng = Rng(0x9E3779B97F4A7C15);
    for _ in 0..iters(200_000) {
        let depth = 1 + rng.below(6);
        exercise(&gen(&mut rng, depth));
    }
}

#[test]
fn token_soup_never_panics() {
    let pieces: Vec<&str> = ATOMS
        .iter()
        .copied()
        .chain(FUNCS.iter().copied())
        .chain(BINOPS.iter().copied())
        .chain(["(", ")", "[", "]", "{", "}", ",", ".", "?", ":", "!", "//", "\n", "\"", "'", "\\", " "])
        .collect();
    let mut rng = Rng(0xDEADBEEFCAFEF00D);
    for _ in 0..iters(300_000) {
        let n = 1 + rng.below(14);
        let source: String = (0..n).map(|_| *rng.pick(&pieces)).collect::<Vec<_>>().join(if rng.below(2) == 0 { "" } else { " " });
        exercise(&source);
    }
}

#[test]
fn mutated_bytes_never_panic() {
    let seeds = [
        "pct(base, 20) + bracket(x * 12, table) / 12",
        "a > 0 ? min(a, 100) : coalesce(b, 0)",
        "{\"k\": [1,2,3]}[\"k\"][1]",
        "round(a / 3, 2) // comment\n + 1",
        "\"héllo wörld ✓\" + s",
    ];
    let junk: &[&str] = &["\u{a0}", "\u{85}", "é", "✓", "\u{0}", "\u{202e}", "💥", "\r", "\t", "\u{feff}"];
    let mut rng = Rng(0x1234_5678_9ABC_DEF1);
    for _ in 0..iters(300_000) {
        let mut s = rng.pick(&seeds).to_string();
        for _ in 0..1 + rng.below(4) {
            let mut at = rng.below(s.len() + 1);
            while !s.is_char_boundary(at) {
                at -= 1;
            }
            match rng.below(3) {
                0 => s.insert_str(at, rng.pick::<&str>(junk)),
                1 => {
                    let end = (at + rng.below(6)).min(s.len());
                    let mut end = end;
                    while !s.is_char_boundary(end) {
                        end += 1;
                    }
                    s.replace_range(at..end, "");
                }
                _ => s.insert(at, (b' ' + rng.below(95) as u8) as char),
            }
        }
        exercise(&s);
    }
}

/// Arithmetic must agree with an independent integer oracle.
#[test]
fn integer_arithmetic_matches_i128() {
    fn build(rng: &mut Rng, depth: usize) -> (String, Option<i128>) {
        if depth == 0 || rng.below(4) == 0 {
            let n = rng.below(2001) as i128 - 1000;
            return (if n < 0 { format!("({n})") } else { n.to_string() }, Some(n));
        }
        let (ls, lv) = build(rng, depth - 1);
        let (rs, rv) = build(rng, depth - 1);
        let op = *rng.pick(&["+", "-", "*"]);
        let value = match (lv, rv) {
            (Some(x), Some(y)) => match op {
                "+" => x.checked_add(y),
                "-" => x.checked_sub(y),
                _ => x.checked_mul(y),
            },
            _ => None,
        };
        (format!("({ls} {op} {rs})"), value)
    }
    let mut rng = Rng(42);
    for _ in 0..iters(100_000) {
        let (source, want) = build(&mut rng, 5);
        let Some(want) = want else { continue };
        if want.unsigned_abs() > 79_228_162_514_264_337_593_543_950_335u128 {
            continue;
        }
        let got = compile(&source).unwrap().eval(&MapEnv::default()).unwrap();
        assert_eq!(got, Value::Num(Decimal::from_i128_with_scale(want, 0)), "{source}");
    }
}

/// Rule text is untrusted input too: a very long or very deep one must be refused or handled, not crash
/// the process (a stack overflow aborts and cannot be caught).
#[test]
fn long_and_deep_inputs_do_not_crash() {
    let n = 200_000;
    let sum = vec!["1"; n].join("+");
    let cases: Vec<(&str, String)> = vec![
        ("long sum", sum),
        ("long product", vec!["1"; n].join("*")),
        ("long and-chain", vec!["true"; n].join("&&")),
        ("long comparison chain", vec!["1"; n].join("<")),
        ("long postfix chain", format!("a{}", ".k".repeat(n))),
        ("long index chain", format!("l{}", "[0]".repeat(n))),
        ("deep parens", format!("{}1{}", "(".repeat(n), ")".repeat(n))),
        ("deep unary", format!("{}1", "-".repeat(n))),
        ("deep ternary", format!("{}1{}", "true ? ".repeat(n), " : 0".repeat(n))),
        ("deep list", format!("{}1{}", "[".repeat(n), "]".repeat(n))),
        ("deep call", format!("{}1{}", "abs(".repeat(n), ")".repeat(n))),
        ("huge list literal", format!("[{}]", vec!["1"; n].join(","))),
        ("huge string", format!("\"{}\"", "x".repeat(5_000_000))),
    ];
    for (name, source) in cases {
        // Run on a thread with the default (2 MiB) test stack, which is what the host program has too.
        let handle = std::thread::Builder::new().stack_size(2 * 1024 * 1024).spawn(move || {
            let outcome = compile(&source);
            if let Ok(expr) = outcome {
                let _ = expr.identifiers();
                let _ = expr.eval(&env());
                let copy = expr.clone();
                drop(expr);
                drop(copy);
            }
        });
        handle.unwrap().join().unwrap_or_else(|_| panic!("{name}: panicked"));
        eprintln!("ok: {name}");
    }
}

/// A legitimate formula that adds up a few hundred named terms must work.
#[test]
fn a_wide_formula_is_not_refused() {
    for terms in [10, 100, 127, 128, 129, 500, 5000] {
        let source = (0..terms).map(|_| "a").collect::<Vec<_>>().join(" + ");
        let expr = compile(&source).unwrap_or_else(|e| panic!("{terms} terms did not compile: {e}"));
        let got = expr.eval(&env()).unwrap_or_else(|e| panic!("{terms} terms failed to evaluate: {e}"));
        assert_eq!(got, Value::Num(Decimal::from(1000 * terms as i64)));
    }
}

#[test]
fn extreme_numbers_error_instead_of_panicking() {
    for source in [
        "prorate(79228162514264337593543950335, 1, 0.0000000000000000000000000001)",
        "pct(79228162514264337593543950335, 79228162514264337593543950335)",
        "big + big",
        "big * 2",
        "-big - big",
        "big / tiny",
        "big % tiny",
        "sum([big, big])",
        "bracket(big, [[null, big]])",
        "round(big, 10)",
        "abs(-big)",
        "ceil(big)",
        "dec(\"7.9228162514264337593543950336e28\")",
        "dec(\"1e999\")",
        "min()",
        "max([])",
    ] {
        exercise(source);
    }
}

/// The deepest tree the parser allows must still evaluate on a small stack.
#[test]
fn the_deepest_allowed_tree_fits_a_small_stack() {
    let mut source = String::from("1");
    for _ in 0..60 {
        source = format!("(t ? {source} : 0)");
        source = format!("-(l{}[0] * 0 + {source})", "[0]".repeat(0));
    }
    let handle = std::thread::Builder::new().stack_size(512 * 1024).spawn(move || {
        if let Ok(expr) = compile(&source) {
            let _ = expr.eval(&env());
            let _ = expr.identifiers();
            drop(expr.clone());
        }
    });
    handle.unwrap().join().expect("deep tree overflowed a 512 KiB stack");
}

// ---- money arithmetic against an independent oracle
//
// The oracle keeps a number as an exact `mantissa / 10^scale` in i128 and never rounds, so every result the
// engine gives can be checked digit for digit. Cases the oracle cannot hold are skipped, never guessed.

/// A random amount: up to ±1000.00, with 0 to 2 decimals.
fn amount(rng: &mut Rng) -> Decimal {
    Decimal::new(rng.below(200_001) as i64 - 100_000, rng.below(3) as u32)
}

fn literal(d: Decimal) -> String {
    if d.is_sign_negative() {
        format!("({d})")
    } else {
        d.to_string()
    }
}

fn run(source: &str, env: &MapEnv, precision: Precision) -> Result<Value, swift_payroll_expr::Error> {
    compile(source).unwrap_or_else(|e| panic!("{source}: {e}")).eval_with(env, precision)
}

/// `+ - *` on amounts with decimals give exactly the oracle's answer, in both precisions, whenever every
/// step fits the number type.
#[test]
fn decimal_arithmetic_matches_the_exact_oracle() {
    fn build(rng: &mut Rng, depth: usize) -> (String, Option<Exact>) {
        if depth == 0 || rng.below(4) == 0 {
            let d = amount(rng);
            return (literal(d), Some(Exact::of(d)));
        }
        let (ls, lv) = build(rng, depth - 1);
        let (rs, rv) = build(rng, depth - 1);
        let op = *rng.pick(&["+", "-", "*"]);
        let value = match (lv, rv) {
            (Some(x), Some(y)) => match op {
                "+" => x.add(y),
                "-" => x.add(y.neg()),
                _ => x.mul(y),
            }
            .and_then(Exact::reduced),
            _ => None,
        };
        (format!("({ls} {op} {rs})"), value)
    }
    let env = MapEnv::default();
    let mut rng = Rng(7);
    let mut checked = 0;
    for _ in 0..iters(100_000) {
        let (source, want) = build(&mut rng, 4);
        let Some(want) = want else { continue };
        for precision in [Precision::Round, Precision::Exact] {
            assert_eq!(run(&source, &env, precision), Ok(Value::Num(want.decimal())), "{source} ({precision:?})");
        }
        checked += 1;
    }
    assert!(checked > iters(100_000) / 2, "the oracle skipped too many cases: {checked}");
}

/// Division: with `exact`, a quotient that has a finite decimal is given exactly, and one that has none
/// is refused as imprecise, never rounded. With `round`, an exact quotient is the same number.
#[test]
fn division_matches_the_exact_oracle() {
    let env = MapEnv::default();
    let mut rng = Rng(11);
    for _ in 0..iters(200_000) {
        let (x, y) = (amount(&mut rng), amount(&mut rng));
        let source = format!("{} / {}", literal(x), literal(y));
        let exact = run(&source, &env, Precision::Exact);
        let round = run(&source, &env, Precision::Round);
        if y.is_zero() {
            assert_eq!(exact.map_err(|e| e.kind), Err(ErrorKind::DivisionByZero), "{source}");
            continue;
        }
        match Exact::of(x).div(Exact::of(y)).and_then(Exact::reduced) {
            Some(want) => {
                assert_eq!(exact, Ok(Value::Num(want.decimal())), "{source} (exact)");
                assert_eq!(round, Ok(Value::Num(want.decimal())), "{source} (round)");
            }
            None => {
                assert_eq!(exact.map_err(|e| e.kind), Err(ErrorKind::Precision), "{source}: no finite decimal, so exact must refuse");
                // The rounded quotient is off by at most one unit in its last digit: |q*y - x| <= |y| * 10^-scale(q),
                // worked out in the oracle so the check itself rounds nothing.
                let Ok(Value::Num(q)) = round else { panic!("{source}: round failed: {round:?}") };
                let (qe, ye) = (Exact::of(q), Exact::of(y));
                let diff = qe.mul(ye).and_then(|qy| qy.add(Exact::of(x).neg())).unwrap();
                let bound = ye.m.abs() * 10i128.pow(diff.s - ye.s - qe.s);
                assert!(diff.m.abs() <= bound, "{source}: {q} * {y} is {} away", diff.decimal());
            }
        }
    }
}

/// `pct` and `prorate` are `x * p / 100` and `amount * part / whole`, exactly when that has a finite decimal.
#[test]
fn pct_and_prorate_match_the_exact_oracle() {
    let env = MapEnv::default();
    let mut rng = Rng(13);
    for _ in 0..iters(200_000) {
        let (x, p) = (amount(&mut rng), amount(&mut rng));
        let source = format!("pct({}, {})", literal(x), literal(p));
        let want = Exact::of(x).mul(Exact::of(p)).and_then(|v| v.div(Exact { m: 100, s: 0 })).and_then(Exact::reduced).unwrap();
        for precision in [Precision::Round, Precision::Exact] {
            assert_eq!(run(&source, &env, precision), Ok(Value::Num(want.decimal())), "{source} ({precision:?})");
        }

        let (a, part, whole) = (amount(&mut rng), amount(&mut rng), amount(&mut rng));
        let source = format!("prorate({}, {}, {})", literal(a), literal(part), literal(whole));
        let exact = run(&source, &env, Precision::Exact);
        if whole <= Decimal::ZERO {
            assert_eq!(exact.map_err(|e| e.kind), Err(ErrorKind::InvalidArgument), "{source}");
            continue;
        }
        match Exact::of(a).mul(Exact::of(part)).and_then(|v| v.div(Exact::of(whole))).and_then(Exact::reduced) {
            Some(want) => assert_eq!(exact, Ok(Value::Num(want.decimal())), "{source}"),
            None => assert_eq!(exact.map_err(|e| e.kind), Err(ErrorKind::Precision), "{source}"),
        }
    }
}

/// Progressive tax by bands, and the rate of the band an amount falls in, against a plain integer sum.
#[test]
fn bracket_and_rate_at_match_an_integer_oracle() {
    let mut rng = Rng(17);
    for _ in 0..iters(100_000) {
        // Upper limits in cents, going up; rates in whole percent.
        let bands = 1 + rng.below(5);
        let open_top = rng.below(4) != 0;
        let mut uppers: Vec<i128> = Vec::new();
        let mut limit = 0i128;
        for _ in 0..bands {
            limit += rng.below(500_000) as i128 + if uppers.is_empty() { 0 } else { 1 };
            uppers.push(limit);
        }
        let rates: Vec<i128> = (0..bands).map(|_| rng.below(61) as i128).collect();
        let table = Value::List(
            (0..bands)
                .map(|i| {
                    let upper = if open_top && i + 1 == bands { Value::Null } else { Value::Num(Decimal::from_i128_with_scale(uppers[i], 2)) };
                    Value::List(vec![upper, Value::Num(Decimal::from_i128_with_scale(rates[i], 2))])
                })
                .collect(),
        );
        let x = rng.below(3_000_000) as i128 - 100_000;
        let mut names = BTreeMap::new();
        names.insert("x".to_string(), Value::Num(Decimal::from_i128_with_scale(x, 2)));
        names.insert("table".to_string(), table);
        let env = MapEnv(names);

        // Oracle, in units of 1/10000 (cents times percent).
        let top = uppers[bands - 1];
        let above = !open_top && x > top;
        let mut tax = 0i128;
        let mut lower = 0i128;
        for i in 0..bands {
            let upper = if open_top && i + 1 == bands { i128::MAX } else { uppers[i] };
            let part = x.min(upper) - lower;
            if part > 0 {
                tax += part * rates[i];
            }
            lower = upper;
        }
        let rate = (0..bands).find(|&i| (open_top && i + 1 == bands) || x <= uppers[i]).map(|i| rates[i]);

        for precision in [Precision::Round, Precision::Exact] {
            let got = run("bracket(x, table)", &env, precision);
            if above {
                assert_eq!(got.map_err(|e| e.kind), Err(ErrorKind::InvalidArgument), "x={x} above {top}");
            } else {
                let want = if x <= 0 { Decimal::ZERO } else { Decimal::from_i128_with_scale(tax, 4) };
                assert_eq!(got, Ok(Value::Num(want)), "x={x} uppers={uppers:?} rates={rates:?} open={open_top}");
            }
        }
        match rate {
            Some(r) => assert_eq!(run("rate_at(x, table)", &env, Precision::Exact), Ok(Value::Num(Decimal::from_i128_with_scale(r, 2))), "x={x}"),
            None => assert!(run("rate_at(x, table)", &env, Precision::Exact).is_err(), "x={x}"),
        }
    }
}
