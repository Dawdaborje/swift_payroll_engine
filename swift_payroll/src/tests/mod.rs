#[cfg(test)]
mod helpers;

#[cfg(test)]
mod basic {
    use super::helpers::assert_fixture_dir;

    #[test]
    fn test_basic_fixtures() {
        assert_fixture_dir("basic");
    }
}

#[cfg(test)]
mod intermediate {
    use super::helpers::assert_fixture_dir;

    #[test]
    fn test_intermediate_fixtures() {
        assert_fixture_dir("intermediate");
    }
}

#[cfg(test)]
mod advanced {
    use super::helpers::assert_fixture_dir;
    use crate::fixture::{example_data_root, load_employee_contexts};
    use sp_engine::calculator::CalculationContext;

    #[test]
    fn test_advanced_fixtures() {
        assert_fixture_dir("advanced");
    }

    #[test]
    fn test_cel_rhai_parity_amounts_match() {
        let path = example_data_root().join("advanced/002_cel_rhai_parity.json");
        let pairs = load_employee_contexts(&path).expect("load parity fixture");
        let contexts: Vec<_> = pairs.iter().map(|(_, ctx)| ctx.clone()).collect();
        let results = CalculationContext {
            emp_contexts: contexts,
        }
        .calculate()
        .expect("calculate");

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].gross_salary, results[1].gross_salary);
        assert_eq!(results[0].net_salary, results[1].net_salary);
        assert_eq!(results[0].deductions.len(), results[1].deductions.len());
        for (a, b) in results[0].deductions.iter().zip(results[1].deductions.iter()) {
            assert_eq!(a.name, b.name);
            assert_eq!(a.amount, b.amount);
        }
    }
}

#[cfg(test)]
mod errors {
    use rust_decimal::Decimal;
    use sp_dsl::models::{DslType, PayrollRuleContext};
    use sp_engine::calculator::{CalculationContext, EngineError};
    use sp_engine::models::{
        deduction::EmpDeduction,
        emp_context::{EmployeeContext, EmployeeIdentity},
    };

    #[test]
    fn test_invalid_cel_expression_returns_error() {
        let rule = PayrollRuleContext::new(
            "BAD".to_string(),
            "Bad".to_string(),
            "this is not valid cel!!!".to_string(),
            DslType::CEL,
        )
        .expect_err("invalid CEL should fail at compile time");

        assert!(matches!(rule, sp_dsl::models::PayrollRuleError::Parse(_)));
    }

    #[test]
    fn test_invalid_cel_at_runtime_after_valid_compile() {
        // Compile succeeds; evaluation path is separate from parse in CEL.
        let rule = PayrollRuleContext::new(
            "BAD".to_string(),
            "Bad".to_string(),
            "gross / 0.0".to_string(),
            DslType::CEL,
        )
        .expect("valid cel compiles");
        let emp = EmployeeContext {
            identity: EmployeeIdentity {
                emp_id: "ERR-001".to_string(),
                name: Some("Bad Rule".to_string()),
            },
            base_salary: Decimal::new(10000, 0),
            allowances: vec![],
            deductions: vec![EmpDeduction {
                name: "BAD".to_string(),
                amount: Decimal::ZERO,
                dsl_expr: Some(rule),
            }],
            pay_period: None,
        };

        let err = CalculationContext {
            emp_contexts: vec![emp],
        }
        .calculate()
        .expect_err("division by zero should fail at eval");

        assert!(matches!(err, EngineError::DslEvaluation { .. }));
    }

    #[test]
    fn test_negative_base_salary_returns_error() {
        let emp = EmployeeContext {
            identity: EmployeeIdentity {
                emp_id: "ERR-002".to_string(),
                name: None,
            },
            base_salary: Decimal::new(-100, 0),
            allowances: vec![],
            deductions: vec![],
            pay_period: None,
        };

        let err = CalculationContext {
            emp_contexts: vec![emp],
        }
        .calculate()
        .expect_err("negative salary should fail");

        assert!(matches!(err, EngineError::InvalidInput(_)));
    }
}

/// Stress path: up to 500 employees. Throughput at 1M lives in Criterion benches.
#[cfg(test)]
mod stress {
    use crate::cohort::{expand_profiles, ProfileCohort};
    use crate::fixture::{example_data_root, load_employee_contexts, load_employees};
    use sp_engine::calculator::CalculationContext;

    const MAX_TEST_EMPLOYEES: usize = 500;

    #[test]
    fn test_cohort_500_file_calculates() {
        let path = example_data_root().join("benchmark/cohort_500.json");
        let pairs = load_employee_contexts(&path).expect("load cohort_500");
        assert!(pairs.len() <= MAX_TEST_EMPLOYEES);
        assert_eq!(pairs.len(), 500);

        let taxish = pairs[0]
            .0
            .deductions
            .iter()
            .filter(|d| d.engine == "cel" || d.engine == "rhai")
            .count();
        assert!(
            taxish >= 10,
            "expected ≥10 tax/DSL deductions, got {taxish}"
        );

        let contexts: Vec<_> = pairs.into_iter().map(|(_, ctx)| ctx).collect();
        let results = CalculationContext {
            emp_contexts: contexts,
        }
        .calculate()
        .expect("calculate cohort_500");
        assert_eq!(results.len(), 500);
    }

    #[test]
    fn test_streaming_matches_materialized() {
        let path = example_data_root().join("benchmark/profiles.json");
        let profiles = load_employees(&path).expect("profiles");
        let cohort = ProfileCohort::from_profiles(&profiles).expect("cohort");

        let materialized = expand_profiles(&profiles, MAX_TEST_EMPLOYEES).expect("expand");
        let streamed = CalculationContext::calculate_stream(MAX_TEST_EMPLOYEES, |i| {
            cohort.employee_at(i)
        })
        .expect("stream");

        assert_eq!(streamed.len(), materialized.len());
        let mat_results = CalculationContext {
            emp_contexts: materialized,
        }
        .calculate()
        .expect("materialized");

        for (a, b) in streamed.iter().zip(mat_results.iter()) {
            assert_eq!(a.employee_id, b.employee_id);
            assert_eq!(a.gross_salary, b.gross_salary);
            assert_eq!(a.net_salary, b.net_salary);
        }
    }
}
