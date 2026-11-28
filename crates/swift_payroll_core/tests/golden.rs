//! Golden payslips: whole payslips recorded once, checked by hand (each file's `notes` show the working),
//! and compared field by field on every test run.
//!
//! A payslip must be reproducible years later. If a change to this crate, or an upgrade of `rust_decimal`
//! or `serde_json`, changes any digit of any of these payslips, this test fails. A failure is never fixed by
//! re-recording the file without first working out, by hand, which figure is right.

use std::path::Path;

use serde::Deserialize;
use serde_json::{json, Value as Json};
use swift_payroll_core::{Compiled, Input, Structure};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    #[allow(dead_code)]
    notes: Vec<String>,
    structure: Structure,
    employees: Vec<Input>,
    expected: Vec<Json>,
}

fn check(path: &Path) {
    let text = std::fs::read_to_string(path).unwrap();
    let case: Case = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let compiled = Compiled::new(&case.structure).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    assert_eq!(case.employees.len(), case.expected.len(), "{}: one expected result per employee", path.display());
    for (input, expected) in case.employees.iter().zip(&case.expected) {
        let got = match compiled.run(input) {
            Ok(slip) => serde_json::to_value(slip).unwrap(),
            Err(error) => json!({ "employee_id": input.employee_id, "error": error.to_string() }),
        };
        assert_eq!(&got, expected, "{}: {} differs from the recorded payslip", path.display(), input.employee_id);
    }
}

#[test]
fn every_golden_payslip_is_reproduced_exactly() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden");
    let mut files: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().path()).filter(|p| p.extension().is_some_and(|e| e == "json")).collect();
    files.sort();
    assert!(files.len() >= 5, "the golden cases are missing from {}", dir.display());
    for file in files {
        check(&file);
    }
}
