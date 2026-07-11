use std::path::Path;

use rust_decimal::Decimal;
use sp_engine::calculator::CalculationContext;

use crate::cli::models::RawExpectedResults;
use crate::fixture::{example_data_root, load_employee_contexts, FixtureError};

fn assert_results_match(
    employee_id: &str,
    expected: &RawExpectedResults,
    gross: Decimal,
    net: Decimal,
    deductions: &[(String, Decimal)],
) {
    assert_eq!(
        expected.gross_salary, gross,
        "gross mismatch for {employee_id}"
    );
    assert_eq!(expected.net_salary, net, "net mismatch for {employee_id}");
    assert_eq!(
        expected.deductions.len(),
        deductions.len(),
        "deduction count mismatch for {employee_id}"
    );
    for (exp, (id, amount)) in expected.deductions.iter().zip(deductions.iter()) {
        assert_eq!(&exp.id, id, "deduction id mismatch for {employee_id}");
        assert_eq!(
            &exp.amount, amount,
            "deduction amount mismatch for {employee_id} / {id}"
        );
    }
}

pub fn assert_fixture_file(relative_path: impl AsRef<Path>) {
    let path = example_data_root().join(relative_path.as_ref());
    let pairs = load_employee_contexts(&path).unwrap_or_else(|e| {
        panic!("failed to load {}: {e}", path.display());
    });

    let contexts: Vec<_> = pairs.iter().map(|(_, ctx)| ctx.clone()).collect();
    let calc = CalculationContext {
        emp_contexts: contexts,
    };
    let results = calc.calculate().expect("calculation should succeed");

    assert_eq!(
        pairs.len(),
        results.len(),
        "employee/result count mismatch in {}",
        path.display()
    );

    for ((raw, _), result) in pairs.iter().zip(results.iter()) {
        let expected = raw.results.as_ref().unwrap_or_else(|| {
            panic!(
                "{}",
                FixtureError::MissingResults(raw.employee_id.clone())
            )
        });
        let deductions: Vec<(String, Decimal)> = result
            .deductions
            .iter()
            .map(|d| (d.name.clone(), d.amount))
            .collect();
        assert_results_match(
            &raw.employee_id,
            expected,
            result.gross_salary,
            result.net_salary,
            &deductions,
        );
    }
}

pub fn assert_fixture_dir(subdir: &str) {
    let dir = example_data_root().join(subdir);
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", dir.display()))
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    files.sort();
    assert!(
        !files.is_empty(),
        "no JSON fixtures found in {}",
        dir.display()
    );
    for file in files {
        let rel = Path::new(subdir).join(file.file_name().unwrap());
        assert_fixture_file(&rel);
    }
}
