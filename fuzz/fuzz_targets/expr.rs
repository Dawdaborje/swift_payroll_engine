//! Any text compiled and evaluated as an expression: never a panic, a stack overflow or a hang, and the
//! same answer every time, in both precision modes.
#![no_main]

use std::collections::BTreeMap;

use libfuzzer_sys::fuzz_target;
use swift_payroll_expr::{compile, MapEnv, Precision, Value};

fuzz_target!(|data: &[u8]| {
    let Ok(source) = std::str::from_utf8(data) else { return };
    let Ok(expr) = compile(source) else { return };
    let _ = expr.identifiers();
    let _ = expr.literal_arguments("total");
    let env = MapEnv(BTreeMap::from([
        ("a".to_string(), Value::from(1000)),
        ("b".to_string(), Value::from(-3)),
        ("s".to_string(), Value::from("text")),
        ("t".to_string(), Value::Bool(true)),
        ("l".to_string(), Value::List(vec![Value::from(1), Value::from(2)])),
    ]));
    for precision in [Precision::Round, Precision::Exact] {
        let first = format!("{:?}", expr.eval_with(&env, precision));
        let second = format!("{:?}", expr.eval_with(&env, precision));
        assert_eq!(first, second, "evaluation is not deterministic for {source:?}");
    }
});
